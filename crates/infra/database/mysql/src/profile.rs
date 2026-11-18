//! MySQL's and MariaDB's profiles, pure (tested without a server): what a version's
//! profiles can hold ([`caps`]), the plan read into the normalized [`Profile`] from either
//! of their answers, and the session's counters around a run ([`counters`]).
//!
//! | Engine | Plan | As it ran |
//! |---|---|---|
//! | MySQL 5.7 | `EXPLAIN FORMAT=JSON` | — |
//! | MySQL 8.0.18+ | `EXPLAIN FORMAT=JSON` | `EXPLAIN ANALYZE` (a tree of text: [`parse_tree`]) |
//! | MariaDB 10.1+ | `EXPLAIN FORMAT=JSON` | `ANALYZE FORMAT=JSON` (the plan with its `r_*` figures) |
//!
//! [`parse_json`] reads both engines' JSON: a table's `access_type` is its kind (`ALL` a
//! scan, `index` a full index scan, `range` / `ref` / `eq_ref` / `const` an index's
//! lookups, covering with `using_index`), `key` its index, `rows_examined_per_scan` (MySQL)
//! or `rows` (MariaDB) its estimate, `attached_condition` its filter, `cost_info` its costs;
//! `nested_loop` a join, `ordering_operation` / `filesort` a sort, `grouping_operation` an
//! aggregate, `duplicates_removal` a distinct, `windowing` a window, `union_result` a
//! union, `temporary_table` a materialization, and every kind of subquery a sub-plan.
//! MariaDB's `r_loops`, `r_rows`, `r_filtered`, `r_table_time_ms` + `r_other_time_ms` (the
//! time of all loops) are what it really did. Any key not known becomes an [`Extra`] of its
//! node (`used_columns`, `possible_keys`, `r_engine_stats`…, a newer version's): nothing is
//! dropped. MySQL's single-table writes have no tree as they ran (`<not executable by
//! iterator executor>`): their profile is the plan with the statement's own time and
//! counters.

use database::DbError;
use database::capabilities::ProfileCaps;
use database::mysql::ServerVersion;
use database::profile::{
    AggStrategy, ChildRole, CondKind, Condition, EngineCounters, Extra, ExtraValue, JoinKind,
    NodeId, ObjectKind, ObjectRef, OpKind, Profile, ProfileDepth, ProfileNode, Quantity,
    ScanDirection, Unit, WriteKind,
};
use serde_json::{Map, Value as Json};

/// The savepoint a profile run inside the user's transaction is undone to.
pub const SAVEPOINT: &str = "console_profile";

/// What MySQL's `EXPLAIN ANALYZE` says of a statement it can't run as a tree (a
/// single-table write before 8.0.?).
pub const NOT_A_TREE: &str = "<not executable by iterator executor>";

/// Whether `version` can show a plan as it ran: MySQL 8.0.18's `EXPLAIN ANALYZE`, MariaDB
/// 10.1's `ANALYZE FORMAT=JSON`.
pub const fn runs(version: ServerVersion) -> bool {
    if version.mariadb { version.number >= 100100 } else { version.number >= 80018 }
}

/// What a version's profiles can hold.
pub const fn caps(version: ServerVersion) -> ProfileCaps {
    let actual = runs(version);
    ProfileCaps {
        estimated: true,
        actual,
        node_timing: actual,
        engine_counters: actual,
        table_stats: true,
        undo_writes: actual,
        ..ProfileCaps::NONE
    }
}

/// The session's counters read before and after a run (`SHOW SESSION STATUS`): the
/// handler's reads, the scans and joins without an index, sorts' passes, temporary tables.
pub const COUNTERS: &[&str] = &[
    "Created_tmp_disk_tables",
    "Created_tmp_tables",
    "Handler_read_first",
    "Handler_read_key",
    "Handler_read_last",
    "Handler_read_next",
    "Handler_read_prev",
    "Handler_read_rnd",
    "Handler_read_rnd_next",
    "Select_full_join",
    "Select_full_range_join",
    "Select_range",
    "Select_range_check",
    "Select_scan",
    "Sort_merge_passes",
    "Sort_range",
    "Sort_rows",
    "Sort_scan",
];

/// The query reading [`COUNTERS`].
pub fn status_sql() -> String {
    let names: Vec<String> = COUNTERS.iter().map(|c| format!("'{c}'")).collect();
    format!("SHOW SESSION STATUS WHERE Variable_name IN ({})", names.join(", "))
}

/// The counters' rows (`name`, `value`) as numbers.
pub fn status_values(rows: &[Vec<Option<String>>]) -> Vec<(String, u64)> {
    rows.iter()
        .filter_map(|r| {
            let name = r.first().cloned().flatten()?;
            let value = r.get(1).cloned().flatten()?.trim().parse().ok()?;
            Some((name, value))
        })
        .collect()
}

/// What the run added to each counter: the engine's counters the model knows, every one
/// as an extra (`Session status`).
pub fn counters(before: &[(String, u64)], after: &[(String, u64)]) -> (EngineCounters, Vec<Extra>) {
    let delta = |name: &str| -> Option<u64> {
        let a = after.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?.1;
        let b = before.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map_or(0, |(_, v)| *v);
        Some(a.saturating_sub(b))
    };
    let reads: Option<u64> = [
        "Handler_read_first",
        "Handler_read_key",
        "Handler_read_last",
        "Handler_read_next",
        "Handler_read_prev",
        "Handler_read_rnd",
        "Handler_read_rnd_next",
    ]
    .iter()
    .filter_map(|n| delta(n))
    .reduce(|a, b| a + b);
    let counters = EngineCounters {
        // The rows the storage engine handed over, every read counted.
        rows_examined: reads,
        select_scan: delta("Select_scan"),
        sort_merge_passes: delta("Sort_merge_passes"),
        tmp_disk_tables: delta("Created_tmp_disk_tables"),
        ..EngineCounters::default()
    };
    let extras = COUNTERS
        .iter()
        .filter_map(|n| {
            let value = delta(n)?;
            let quantity = Quantity::new(value as f64, Unit::Count);
            Some(Extra::new(*n, ExtraValue::Quantity(quantity)).in_group("Session status"))
        })
        .collect();
    (counters, extras)
}

/// `performance_schema`'s record of the session's last statement starting with `prefix`
/// (the profiled one; the user may not read it — then nothing is known): rows examined and
/// sent, whether no index (or no good one) was used, the time its locks took, its
/// temporary tables on disk, its sorts' passes, its full scans.
pub fn history_sql(prefix: &str) -> String {
    format!(
        "SELECT ROWS_EXAMINED, ROWS_SENT, NO_INDEX_USED, NO_GOOD_INDEX_USED, LOCK_TIME, \
         CREATED_TMP_DISK_TABLES, SORT_MERGE_PASSES, SELECT_SCAN \
         FROM performance_schema.events_statements_history \
         WHERE THREAD_ID = (SELECT THREAD_ID FROM performance_schema.threads \
         WHERE PROCESSLIST_ID = CONNECTION_ID()) AND SQL_TEXT LIKE '{prefix}%' \
         ORDER BY EVENT_ID DESC LIMIT 1"
    )
}

/// That record read: the rows it examined (the engine's own count, better than the
/// handler's reads), the rest as extras (`performance_schema`).
pub fn history_row(row: &[Option<String>]) -> (Option<u64>, Vec<Extra>) {
    let number =
        |i: usize| row.get(i).cloned().flatten().and_then(|t| t.trim().parse::<u64>().ok());
    let group = "performance_schema";
    let mut extras = Vec::new();
    let count = |key: &str, value: Option<u64>, unit: Unit| {
        value.map(|v| {
            Extra::new(key, ExtraValue::Quantity(Quantity::new(v as f64, unit))).in_group(group)
        })
    };
    extras.extend(count("ROWS_EXAMINED", number(0), Unit::Rows));
    extras.extend(count("ROWS_SENT", number(1), Unit::Rows));
    for (i, key) in [(2, "NO_INDEX_USED"), (3, "NO_GOOD_INDEX_USED")] {
        if let Some(flag) = number(i) {
            extras.push(Extra::new(key, ExtraValue::Bool(flag != 0)).in_group(group));
        }
    }
    // Picoseconds.
    if let Some(lock) = number(4) {
        let ms = lock as f64 / 1e9;
        extras.push(
            Extra::new("LOCK_TIME", ExtraValue::Quantity(Quantity::new(ms, Unit::Millis)))
                .in_group(group),
        );
    }
    extras.extend(count("CREATED_TMP_DISK_TABLES", number(5), Unit::Count));
    extras.extend(count("SORT_MERGE_PASSES", number(6), Unit::Count));
    extras.extend(count("SELECT_SCAN", number(7), Unit::Count));
    (number(0), extras)
}

fn invalid(why: impl std::fmt::Display) -> DbError {
    DbError::Query(format!("the server's plan can't be read: {why}"))
}

/// A number MySQL writes as a number or as text (`"10.00"`, `"476K"` stays text).
fn number(v: &Json) -> Option<f64> {
    match v {
        Json::Number(n) => n.as_f64(),
        Json::String(s) => s.trim().parse().ok(),
        Json::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

fn truthy(v: &Json) -> bool {
    match v {
        Json::Bool(b) => *b,
        Json::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Json::String(s) => !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false"),
        _ => false,
    }
}

/// An unknown value as an extra's (an object a group, a list a list: nothing thrown
/// away).
pub fn extra_value(v: &Json) -> ExtraValue {
    match v {
        Json::Null => ExtraValue::Text(String::new()),
        Json::Bool(b) => ExtraValue::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => ExtraValue::Int(i),
            None => ExtraValue::Float(n.as_f64().unwrap_or_default()),
        },
        Json::String(s) => ExtraValue::Text(s.clone()),
        Json::Array(items) => ExtraValue::List(items.iter().map(extra_value).collect()),
        Json::Object(map) => ExtraValue::Group(
            map.iter().map(|(k, v)| Extra::new(k.clone(), extra_value(v))).collect(),
        ),
    }
}

/// The kinds of subqueries a block or a table lists, each a sub-plan.
const SUBQUERIES: &[&str] = &[
    "attached_subqueries",
    "optimized_away_subqueries",
    "having_subqueries",
    "select_list_subqueries",
    "order_by_subqueries",
    "group_by_subqueries",
    "subqueries",
];

/// The operations that wrap what they work on (the same shape as a block's body inside).
fn operation(key: &str) -> Option<(OpKind, &'static str)> {
    Some(match key {
        "ordering_operation" => (OpKind::Sort, "Ordering"),
        "grouping_operation" => (OpKind::Aggregate(AggStrategy::Sorted), "Grouping"),
        "duplicates_removal" => (OpKind::Unique, "Duplicates removal"),
        "windowing" => (OpKind::WindowAgg, "Windowing"),
        "filesort" => (OpKind::Sort, "Filesort"),
        "temporary_table" => (OpKind::Materialize, "Temporary table"),
        "read_sorted_file" => (OpKind::Sort, "Read sorted file"),
        "union_result" => (OpKind::Append, "Union"),
        "block-nl-join" => (OpKind::NestedLoop, "Block nested loop"),
        _ => return None,
    })
}

/// Reads MySQL's and MariaDB's `EXPLAIN FORMAT=JSON` / MariaDB's `ANALYZE FORMAT=JSON`.
pub fn parse_json(json: &str, depth: ProfileDepth) -> Result<Profile, DbError> {
    let root: Json = serde_json::from_str(json.trim()).map_err(invalid)?;
    let map = root.as_object().ok_or_else(|| invalid("not an object"))?;
    let mut reader = JsonReader { profile: Profile::new(Default::default(), depth) };
    let mut found = false;
    for (key, v) in map {
        match (key.as_str(), v) {
            ("query_block", Json::Object(block)) => {
                found = true;
                reader.top_block(block);
            }
            // MariaDB's planning.
            ("query_optimization", Json::Object(o)) => {
                reader.profile.statement.planning_ms = o.get("r_total_time_ms").and_then(number);
            }
            (key, v) => reader.profile.statement.extra.push(Extra::new(key, extra_value(v))),
        }
    }
    if !found {
        return Err(invalid("no query_block"));
    }
    let mut profile = reader.profile;
    finish(&mut profile);
    Ok(profile)
}

struct JsonReader {
    profile: Profile,
}

impl JsonReader {
    fn node_mut(&mut self, id: NodeId) -> &mut ProfileNode {
        &mut self.profile.nodes[id.index()]
    }

    /// The statement's own block: its contents become the plan's roots.
    fn top_block(&mut self, block: &Map<String, Json>) {
        for (key, v) in block {
            match key.as_str() {
                "select_id" => {}
                "cost_info" => {
                    if let Some(cost) = v.get("query_cost").and_then(number) {
                        self.profile.statement.extra.push(Extra::new(
                            "query_cost",
                            ExtraValue::Quantity(Quantity::new(cost, Unit::Cost)),
                        ));
                    }
                }
                "r_total_time_ms" => self.profile.statement.execution_ms = number(v),
                "message" => {
                    let mut node = ProfileNode::new(OpKind::Result, v.as_str().unwrap_or_default());
                    node.extra.push(Extra::new("message", extra_value(v)));
                    self.profile.push(None, node);
                }
                _ => {
                    if !self.content(None, None, key, v) {
                        self.profile.statement.extra.push(Extra::new(key.as_str(), extra_value(v)));
                    }
                }
            }
        }
    }

    /// One key of a body under `parent` (`None`: the plan's top): `true` if it made nodes.
    fn content(
        &mut self,
        parent: Option<NodeId>,
        role: Option<ChildRole>,
        key: &str,
        v: &Json,
    ) -> bool {
        match (key, v) {
            ("table", Json::Object(table)) => {
                self.table(parent, role, table);
                true
            }
            ("nested_loop", Json::Array(items)) => {
                let join =
                    self.profile.push(parent, ProfileNode::new(OpKind::NestedLoop, "Nested loop"));
                let node = self.node_mut(join);
                node.join = Some(JoinKind::Inner);
                node.role = role;
                for (i, item) in items.iter().enumerate() {
                    let role = Some(if i == 0 { ChildRole::Outer } else { ChildRole::Inner });
                    if let Json::Object(item) = item {
                        for (k, v) in item {
                            if !self.content(Some(join), role, k, v) {
                                let extra = Extra::new(k.as_str(), extra_value(v));
                                self.node_mut(join).extra.push(extra);
                            }
                        }
                    }
                }
                true
            }
            ("query_block", Json::Object(block)) => {
                self.sub_block(parent, role.or(Some(ChildRole::Subquery)), block);
                true
            }
            ("query_specifications", Json::Array(items)) => {
                for item in items {
                    if let Some(Json::Object(block)) = item.get("query_block") {
                        self.sub_block(parent, Some(ChildRole::Member), block);
                    }
                }
                true
            }
            (key, Json::Array(items)) if SUBQUERIES.contains(&key) => {
                for item in items {
                    self.subquery(parent, ChildRole::SubPlan, item);
                }
                true
            }
            ("materialized_from_subquery" | "materialized", Json::Object(m)) => {
                self.subquery(parent, ChildRole::Subquery, &Json::Object(m.clone()));
                true
            }
            ("expression_cache", Json::Object(m)) => {
                self.subquery(parent, ChildRole::SubPlan, &Json::Object(m.clone()));
                true
            }
            (key, Json::Object(inner)) => match operation(key) {
                Some((op, name)) => {
                    self.operation(parent, role, op, name, inner);
                    true
                }
                None if inner
                    .keys()
                    .any(|k| matches!(k.as_str(), "table" | "nested_loop" | "query_block")) =>
                {
                    self.operation(parent, role, OpKind::Other, key, inner);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    /// A subquery's entry (`{ dependent, cacheable, query_block }`) as a sub-plan.
    fn subquery(&mut self, parent: Option<NodeId>, role: ChildRole, item: &Json) {
        let Json::Object(map) = item else { return };
        let Some(Json::Object(block)) = map.get("query_block") else {
            // MariaDB's expression cache wraps one.
            for (k, v) in map {
                self.content(parent, Some(role), k, v);
            }
            return;
        };
        let id = self.sub_block(parent, Some(role), block);
        for (k, v) in map.iter().filter(|(k, _)| *k != "query_block") {
            let extra = Extra::new(k.as_str(), extra_value(v));
            self.node_mut(id).extra.push(extra);
        }
    }

    /// A block of its own (`Select #n`): a node, its contents under it.
    fn sub_block(
        &mut self,
        parent: Option<NodeId>,
        role: Option<ChildRole>,
        block: &Map<String, Json>,
    ) -> NodeId {
        let select = block.get("select_id").and_then(number).map(|n| n as u32);
        let name = select.map_or_else(|| "Subquery".to_owned(), |n| format!("Select #{n}"));
        let id = self.profile.push(parent, ProfileNode::new(OpKind::Other, name));
        self.node_mut(id).role = role;
        self.body(id, block);
        id
    }

    /// An operation over what its object holds.
    fn operation(
        &mut self,
        parent: Option<NodeId>,
        role: Option<ChildRole>,
        op: OpKind,
        name: &str,
        inner: &Map<String, Json>,
    ) {
        let id = self.profile.push(parent, ProfileNode::new(op, name));
        self.node_mut(id).role = role;
        self.body(id, inner);
        let node = self.node_mut(id);
        // An ordering that needs no sort (the index's order) is no sort.
        if name == "Ordering"
            && !node
                .extra
                .iter()
                .any(|e| e.key == "using_filesort" && e.value == ExtraValue::Bool(true))
        {
            node.op = OpKind::Other;
        }
        if name == "Grouping" {
            let temp = node
                .extra
                .iter()
                .any(|e| e.key == "using_temporary_table" && e.value == ExtraValue::Bool(true));
            if temp {
                node.op = OpKind::Aggregate(AggStrategy::Hashed);
            }
        }
    }

    /// Every key of `map` under node `id`: what makes nodes, then its own figures.
    fn body(&mut self, id: NodeId, map: &Map<String, Json>) {
        for (key, v) in map {
            if self.content(Some(id), None, key, v) {
                continue;
            }
            scalar(self.node_mut(id), key, v);
        }
    }

    /// A table's access.
    fn table(&mut self, parent: Option<NodeId>, role: Option<ChildRole>, map: &Map<String, Json>) {
        let id = self.profile.push(parent, ProfileNode::new(OpKind::Other, ""));
        self.node_mut(id).role = role;
        self.body(id, map);
        let node = self.node_mut(id);
        let access = map.get("access_type").and_then(Json::as_str).unwrap_or_default().to_owned();
        let covering = map.get("using_index").is_some_and(truthy);
        node.op = match access.as_str() {
            "ALL" => OpKind::SeqScan,
            "system" => OpKind::Result,
            "" => OpKind::Other,
            _ if covering => OpKind::IndexOnlyScan,
            _ => OpKind::IndexScan,
        };
        let write =
            ["update", "delete", "insert"].into_iter().find(|w| map.get(*w).is_some_and(truthy));
        if let Some(write) = write {
            node.extra.push(Extra::new("access", ExtraValue::Text(node.op.id().to_owned())));
            node.op = OpKind::ModifyTable(match write {
                "update" => WriteKind::Update,
                "delete" => WriteKind::Delete,
                _ => WriteKind::Insert,
            });
        }
        node.name = if access.is_empty() {
            map.get("message").and_then(Json::as_str).unwrap_or("table").to_owned()
        } else {
            access.clone()
        };
        if !access.is_empty() {
            node.extra.push(Extra::new("access_type", ExtraValue::Text(access)));
        }
    }
}

/// One of a node's own keys into its place (what isn't known: an extra).
fn scalar(node: &mut ProfileNode, key: &str, v: &Json) {
    let percent =
        |v: &Json| number(v).map(|n| ExtraValue::Quantity(Quantity::new(n, Unit::Percent)));
    let millis = |v: &Json| number(v).map(|n| ExtraValue::Quantity(Quantity::new(n, Unit::Millis)));
    match key {
        "table_name" => {
            let name = v.as_str().unwrap_or_default();
            let kind = if name.starts_with('<') { ObjectKind::Other } else { ObjectKind::Table };
            node.object = Some(ObjectRef::new(kind, name));
        }
        "access_type" | "using_index" | "update" | "delete" | "insert" | "select_id" => {}
        "key" => node.index = v.as_str().map(str::to_owned),
        "rows_examined_per_scan" | "rows" => node.estimates.rows = number(v),
        "cost_info" => {
            if let Json::Object(costs) = v {
                node.estimates.total_cost = costs.get("prefix_cost").and_then(number);
                for (k, c) in costs {
                    let value = match number(c) {
                        Some(n) if k != "data_read_per_join" => {
                            ExtraValue::Quantity(Quantity::new(n, Unit::Cost))
                        }
                        _ => extra_value(c),
                    };
                    node.extra.push(Extra::new(format!("cost_info.{k}"), value).in_group("Cost"));
                }
            }
        }
        // MariaDB 11's own cost.
        "cost" => {
            node.estimates.total_cost = number(v);
        }
        "attached_condition" | "where_condition" => {
            if let Some(text) = v.as_str() {
                node.conditions.push(Condition::new(CondKind::Filter, text));
            }
        }
        "index_condition" | "pushed_index_condition" => {
            if let Some(text) = v.as_str() {
                node.conditions.push(Condition::new(CondKind::IndexCond, text));
            }
        }
        "having_condition" => {
            if let Some(text) = v.as_str() {
                node.conditions.push(Condition::new(CondKind::Other("Having".into()), text));
            }
        }
        "sort_key" => {
            if let Some(text) = v.as_str() {
                node.conditions.push(Condition::new(CondKind::SortKey, text));
            }
        }
        "r_loops" => node.actuals.loops = number(v),
        "r_rows" | "r_output_rows" => node.actuals.rows = number(v),
        "r_total_time_ms" => node.actuals.total_ms = number(v),
        "filtered" | "r_filtered" | "r_total_filtered" => {
            if let Some(value) = percent(v) {
                node.extra.push(Extra::new(key, value));
            }
        }
        "r_table_time_ms" | "r_other_time_ms" => {
            if let Some(value) = millis(v) {
                node.extra.push(Extra::new(key, value));
            }
        }
        _ => node.extra.push(Extra::new(key, extra_value(v))),
    }
}

/// MariaDB's figures made per loop (it says the time of all loops), a filter's rows
/// removed from its `r_filtered`, the I/O-free totals; the plan's size noted.
fn finish(profile: &mut Profile) {
    for node in &mut profile.nodes {
        let extra =
            |key: &str| node.extra.iter().find(|e| e.key == key).and_then(|e| e.value.number());
        let (table_ms, other_ms, filtered) =
            (extra("r_table_time_ms"), extra("r_other_time_ms"), extra("r_filtered"));
        let loops = node.actuals.loops.filter(|l| *l > 0.0);
        let spent = match (table_ms, other_ms) {
            (None, None) => node.actuals.total_ms,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
        if let (Some(spent), Some(loops)) = (spent, loops) {
            node.actuals.total_ms = Some(spent / loops);
        }
        if node.actuals.loops == Some(0.0) {
            node.actuals.never_executed = true;
        }
        let filters = node.conditions.iter().any(|c| c.kind == CondKind::Filter);
        if let (Some(rows), Some(kept), true) = (node.actuals.rows, filtered, filters) {
            let kept = (kept / 100.0).clamp(0.0, 1.0);
            if kept < 1.0 {
                let left = rows * kept;
                node.actuals.rows_removed_by_filter = Some(rows - left);
                node.actuals.rows = Some(left);
            }
        }
    }
    profile.mark_size();
}

/// What a line of MySQL's tree (`-> Table scan on c  (cost=205 rows=2000) (actual
/// time=0.239..1.07 rows=2000 loops=1)`) says: its words and its figures.
#[derive(Debug, Default, PartialEq)]
struct TreeLine {
    words: String,
    startup_cost: Option<f64>,
    total_cost: Option<f64>,
    rows: Option<f64>,
    actual: Option<(f64, f64, f64, f64)>,
    never: bool,
}

/// The figures after a line's words.
fn tree_line(text: &str) -> TreeLine {
    let mut line = TreeLine::default();
    let markers = ["(cost=", "(actual time=", "(never executed)", "(rows="];
    let start = markers
        .iter()
        .filter_map(|m| {
            let mut from = 0;
            // The figures' own parenthesis follows a space (a condition's doesn't name them).
            while let Some(at) = text[from..].find(m) {
                let at = from + at;
                if at > 0 && text.as_bytes()[at - 1] == b' ' {
                    return Some(at);
                }
                from = at + 1;
            }
            None
        })
        .min();
    let (words, figures) = match start {
        Some(at) => (&text[..at], &text[at..]),
        None => (text, ""),
    };
    line.words = words.trim().to_owned();
    for group in figures.split('(').skip(1) {
        let group = group.trim().trim_end_matches(')').trim();
        if group.starts_with("never executed") {
            line.never = true;
            continue;
        }
        let actual = group.strip_prefix("actual ");
        let mut time = None;
        let mut rows = None;
        let mut loops = None;
        let mut cost = None;
        for part in actual.unwrap_or(group).split_whitespace() {
            let Some((name, value)) = part.split_once('=') else { continue };
            let pair = |v: &str| -> Option<(Option<f64>, f64)> {
                match v.split_once("..") {
                    Some((a, b)) => Some((a.parse().ok(), b.parse().ok()?)),
                    None => Some((None, v.parse().ok()?)),
                }
            };
            match name {
                "time" => time = pair(value),
                "cost" => cost = pair(value),
                "rows" => rows = value.parse::<f64>().ok(),
                "loops" => loops = value.parse::<f64>().ok(),
                _ => {}
            }
        }
        if actual.is_some() {
            let (first, all) = time.unwrap_or((None, 0.0));
            line.actual =
                Some((first.unwrap_or(0.0), all, rows.unwrap_or(0.0), loops.unwrap_or(1.0)));
        } else {
            if let Some((startup, total)) = cost {
                line.startup_cost = startup;
                line.total_cost = Some(total);
            }
            line.rows = rows.or(line.rows);
        }
    }
    line
}

/// The text after `prefix` up to ` using `, and what follows `using` (its index).
fn on_using(words: &str, prefix: &str) -> Option<(String, Option<String>, String)> {
    let rest = words.strip_prefix(prefix)?;
    let (object, after) = match rest.split_once(" using ") {
        Some((object, after)) => (object.trim(), after),
        None => {
            let (object, after) = rest.split_once(' ').unwrap_or((rest, ""));
            return Some((object.to_owned(), None, after.trim().to_owned()));
        }
    };
    let (index, after) = after.split_once(' ').unwrap_or((after, ""));
    Some((object.to_owned(), Some(index.to_owned()), after.trim().to_owned()))
}

/// A tree line's words read into a node: its kind, object, index and conditions.
fn tree_node(words: &str) -> ProfileNode {
    let mut node = ProfileNode::new(OpKind::guess(words), words);
    // The server's own (`<temporary>`, `<subquery2>`): named in the words, no table.
    let object =
        |name: &str| (!name.starts_with('<')).then(|| ObjectRef::new(ObjectKind::Table, name));
    let lower = words.to_ascii_lowercase();
    let scans: [(&str, OpKind); 12] = [
        ("Table scan on ", OpKind::SeqScan),
        ("Covering index skip scan on ", OpKind::IndexOnlyScan),
        ("Index skip scan on ", OpKind::IndexScan),
        ("Covering index range scan on ", OpKind::IndexOnlyScan),
        ("Index range scan on ", OpKind::IndexScan),
        ("Covering index scan on ", OpKind::IndexOnlyScan),
        ("Index scan on ", OpKind::IndexScan),
        ("Single-row covering index lookup on ", OpKind::IndexOnlyScan),
        ("Single-row index lookup on ", OpKind::IndexScan),
        ("Covering index lookup on ", OpKind::IndexOnlyScan),
        ("Index lookup on ", OpKind::IndexScan),
        ("Full-text index search on ", OpKind::IndexScan),
    ];
    for (prefix, op) in scans {
        if let Some((name, index, rest)) = on_using(words, prefix) {
            node.op = op;
            node.object = object(&name);
            node.index = index;
            let rest = rest.trim();
            if rest.contains("(reverse)") {
                node.scan_direction = Some(ScanDirection::Backward);
            }
            let cond = rest.strip_prefix("over ").unwrap_or(rest).trim();
            let cond = cond.trim_end_matches("(reverse)").trim();
            if !cond.is_empty() {
                node.conditions.push(Condition::new(CondKind::IndexCond, cond));
            }
            return node;
        }
    }
    let after = |prefix: &str| words.strip_prefix(prefix).map(str::trim);
    if let Some(cond) = after("Filter:") {
        node.op = OpKind::Other;
        node.name = "Filter".to_owned();
        node.conditions.push(Condition::new(CondKind::Filter, cond));
    } else if let Some(keys) = after("Sort:")
        .or_else(|| after("Sort row IDs:"))
        .or_else(|| after("Sort with duplicate removal:"))
    {
        node.op = OpKind::Sort;
        node.conditions.push(Condition::new(CondKind::SortKey, keys));
    } else if lower.starts_with("group aggregate") {
        node.op = OpKind::Aggregate(AggStrategy::Sorted);
    } else if lower.starts_with("aggregate using temporary table") {
        node.op = OpKind::Aggregate(AggStrategy::Hashed);
    } else if lower.starts_with("aggregate") {
        node.op = OpKind::Aggregate(AggStrategy::Plain);
    } else if lower.starts_with("nested loop") {
        node.op = OpKind::NestedLoop;
        node.join = Some(join_of(&lower));
    } else if lower.contains("hash join")
        || lower.starts_with("hash semijoin")
        || lower.starts_with("hash antijoin")
    {
        node.op = OpKind::HashJoin;
        node.join = Some(join_of(&lower));
        if let Some(open) = words.find('(') {
            let cond = words[open..].trim();
            node.conditions.push(Condition::new(CondKind::HashCond, cond));
        }
    } else if lower == "hash" {
        node.op = OpKind::Hash;
    } else if lower.starts_with("limit") {
        node.op = OpKind::Limit;
    } else if lower.starts_with("materialize") || lower.starts_with("temporary table") {
        node.op = OpKind::Materialize;
        if lower.starts_with("materialize cte") || lower.starts_with("materialize recursive cte") {
            node.role = Some(ChildRole::Cte);
        }
    } else if lower.starts_with("window") {
        node.op = OpKind::WindowAgg;
    } else if lower.starts_with("remove duplicate") {
        node.op = OpKind::Unique;
    } else if lower.starts_with("union")
        || lower.starts_with("append")
        || lower.starts_with("intersect")
        || lower.starts_with("except")
    {
        node.op = OpKind::Append;
    } else if lower.starts_with("select #") {
        node.op = OpKind::Other;
        node.role = Some(ChildRole::SubPlan);
    } else if lower.starts_with("rows fetched before execution")
        || lower.starts_with("zero rows")
        || lower.starts_with("constant row")
    {
        node.op = OpKind::Result;
    } else if let Some(table) = after("Update ") {
        node.op = OpKind::ModifyTable(WriteKind::Update);
        node.object = object(table.split_whitespace().next().unwrap_or(table));
    } else if let Some(table) = after("Delete from ") {
        node.op = OpKind::ModifyTable(WriteKind::Delete);
        node.object = object(table.split_whitespace().next().unwrap_or(table));
    } else if let Some(table) = after("Insert into ") {
        node.op = OpKind::ModifyTable(WriteKind::Insert);
        node.object = object(table.split_whitespace().next().unwrap_or(table));
    } else if lower.starts_with("stream results") {
        node.op = OpKind::Other;
    }
    node
}

fn join_of(lower: &str) -> JoinKind {
    if lower.contains("left") {
        JoinKind::Left
    } else if lower.contains("antijoin") {
        JoinKind::Anti
    } else if lower.contains("semijoin") {
        JoinKind::Semi
    } else if lower.contains("outer") {
        JoinKind::Full
    } else {
        JoinKind::Inner
    }
}

/// Reads MySQL's `EXPLAIN ANALYZE` (or `EXPLAIN FORMAT=TREE`): a node a line, its depth by
/// its indentation (four spaces a level), its figures per loop (the time of its first and
/// of all its rows, its rows, its loops). A filter over a single scan gives the scan its
/// condition and the rows it removed (as PostgreSQL's scans say them).
pub fn parse_tree(text: &str, depth: ProfileDepth) -> Result<Profile, DbError> {
    let mut profile = Profile::new(Default::default(), depth);
    let mut stack: Vec<(usize, NodeId)> = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        let Some(body) = trimmed.strip_prefix("->") else {
            // A line of its own words (a long condition broken): the node before's.
            if let Some((_, last)) = stack.last()
                && !trimmed.is_empty()
            {
                let node = &mut profile.nodes[last.index()];
                node.name.push(' ');
                node.name.push_str(trimmed.trim());
            }
            continue;
        };
        let indent = raw.len() - trimmed.len();
        let line = tree_line(body.trim());
        let mut node = tree_node(&line.words);
        node.estimates.startup_cost = line.startup_cost;
        node.estimates.total_cost = line.total_cost;
        node.estimates.rows = line.rows;
        if let Some((first, all, rows, loops)) = line.actual {
            node.actuals.startup_ms = Some(first);
            node.actuals.total_ms = Some(all);
            node.actuals.rows = Some(rows);
            node.actuals.loops = Some(loops);
        }
        if line.never {
            node.actuals.never_executed = true;
            node.actuals.loops = Some(0.0);
        }
        while stack.last().is_some_and(|(d, _)| *d >= indent) {
            stack.pop();
        }
        let parent = stack.last().map(|(_, id)| *id);
        let id = profile.push(parent, node);
        stack.push((indent, id));
    }
    if profile.nodes.is_empty() {
        return Err(invalid("no step"));
    }
    fold_filters(&mut profile);
    if let Some(root) = profile.roots.first().map(|r| profile.node(*r).clone())
        && let Some(ms) = root.actuals.total_ms
    {
        profile.statement.execution_ms = Some(ms * root.actuals.loops.unwrap_or(1.0).max(1.0));
    }
    profile.mark_size();
    Ok(profile)
}

/// A `Filter` whose only child is a scan: the scan gets its condition and the rows it
/// removed (its rows read less the filter's), as the analysis reads scans.
fn fold_filters(profile: &mut Profile) {
    for i in 0..profile.nodes.len() {
        let filter = &profile.nodes[i];
        if filter.name != "Filter" || filter.children.len() != 1 {
            continue;
        }
        let child = filter.children[0];
        let Some(cond) = filter.condition(&CondKind::Filter).map(str::to_owned) else { continue };
        let kept = filter.total_rows();
        let scan = &mut profile.nodes[child.index()];
        if !scan.op.is_scan() {
            continue;
        }
        if scan.condition(&CondKind::Filter).is_none() {
            scan.conditions.push(Condition::new(CondKind::Filter, cond));
        }
        if let (Some(read), Some(kept)) = (scan.total_rows(), kept) {
            let loops = scan.actuals.loops.unwrap_or(1.0).max(1.0);
            scan.actuals.rows_removed_by_filter = Some(((read - kept) / loops).max(0.0));
        }
    }
}

/// Why `statement` (read as `kind`) isn't profiled as it runs on MySQL: what can't be
/// undone (a schema change commits by itself, a transaction's own statements).
pub fn refused(kind: database::sql::StatementKind, statement: &str) -> Option<String> {
    use database::sql::StatementKind as K;
    if let Some(words) = database::profile::actual_refused(statement, database::sql::Flavor::MySql)
    {
        return Some(words);
    }
    match kind {
        K::Ddl(_) => Some("a change of the schema (it commits by itself)".to_owned()),
        K::Transaction(_) => Some("a transaction's statement".to_owned()),
        K::Session => Some("a session's statement".to_owned()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/profile.rs"]
mod tests;
