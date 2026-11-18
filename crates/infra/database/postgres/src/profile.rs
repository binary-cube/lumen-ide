//! PostgreSQL's profiles, pure (tested without a server): what `EXPLAIN` is asked on each
//! version ([`options`], 9.2 to 18), what a version's profiles can hold ([`caps`]), and its
//! JSON answer read into the normalized [`Profile`] ([`parse`]). Every key the model knows
//! has its place ([`KEYS`]); any other one — of a node, a worker, the statement, a newer
//! version — becomes an [`Extra`] with its type, grouped by what it belongs to: nothing is
//! dropped.
//!
//! | What | Since | Used |
//! |---|---|---|
//! | `FORMAT JSON`, `ANALYZE`, `VERBOSE`, `COSTS`, `BUFFERS` | 9.0 | always (`BUFFERS` only with `ANALYZE` before 13) |
//! | `TIMING`, `Rows Removed by Filter`, `Heap Fetches`, `I/O Read/Write Time` | 9.2 | `TIMING OFF` when asked |
//! | `Planning Time` / `Execution Time` (before: `Total runtime`) | 9.4 | both read |
//! | `Workers Planned` / `Launched`, `Gather`, `Parallel Aware` | 9.6 | |
//! | `SUMMARY` | 10 | on an estimated plan (its planning measured) |
//! | `JIT` | 11 | |
//! | `SETTINGS` | 12 | yes |
//! | `WAL`, planning `BUFFERS`, `Incremental Sort`, `HashAgg` on disk | 13 | `WAL` for writes |
//! | `Query Identifier` (`VERBOSE`, `compute_query_id`), `Memoize` | 14 | |
//! | `Temp I/O` times | 15 | |
//! | `MEMORY`, `SERIALIZE`, `Shared/Local I/O … Time` | 17 | `MEMORY` yes; `SERIALIZE TEXT` when asked |
//! | `BUFFERS` by default with `ANALYZE`, `Index Searches`, `Disabled`, fractional rows, `Material` / `WindowAgg` / CTE storage | 18 | `BUFFERS OFF` when not asked |

use database::DbError;
use database::capabilities::ProfileCaps;
use database::profile::{
    AggStrategy, CacheStats, ChildRole, CondKind, Condition, Engine, Extra, ExtraValue, HashStats,
    Io, Jit, JoinKind, Memory, NodeId, ObjectKind, ObjectRef, OpKind, Parallel, PlanningMemory,
    Profile, ProfileDepth, ProfileNode, ProfileOptions, Quantity, ScanDirection, Serialize,
    Setting, Spill, SpillKind, Storage, TriggerTime, Unit, Wal, WorkerStats, WriteKind,
};
use serde_json::{Map, Value as Json};

/// The savepoint a profile run inside the user's transaction is undone to.
pub const SAVEPOINT: &str = "console_profile";

/// PostgreSQL's page size when the server didn't say (`SHOW block_size`).
pub const BLOCK_SIZE: u32 = 8192;

/// What a version's profiles can hold.
pub const fn caps(version: u32) -> ProfileCaps {
    ProfileCaps {
        estimated: true,
        actual: true,
        node_timing: true,
        buffers: true,
        io_timing: true,
        wal: version >= 130_000,
        memory: true,
        spill: true,
        parallel: version >= 90_600,
        settings: version >= 120_000,
        verbose_output: true,
        jit: version >= 110_000,
        triggers: true,
        engine_counters: false,
        node_actuals_by_scan: false,
        // By the server's `Query Identifier` (14+), when `pg_stat_statements` is installed.
        statement_history: version >= 140_000,
        table_stats: true,
        undo_writes: true,
    }
}

/// The `EXPLAIN (…) ` a profile of `options` sends on `version` (`writes`: the statement
/// writes — its WAL asked for).
pub fn options(version: u32, options: &ProfileOptions, writes: bool) -> String {
    let runs = options.depth.runs();
    let mut list: Vec<&str> = Vec::new();
    if runs {
        list.push("ANALYZE");
        if options.depth == (ProfileDepth::Actual { timing: false }) {
            list.push("TIMING OFF");
        }
    }
    if options.buffers && (runs || version >= 130_000) {
        list.push("BUFFERS");
    } else if runs && version >= 180_000 {
        // On by default with ANALYZE from 18: off as asked.
        list.push("BUFFERS OFF");
    }
    if options.verbose {
        list.push("VERBOSE");
    }
    if options.settings && version >= 120_000 {
        list.push("SETTINGS");
    }
    if options.wal && runs && writes && version >= 130_000 {
        list.push("WAL");
    }
    if !runs && version >= 100_000 {
        list.push("SUMMARY");
    }
    if version >= 170_000 {
        list.push("MEMORY");
    }
    if options.serialize && runs && version >= 170_000 {
        list.push("SERIALIZE TEXT");
    }
    list.push("FORMAT JSON");
    format!("EXPLAIN ({}) ", list.join(", "))
}

/// The keys of a node the model has a place for (any other one is kept as an extra).
pub const KEYS: &[&str] = &[
    "Node Type",
    "Plans",
    "Parent Relationship",
    "Subplan Name",
    "Parallel Aware",
    "Async Capable",
    "Join Type",
    "Strategy",
    "Operation",
    "Relation Name",
    "Schema",
    "Alias",
    "Index Name",
    "CTE Name",
    "Function Name",
    "Table Function Name",
    "Scan Direction",
    "Startup Cost",
    "Total Cost",
    "Plan Rows",
    "Plan Width",
    "Actual Startup Time",
    "Actual Total Time",
    "Actual Rows",
    "Actual Loops",
    "Disabled",
    "Output",
    "Index Cond",
    "Recheck Cond",
    "Hash Cond",
    "Merge Cond",
    "Join Filter",
    "Filter",
    "One-Time Filter",
    "Sort Key",
    "Group Key",
    "Presorted Key",
    "Cache Key",
    "Order By",
    "TID Cond",
    "Run Condition",
    "Rows Removed by Filter",
    "Rows Removed by Join Filter",
    "Rows Removed by Index Recheck",
    "Heap Fetches",
    "Index Searches",
    "Exact Heap Blocks",
    "Lossy Heap Blocks",
    "Shared Hit Blocks",
    "Shared Read Blocks",
    "Shared Dirtied Blocks",
    "Shared Written Blocks",
    "Local Hit Blocks",
    "Local Read Blocks",
    "Local Dirtied Blocks",
    "Local Written Blocks",
    "Temp Read Blocks",
    "Temp Written Blocks",
    "I/O Read Time",
    "I/O Write Time",
    "Shared I/O Read Time",
    "Shared I/O Write Time",
    "Local I/O Read Time",
    "Local I/O Write Time",
    "Temp I/O Read Time",
    "Temp I/O Write Time",
    "WAL Records",
    "WAL FPI",
    "WAL Bytes",
    "WAL Buffers Full",
    "Sort Method",
    "Sort Space Used",
    "Sort Space Type",
    "Peak Memory Usage",
    "Hash Buckets",
    "Original Hash Buckets",
    "Hash Batches",
    "Original Hash Batches",
    "HashAgg Batches",
    "Disk Usage",
    "Cache Hits",
    "Cache Misses",
    "Cache Evictions",
    "Cache Overflows",
    "Storage",
    "Maximum Storage",
    "Workers Planned",
    "Workers Launched",
    "Workers",
];

/// The keys of the statement the model has a place for.
pub const STATEMENT_KEYS: &[&str] = &[
    "Plan",
    "Planning Time",
    "Execution Time",
    "Total Runtime",
    "Planning",
    "Triggers",
    "JIT",
    "Settings",
    "Query Identifier",
    "Serialization",
];

/// A condition's kind, made when found.
type MakeKind = fn() -> CondKind;

/// The conditions, in the order a plan shows them.
const CONDITIONS: &[(&str, MakeKind)] = &[
    ("Index Cond", || CondKind::IndexCond),
    ("Recheck Cond", || CondKind::Recheck),
    ("Hash Cond", || CondKind::HashCond),
    ("Merge Cond", || CondKind::MergeCond),
    ("Join Filter", || CondKind::JoinFilter),
    ("Filter", || CondKind::Filter),
    ("One-Time Filter", || CondKind::OneTimeFilter),
    ("Sort Key", || CondKind::SortKey),
    ("Group Key", || CondKind::GroupKey),
    ("Presorted Key", || CondKind::Presorted),
    ("Cache Key", || CondKind::CacheKey),
    ("Order By", || CondKind::Other("Order By".to_owned())),
    ("TID Cond", || CondKind::Other("TID Cond".to_owned())),
    ("Run Condition", || CondKind::Other("Run Condition".to_owned())),
];

/// PostgreSQL's cost of a disabled step before 18 (`disable_cost`).
const DISABLE_COST: f64 = 1.0e10;

fn invalid(why: impl std::fmt::Display) -> DbError {
    DbError::Query(format!("a plan that can't be read: {why}"))
}

/// `EXPLAIN (FORMAT JSON)`'s answer (`[{"Plan": {…}, "Planning Time": …}, …]`) as a profile
/// (its blocks of `block_size` bytes). Several items (a rule's statements): several roots,
/// their times added up.
pub fn parse(json: &str, block_size: u32) -> Result<Profile, DbError> {
    let root: Json = serde_json::from_str(json).map_err(invalid)?;
    let items = match &root {
        Json::Array(items) => items.as_slice(),
        other => std::slice::from_ref(other),
    };
    let mut profile = Profile::new(Engine::default(), ProfileDepth::Estimated);
    let mut any = false;
    for item in items {
        let Some(object) = item.as_object() else { continue };
        let Some(plan) = object.get("Plan").and_then(Json::as_object) else { continue };
        any = true;
        node(&mut profile, None, plan, block_size);
        statement(&mut profile, object, block_size);
    }
    if !any {
        return Err(invalid("no `Plan` in it"));
    }
    mark_disabled(&mut profile);
    profile.sum_io();
    profile.mark_size();
    if profile.nodes.iter().any(ProfileNode::ran) {
        let timing = profile.nodes.iter().any(|n| n.actuals.total_ms.is_some());
        profile.depth = ProfileDepth::Actual { timing };
    }
    Ok(profile)
}

fn text(v: &Json) -> Option<String> {
    v.as_str().map(str::to_owned)
}

fn number(v: &Json) -> Option<f64> {
    v.as_f64()
}

fn count(v: &Json) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f.round() as u64))
}

/// A JSON value the model doesn't know, as an extra's value (objects as groups).
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
        Json::Object(map) => ExtraValue::Group(extras(map, None)),
    }
}

fn extras(map: &Map<String, Json>, group: Option<&str>) -> Vec<Extra> {
    map.iter()
        .map(|(k, v)| {
            let mut e = Extra::new(k.clone(), extra_value(v));
            e.group = group.map(str::to_owned);
            e
        })
        .collect()
}

/// The group an unknown key goes to, by its prefix.
fn group_of(key: &str) -> Option<&'static str> {
    if key.starts_with("Worker") {
        Some("Workers")
    } else if key.starts_with("JIT") {
        Some("JIT")
    } else if key.starts_with("Trigger") {
        Some("Triggers")
    } else {
        None
    }
}

fn io_key(io: &mut Io, key: &str, v: &Json) -> bool {
    let slot: &mut Option<u64> = match key {
        "Shared Hit Blocks" => &mut io.shared_hit,
        "Shared Read Blocks" => &mut io.shared_read,
        "Shared Dirtied Blocks" => &mut io.shared_dirtied,
        "Shared Written Blocks" => &mut io.shared_written,
        "Local Hit Blocks" => &mut io.local_hit,
        "Local Read Blocks" => &mut io.local_read,
        "Local Dirtied Blocks" => &mut io.local_dirtied,
        "Local Written Blocks" => &mut io.local_written,
        "Temp Read Blocks" => &mut io.temp_read,
        "Temp Written Blocks" => &mut io.temp_written,
        _ => {
            let time: &mut Option<f64> = match key {
                "I/O Read Time" | "Shared I/O Read Time" => &mut io.read_ms,
                "I/O Write Time" | "Shared I/O Write Time" => &mut io.write_ms,
                "Local I/O Read Time" => &mut io.local_read_ms,
                "Local I/O Write Time" => &mut io.local_write_ms,
                "Temp I/O Read Time" => &mut io.temp_read_ms,
                "Temp I/O Write Time" => &mut io.temp_write_ms,
                _ => return false,
            };
            *time = number(v);
            return true;
        }
    };
    *slot = count(v);
    true
}

fn op_of(name: &str, strategy: Option<&str>, operation: Option<&str>) -> OpKind {
    match name {
        "Seq Scan" | "Sample Scan" => OpKind::SeqScan,
        "Index Scan" => OpKind::IndexScan,
        "Index Only Scan" => OpKind::IndexOnlyScan,
        "Bitmap Heap Scan" => OpKind::BitmapHeapScan,
        "Bitmap Index Scan" => OpKind::BitmapIndexScan,
        "BitmapOr" => OpKind::MultiIndexOr,
        "Tid Scan" | "Tid Range Scan" => OpKind::TidScan,
        "Function Scan" | "Table Function Scan" => OpKind::FunctionScan,
        "Values Scan" => OpKind::ValuesScan,
        "CTE Scan" | "WorkTable Scan" => OpKind::CteScan,
        "Subquery Scan" => OpKind::SubqueryScan,
        "Nested Loop" => OpKind::NestedLoop,
        "Hash Join" => OpKind::HashJoin,
        "Merge Join" => OpKind::MergeJoin,
        "Hash" => OpKind::Hash,
        "Sort" => OpKind::Sort,
        "Incremental Sort" => OpKind::IncrementalSort,
        "Aggregate" => OpKind::Aggregate(match strategy {
            Some("Sorted") => AggStrategy::Sorted,
            Some("Hashed") => AggStrategy::Hashed,
            Some("Mixed") => AggStrategy::Mixed,
            _ => AggStrategy::Plain,
        }),
        "Group" => OpKind::Aggregate(AggStrategy::Sorted),
        "WindowAgg" => OpKind::WindowAgg,
        "Unique" => OpKind::Unique,
        "SetOp" => OpKind::SetOp,
        "Limit" => OpKind::Limit,
        "Materialize" => OpKind::Materialize,
        "Memoize" | "Result Cache" => OpKind::Memoize,
        "Gather" => OpKind::Gather,
        "Gather Merge" => OpKind::GatherMerge,
        "Append" => OpKind::Append,
        "Merge Append" => OpKind::MergeAppend,
        "Recursive Union" => OpKind::Compound,
        "Result" | "ProjectSet" => OpKind::Result,
        "LockRows" => OpKind::LockRows,
        "ModifyTable" => match operation {
            Some("Insert") => OpKind::ModifyTable(WriteKind::Insert),
            Some("Update") => OpKind::ModifyTable(WriteKind::Update),
            Some("Delete") => OpKind::ModifyTable(WriteKind::Delete),
            Some("Merge") => OpKind::ModifyTable(WriteKind::Merge),
            _ => OpKind::Other,
        },
        _ => OpKind::Other,
    }
}

fn role_of(words: &str) -> Option<ChildRole> {
    Some(match words {
        "Outer" => ChildRole::Outer,
        "Inner" => ChildRole::Inner,
        "InitPlan" => ChildRole::InitPlan,
        "SubPlan" => ChildRole::SubPlan,
        "Member" => ChildRole::Member,
        "Subquery" => ChildRole::Subquery,
        _ => return None,
    })
}

/// A condition's text: a string, or a list (`Sort Key`) joined.
fn condition_text(v: &Json) -> Option<String> {
    match v {
        Json::String(s) => Some(s.clone()),
        Json::Array(items) => {
            Some(items.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(", "))
        }
        _ => None,
    }
}

/// A worker's figures (`Workers: [{"Worker Number": 0, …}]`).
fn worker(map: &Map<String, Json>, block_size: u32) -> WorkerStats {
    let mut w = WorkerStats::default();
    let mut io = Io { block_size: Some(block_size), ..Io::default() };
    let mut has_io = false;
    for (key, v) in map {
        match key.as_str() {
            "Worker Number" => w.number = count(v).unwrap_or(0) as u32,
            "Actual Startup Time" => w.startup_ms = number(v),
            "Actual Total Time" => w.total_ms = number(v),
            "Actual Rows" => w.rows = number(v),
            "Actual Loops" => w.loops = number(v),
            k if io_key(&mut io, k, v) => has_io = true,
            k => w.extra.push(Extra::new(k, extra_value(v))),
        }
    }
    if has_io {
        w.io = Some(io);
    }
    w
}

fn node(profile: &mut Profile, parent: Option<NodeId>, map: &Map<String, Json>, block_size: u32) {
    let get = |key: &str| map.get(key);
    let name = get("Node Type").and_then(text).unwrap_or_default();
    let strategy = get("Strategy").and_then(Json::as_str);
    let operation = get("Operation").and_then(Json::as_str);
    let mut n = ProfileNode::new(op_of(&name, strategy, operation), name.clone());
    let mut io = Io { block_size: Some(block_size), ..Io::default() };
    let mut has_io = false;
    let mut memory = Memory::default();
    let mut hash = HashStats::default();
    let mut cache = CacheStats::default();
    let mut wal = Wal::default();
    let mut parallel = Parallel::default();
    let mut has_parallel = false;
    let mut conditions: Vec<(usize, Condition)> = Vec::new();
    let mut sort_space: Option<u64> = None;
    let mut sort_type: Option<Storage> = None;
    let mut disk_kb: Option<u64> = None;
    let mut hashagg_batches: Option<u64> = None;
    let mut max_storage_kb: Option<u64> = None;
    for (key, v) in map {
        let k = key.as_str();
        if let Some(at) = CONDITIONS.iter().position(|(c, _)| *c == k) {
            if let Some(t) = condition_text(v) {
                conditions.push((at, Condition::new((CONDITIONS[at].1)(), t)));
            }
            continue;
        }
        if io_key(&mut io, k, v) {
            has_io = true;
            continue;
        }
        match k {
            "Node Type" | "Plans" | "Operation" => {}
            "Strategy" if matches!(n.op, OpKind::Aggregate(_)) => {}
            "Parent Relationship" => n.role = v.as_str().and_then(role_of),
            "Subplan Name" => n.subplan_name = text(v),
            "Parallel Aware" => {
                parallel.aware = v.as_bool().unwrap_or(false);
                has_parallel |= parallel.aware;
            }
            "Async Capable" => n.flags.async_capable = v.as_bool().unwrap_or(false),
            "Join Type" => n.join = v.as_str().and_then(JoinKind::from_words),
            "Relation Name" => {}
            "Schema" | "Alias" => {}
            "Index Name" => {}
            "CTE Name" | "Function Name" | "Table Function Name" => {}
            "Scan Direction" => {
                n.scan_direction = match v.as_str() {
                    Some("Forward") => Some(ScanDirection::Forward),
                    Some("Backward") => Some(ScanDirection::Backward),
                    Some("NoMovement") => Some(ScanDirection::NoMovement),
                    _ => None,
                }
            }
            "Startup Cost" => n.estimates.startup_cost = number(v),
            "Total Cost" => n.estimates.total_cost = number(v),
            "Plan Rows" => n.estimates.rows = number(v),
            "Plan Width" => n.estimates.width = number(v),
            "Actual Startup Time" => n.actuals.startup_ms = number(v),
            "Actual Total Time" => n.actuals.total_ms = number(v),
            "Actual Rows" => n.actuals.rows = number(v),
            "Actual Loops" => n.actuals.loops = number(v),
            "Disabled" => n.flags.disabled = v.as_bool().unwrap_or(false),
            "Output" => {
                n.output = match v {
                    Json::Array(items) => items.iter().filter_map(text).collect(),
                    other => text(other).into_iter().collect(),
                }
            }
            "Rows Removed by Filter" => n.actuals.rows_removed_by_filter = number(v),
            "Rows Removed by Join Filter" => n.actuals.rows_removed_by_join_filter = number(v),
            "Rows Removed by Index Recheck" => n.actuals.rows_removed_by_recheck = number(v),
            "Heap Fetches" => n.actuals.heap_fetches = number(v),
            "Index Searches" => n.actuals.index_searches = number(v),
            "Exact Heap Blocks" => n.actuals.exact_blocks = number(v),
            "Lossy Heap Blocks" => n.actuals.lossy_blocks = number(v),
            "WAL Records" => wal.records = count(v),
            "WAL FPI" => wal.full_page_images = count(v),
            "WAL Bytes" => wal.bytes = count(v),
            "WAL Buffers Full" => wal.buffers_full = count(v),
            "Sort Method" => memory.sort_method = text(v),
            "Sort Space Used" => sort_space = count(v),
            "Sort Space Type" => sort_type = v.as_str().and_then(Storage::from_id),
            "Peak Memory Usage" => memory.peak_bytes = count(v).map(|kb| kb * 1024),
            "Hash Buckets" => hash.buckets = count(v),
            "Original Hash Buckets" => hash.original_buckets = count(v),
            "Hash Batches" => hash.batches = count(v),
            "Original Hash Batches" => hash.original_batches = count(v),
            "HashAgg Batches" => hashagg_batches = count(v),
            "Disk Usage" => disk_kb = count(v),
            "Cache Hits" => cache.hits = count(v),
            "Cache Misses" => cache.misses = count(v),
            "Cache Evictions" => cache.evictions = count(v),
            "Cache Overflows" => cache.overflows = count(v),
            "Storage" => memory.storage = v.as_str().and_then(Storage::from_id),
            "Maximum Storage" => max_storage_kb = count(v),
            "Workers Planned" => {
                parallel.planned = count(v).map(|c| c as u32);
                has_parallel = true;
            }
            "Workers Launched" => {
                parallel.launched = count(v).map(|c| c as u32);
                has_parallel = true;
            }
            "Workers" => {
                if let Json::Array(items) = v {
                    parallel.per_worker = items
                        .iter()
                        .filter_map(Json::as_object)
                        .map(|w| worker(w, block_size))
                        .collect();
                    has_parallel = true;
                }
            }
            other => {
                let mut e = Extra::new(other, extra_value(v));
                e.group = group_of(other).map(str::to_owned);
                n.extra.push(e);
            }
        }
    }
    // A partial aggregate (its finalizing above it); the mode is kept as an extra too (as a
    // non-aggregate's `Strategy`, SetOp's `Hashed`).
    if let Some("Partial") = get("Partial Mode").and_then(Json::as_str) {
        n.flags.partial = true;
    }
    conditions.sort_by_key(|(at, _)| *at);
    n.conditions = conditions.into_iter().map(|(_, c)| c).collect();
    // What it works on.
    let relation = get("Relation Name").and_then(text);
    let alias = get("Alias").and_then(text);
    let schema = get("Schema").and_then(text);
    let index = get("Index Name").and_then(text);
    if let Some(relation) = relation {
        n.object = Some(ObjectRef { kind: ObjectKind::Table, schema, name: relation, alias });
        n.index = index;
    } else if let Some(index) = index {
        n.object = Some(ObjectRef::new(ObjectKind::Index, index));
    } else if let Some(cte) = get("CTE Name").and_then(text) {
        n.object = Some(ObjectRef { alias, ..ObjectRef::new(ObjectKind::Cte, cte) });
    } else if let Some(function) =
        get("Function Name").and_then(text).or_else(|| get("Table Function Name").and_then(text))
    {
        n.object =
            Some(ObjectRef { schema, alias, ..ObjectRef::new(ObjectKind::Function, function) });
    } else if let Some(alias) = alias {
        let kind =
            if n.op == OpKind::SubqueryScan { ObjectKind::Subquery } else { ObjectKind::Other };
        n.object = Some(ObjectRef::new(kind, alias));
    }
    if n.actuals.loops == Some(0.0) {
        n.actuals.never_executed = true;
    }
    if has_io {
        n.io = Some(io);
    }
    if hash != HashStats::default() {
        memory.hash = Some(hash);
    }
    if cache != CacheStats::default() {
        memory.cache = Some(cache);
    }
    if let Some(space) = sort_space {
        memory.sort_space = Some((space * 1024, sort_type.unwrap_or(Storage::Memory)));
    }
    if max_storage_kb.is_some() && memory.peak_bytes.is_none() {
        memory.peak_bytes = max_storage_kb.map(|kb| kb * 1024);
    }
    // What spilled to disk.
    n.spill = if sort_type == Some(Storage::Disk)
        || memory.sort_method.as_deref().is_some_and(|m| m.starts_with("external"))
    {
        Some(Spill {
            kind: SpillKind::Sort,
            disk_bytes: sort_space.map(|kb| kb * 1024),
            batches: None,
        })
    } else if hashagg_batches.is_some_and(|b| b > 1) || disk_kb.is_some_and(|d| d > 0) {
        Some(Spill {
            kind: SpillKind::HashAgg,
            disk_bytes: disk_kb.map(|kb| kb * 1024),
            batches: hashagg_batches,
        })
    } else if hash.batches.is_some_and(|b| b > 1) {
        Some(Spill { kind: SpillKind::Hash, disk_bytes: None, batches: hash.batches })
    } else if memory.storage == Some(Storage::Disk) {
        let kind = match n.op {
            OpKind::WindowAgg => SpillKind::Window,
            OpKind::CteScan | OpKind::Compound => SpillKind::Cte,
            _ => SpillKind::Material,
        };
        Some(Spill { kind, disk_bytes: max_storage_kb.map(|kb| kb * 1024), batches: None })
    } else {
        None
    };
    if hashagg_batches.is_some() && !matches!(n.spill, Some(Spill { kind: SpillKind::HashAgg, .. }))
    {
        n.extra.push(Extra::new(
            "HashAgg Batches",
            ExtraValue::Int(hashagg_batches.unwrap_or(0) as i64),
        ));
    }
    if disk_kb.is_some() && n.spill.is_none() {
        n.extra.push(Extra::new(
            "Disk Usage",
            ExtraValue::Quantity(Quantity::new(disk_kb.unwrap_or(0) as f64 * 1024.0, Unit::Bytes)),
        ));
    }
    if memory != Memory::default() {
        n.memory = Some(memory);
    }
    if wal != Wal::default() {
        n.wal = Some(wal);
    }
    if has_parallel {
        n.parallel = Some(parallel);
    }
    let id = profile.push(parent, n);
    if let Some(Json::Array(children)) = get("Plans") {
        for child in children.iter().filter_map(Json::as_object) {
            node(profile, Some(id), child, block_size);
        }
    }
}

/// Before 18, a disabled step shows only as its cost jumping past `disable_cost`.
fn mark_disabled(profile: &mut Profile) {
    for i in 0..profile.nodes.len() {
        let n = &profile.nodes[i];
        if n.flags.disabled {
            continue;
        }
        let own = n.estimates.total_cost.unwrap_or(0.0);
        let children_below = n
            .children
            .iter()
            .all(|c| profile.nodes[c.index()].estimates.total_cost.unwrap_or(0.0) < DISABLE_COST);
        if own >= DISABLE_COST && children_below {
            profile.nodes[i].flags.disabled = true;
        }
    }
}

fn add(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

fn statement(profile: &mut Profile, map: &Map<String, Json>, block_size: u32) {
    let s = &mut profile.statement;
    for (key, v) in map {
        match key.as_str() {
            "Plan" => {}
            "Planning Time" => s.planning_ms = add(s.planning_ms, number(v)),
            "Execution Time" | "Total Runtime" => s.execution_ms = add(s.execution_ms, number(v)),
            "Planning" => {
                let Some(planning) = v.as_object() else { continue };
                let mut io = Io { block_size: Some(block_size), ..Io::default() };
                let mut has_io = false;
                let mut memory = PlanningMemory::default();
                for (k, x) in planning {
                    match k.as_str() {
                        k if io_key(&mut io, k, x) => has_io = true,
                        "Memory Used" => memory.used_bytes = count(x).map(|kb| kb * 1024),
                        "Memory Allocated" => memory.allocated_bytes = count(x).map(|kb| kb * 1024),
                        other => {
                            s.extra.push(Extra::new(other, extra_value(x)).in_group("Planning"))
                        }
                    }
                }
                if has_io {
                    s.planning_io = Some(io);
                }
                if memory != PlanningMemory::default() {
                    s.planning_memory = Some(memory);
                }
            }
            "Triggers" => {
                let Json::Array(items) = v else { continue };
                for t in items.iter().filter_map(Json::as_object) {
                    let mut time = TriggerTime::default();
                    for (k, x) in t {
                        match k.as_str() {
                            "Trigger Name" => time.name = text(x).unwrap_or_default(),
                            "Relation" => time.relation = text(x),
                            "Time" => time.ms = number(x),
                            "Calls" => time.calls = count(x),
                            other => {
                                s.extra.push(Extra::new(other, extra_value(x)).in_group("Triggers"))
                            }
                        }
                    }
                    s.triggers.push(time);
                }
            }
            "JIT" => {
                let Some(jit) = v.as_object() else { continue };
                let mut j = Jit::default();
                let inlining = jit
                    .get("Options")
                    .and_then(|o| o.get("Inlining"))
                    .and_then(Json::as_bool)
                    .unwrap_or(true);
                for (k, x) in jit {
                    match k.as_str() {
                        "Functions" => j.functions = count(x),
                        "Timing" => {
                            let Some(timing) = x.as_object() else { continue };
                            for (t, y) in timing {
                                match t.as_str() {
                                    "Generation" => match y {
                                        Json::Object(g) => {
                                            j.generation_ms = g.get("Total").and_then(number);
                                            j.deform_ms = g.get("Deform").and_then(number);
                                        }
                                        other => j.generation_ms = number(other),
                                    },
                                    "Inlining" => j.inlining_ms = number(y).filter(|_| inlining),
                                    "Optimization" => j.optimization_ms = number(y),
                                    "Emission" => j.emission_ms = number(y),
                                    "Total" => j.total_ms = number(y),
                                    other => s
                                        .extra
                                        .push(Extra::new(other, extra_value(y)).in_group("JIT")),
                                }
                            }
                        }
                        other => s.extra.push(Extra::new(other, extra_value(x)).in_group("JIT")),
                    }
                }
                s.jit = Some(j);
            }
            "Settings" => {
                if let Some(settings) = v.as_object() {
                    for (name, value) in settings {
                        let value = match value {
                            Json::String(t) => t.clone(),
                            other => other.to_string(),
                        };
                        s.settings.push(Setting { name: name.clone(), value });
                    }
                }
            }
            "Query Identifier" => s.query_id = v.as_i64(),
            "Serialization" => {
                let Some(ser) = v.as_object() else { continue };
                let mut x = Serialize::default();
                for (k, y) in ser {
                    match k.as_str() {
                        "Time" => x.ms = number(y),
                        "Output Volume" => x.bytes = count(y).map(|kb| kb * 1024),
                        "Format" => x.format = text(y),
                        other => s
                            .extra
                            .push(Extra::new(other, extra_value(y)).in_group("Serialization")),
                    }
                }
                s.serialize = Some(x);
            }
            other => {
                let mut e = Extra::new(other, extra_value(v));
                e.group = group_of(other).map(str::to_owned);
                s.extra.push(e);
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/profile.rs"]
mod tests;
