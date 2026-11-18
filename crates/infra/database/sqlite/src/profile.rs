//! SQLite's profiles (SQLite has no `EXPLAIN ANALYZE`: the run is measured here):
//!
//! - the plan from `EXPLAIN QUERY PLAN`, each row's words normalized ([`sqlite_op`]: `SCAN`,
//!   `SEARCH … USING [COVERING] INDEX`, an automatic index, a temporary B-tree, subqueries,
//!   co-routines, compounds, Bloom filters), its tree by the rows' ids;
//! - run on the console's connection ([`measure`]): prepared (its time: the planning), stepped
//!   to its end with the rows counted and dropped, the first row's time; the statement's
//!   counters from `sqlite3_stmt_status` ([`EngineCounters`]); each plan element's real loops
//!   and rows (and its share of the CPU's cycles, as its time) from
//!   `sqlite3_stmt_scanstatus_v2` — SQLite is built with `SQLITE_ENABLE_STMT_SCANSTATUS`
//!   (`.cargo/config.toml`), collected only while a profile runs
//!   (`SQLITE_DBCONFIG_STMT_SCANSTATUS`, off otherwise: the rest of the app pays nothing);
//! - a write runs between `SAVEPOINT console_profile` and `ROLLBACK TO` + `RELEASE` (inside
//!   the user's transaction, or as one of its own): the table is left as it was;
//! - its time limit is a deadline in the progress handler (an interrupt), beside the cancel;
//! - [`table_stats`]: `sqlite_stat1`'s rows (and whether `ANALYZE` ran), the indexes from
//!   `pragma_index_list` / `pragma_index_info`, the file's pages, each table's and index's
//!   bytes from `dbstat` when it's compiled in.

use std::time::{Duration, Instant};

use database::capabilities::ProfileCaps;
use database::profile::{
    ChildRole, CondKind, Condition, Engine, EngineCounters, Extra, ExtraValue, IndexStats, NodeId,
    ObjectKind, ObjectRef, OpKind, Profile, ProfileDepth, ProfileNode, ProfileNote, Quantity,
    RawFormat, TableStats, Unit,
};
use database::sql::Flavor;
use database::{Cancel, DbError, SqlError, TableRef};
use rusqlite::config::DbConfig;
use rusqlite::{Connection, OptionalExtension, StatementStatus, params};

use crate::catalog;
use crate::error;

/// The savepoint a profiled write is undone to.
pub const SAVEPOINT: &str = "console_profile";

/// What its profiles hold: the plan, the run measured with the engine's counters and each
/// element's real rows, a write undone, the tables' statistics. No per-node timing of its
/// own (a node's time is its share of the statement's cycles).
pub const CAPS: ProfileCaps = ProfileCaps {
    estimated: true,
    actual: true,
    engine_counters: true,
    node_actuals_by_scan: true,
    table_stats: true,
    undo_writes: true,
    ..ProfileCaps::NONE
};

/// How many steps of SQLite's machine run between two looks at the cancel and the deadline.
const CHECK: i32 = 1000;

/// What a plan row's words say.
#[derive(Debug, Clone, PartialEq)]
pub struct Normalized {
    pub op: OpKind,
    pub object: Option<ObjectRef>,
    pub index: Option<String>,
    pub conditions: Vec<Condition>,
    pub role: Option<ChildRole>,
}

/// The part in parentheses at the end (`(a=? AND b>?)`), without them.
fn last_parens(text: &str) -> Option<&str> {
    let text = text.trim_end();
    let inner = text.strip_suffix(')')?;
    let open = inner.rfind('(')?;
    Some(&inner[open + 1..])
}

/// A plan row's words (`SEARCH users USING INDEX users_name (name=?)`) normalized.
pub fn sqlite_op(detail: &str) -> Normalized {
    let detail = detail.trim();
    let words: Vec<&str> = detail.split_whitespace().collect();
    let mut n = Normalized {
        op: OpKind::Other,
        object: None,
        index: None,
        conditions: Vec::new(),
        role: None,
    };
    let at = |i: usize| words.get(i).copied().unwrap_or("");
    let upper = detail.to_ascii_uppercase();
    match at(0) {
        "SCAN" | "SEARCH" => {
            let search = at(0) == "SEARCH";
            if at(1) == "CONSTANT" && at(2) == "ROW" {
                n.op = OpKind::Result;
                return n;
            }
            // `SCAN TABLE t` before 3.36.
            let mut i = 1;
            if at(i) == "TABLE" && words.len() > 2 {
                i += 1;
            }
            let name = at(i).to_owned();
            i += 1;
            let mut alias = None;
            if at(i) == "AS" {
                alias = Some(at(i + 1).to_owned());
                i += 2;
            }
            let kind = if name.starts_with('(') { ObjectKind::Subquery } else { ObjectKind::Table };
            n.object = Some(ObjectRef { kind, schema: None, name, alias });
            let rest = words[i.min(words.len())..].join(" ");
            let cond = last_parens(&rest).map(str::to_owned);
            n.op = if rest.contains("AUTOMATIC") {
                OpKind::AutoIndex
            } else if rest.contains("INTEGER PRIMARY KEY") {
                OpKind::RowidLookup
            } else if rest.contains("COVERING INDEX") {
                OpKind::IndexOnlyScan
            } else if rest.contains("USING INDEX") || rest.contains("USING PRIMARY KEY") {
                OpKind::IndexScan
            } else if rest.contains("VIRTUAL TABLE") {
                OpKind::FunctionScan
            } else {
                OpKind::SeqScan
            };
            if matches!(n.op, OpKind::IndexScan | OpKind::IndexOnlyScan) {
                let after = rest.split("INDEX ").nth(1).unwrap_or("");
                n.index = after
                    .split_whitespace()
                    .next()
                    .filter(|w| !w.starts_with('('))
                    .map(str::to_owned);
            }
            if let Some(cond) = cond.filter(|_| search || n.op == OpKind::AutoIndex) {
                n.conditions.push(Condition::new(CondKind::IndexCond, cond));
            }
        }
        "USE" if upper.starts_with("USE TEMP B-TREE FOR ") => {
            n.op = OpKind::TempBTree;
            let purpose = detail["USE TEMP B-TREE FOR ".len()..].to_owned();
            let kind =
                if purpose.contains("GROUP BY") { CondKind::GroupKey } else { CondKind::SortKey };
            n.conditions.push(Condition::new(kind, purpose));
        }
        "CORRELATED" | "SCALAR" | "LIST" if upper.contains("SUBQUERY") => {
            n.role = Some(ChildRole::SubPlan);
            n.op = OpKind::Other;
        }
        "CO-ROUTINE" => {
            n.op = OpKind::Coroutine;
            n.role = Some(ChildRole::Cte);
            n.object = Some(ObjectRef::new(ObjectKind::Cte, at(1)));
        }
        "MATERIALIZE" => {
            n.op = OpKind::Materialize;
            n.role = Some(ChildRole::Cte);
            n.object = Some(ObjectRef::new(ObjectKind::Cte, at(1)));
        }
        "MULTI-INDEX" => n.op = OpKind::MultiIndexOr,
        "INDEX" if words.len() == 2 => {
            n.role = Some(ChildRole::Member);
        }
        "COMPOUND" => n.op = OpKind::Compound,
        // Building an automatic index (`scanstatus` lists it, the plan doesn't).
        "CREATE" if upper.starts_with("CREATE AUTOMATIC INDEX ON ") => {
            let table =
                detail["CREATE AUTOMATIC INDEX ON ".len()..].split('(').next().unwrap_or("");
            n.object = Some(ObjectRef::new(ObjectKind::Table, table.trim()));
        }
        "LEFT-MOST" | "UNION" | "EXCEPT" | "INTERSECT" => {
            n.op = OpKind::Compound;
            n.role = Some(ChildRole::Member);
        }
        "BLOOM" => {
            n.op = OpKind::BloomFilter;
            // `BLOOM FILTER ON t (a=?)`.
            n.object = Some(ObjectRef::new(ObjectKind::Table, at(3)));
            if let Some(cond) = last_parens(detail) {
                n.conditions.push(Condition::new(CondKind::Filter, cond));
            }
        }
        _ => {}
    }
    n
}

/// `EXPLAIN QUERY PLAN`'s rows (`id`, `parent`, `detail`, in their order) as the profile's
/// tree, and each node's row id (what `scanstatus` names a plan element by). A row whose
/// parent isn't there goes to the top; a cycle's rows at the top too: none lost.
pub fn from_rows(rows: &[(i64, i64, String)], depth: ProfileDepth) -> (Profile, Vec<i64>) {
    let mut profile = Profile::new(Engine::default(), depth);
    let mut ids = Vec::with_capacity(rows.len());
    let mut placed = vec![false; rows.len()];
    let known: std::collections::HashSet<i64> = rows.iter().map(|r| r.0).collect();
    fn add(
        rows: &[(i64, i64, String)],
        at: usize,
        parent: Option<NodeId>,
        placed: &mut [bool],
        profile: &mut Profile,
        ids: &mut Vec<i64>,
    ) {
        placed[at] = true;
        let (id, _, detail) = &rows[at];
        let n = sqlite_op(detail);
        let mut node = ProfileNode::new(n.op, detail.trim());
        node.object = n.object;
        node.index = n.index;
        node.conditions = n.conditions;
        node.role = n.role;
        let me = profile.push(parent, node);
        ids.push(*id);
        for child in 0..rows.len() {
            if !placed[child] && rows[child].1 == *id && rows[child].0 != *id {
                add(rows, child, Some(me), placed, profile, ids);
            }
        }
    }
    for top in 0..rows.len() {
        let (id, parent, _) = &rows[top];
        if !placed[top] && (*parent == 0 || !known.contains(parent) || parent == id) {
            add(rows, top, None, &mut placed, &mut profile, &mut ids);
        }
    }
    for i in 0..rows.len() {
        if !placed[i] {
            add(rows, i, None, &mut placed, &mut profile, &mut ids);
        }
    }
    profile.mark_size();
    (profile, ids)
}

/// `EXPLAIN QUERY PLAN` of `sql`'s rows.
pub fn query_plan(conn: &Connection, sql: &str) -> Result<Vec<(i64, i64, String)>, DbError> {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).map_err(error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(3)?)))
        .map_err(error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(error)
}

/// The plan's rows as text (the raw answer kept for an export): `id parent detail`, one a
/// line.
pub fn rows_text(rows: &[(i64, i64, String)]) -> String {
    rows.iter().map(|(id, parent, detail)| format!("{id}\t{parent}\t{detail}\n")).collect()
}

/// The engine's own name and version (`SQLite`, `3.53.2`).
pub fn engine() -> Engine {
    Engine {
        driver: "sqlite".to_owned(),
        name: "SQLite".to_owned(),
        version: Some(rusqlite::version().to_owned()),
        version_num: Some(rusqlite::version_number() as u32),
    }
}

/// What a scanned plan element said.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scan {
    /// Its id among the statement's elements (not quite `EXPLAIN QUERY PLAN`'s: that one is
    /// prepared apart, its program's addresses shifted), and its parent's.
    pub select_id: i64,
    pub parent_id: i64,
    /// Its words, as `EXPLAIN QUERY PLAN` writes them (what it's matched by).
    pub explain: Option<String>,
    pub loops: Option<i64>,
    pub visited: Option<i64>,
    pub estimate: Option<f64>,
    pub cycles: Option<i64>,
}

/// Each plan element's figures, and the whole statement's cycles. Empty when SQLite gives
/// none (built without `SQLITE_ENABLE_STMT_SCANSTATUS`, or the statement isn't the
/// connection's).
#[allow(unsafe_code)]
fn scanstatus(conn: &Connection, sql: &str) -> (Vec<Scan>, Option<i64>) {
    use rusqlite::ffi;
    use std::ffi::{CStr, c_int, c_void};
    let mut out = Vec::new();
    // SAFETY: the connection's handle is valid while `conn` lives, and so is each statement
    // `sqlite3_next_stmt` lists (none is finalized while we look: this thread holds the
    // connection, the caller's statement is still alive). `sqlite3_sql` gives a string
    // owned by the statement, read before anything else runs. `scanstatus_v2` writes one
    // value of the type its op says into the place given (an `i64`, a `double`, an `int`),
    // each a local of that type.
    unsafe {
        let db = conn.handle();
        let mut stmt = ffi::sqlite3_next_stmt(db, std::ptr::null_mut());
        let mut found = std::ptr::null_mut();
        while !stmt.is_null() {
            let text = ffi::sqlite3_sql(stmt);
            if !text.is_null() && CStr::from_ptr(text).to_bytes() == sql.as_bytes() {
                found = stmt;
                break;
            }
            stmt = ffi::sqlite3_next_stmt(db, stmt);
        }
        if found.is_null() {
            return (out, None);
        }
        let flags = ffi::SQLITE_SCANSTAT_COMPLEX as c_int;
        let int64 = |idx: c_int, op: c_int| -> Option<i64> {
            let mut v: i64 = 0;
            let rc = ffi::sqlite3_stmt_scanstatus_v2(
                found,
                idx,
                op,
                flags,
                (&raw mut v).cast::<c_void>(),
            );
            (rc == 0).then_some(v)
        };
        let total = int64(-1, ffi::SQLITE_SCANSTAT_NCYCLE as c_int);
        for idx in 0.. {
            let mut select_id: c_int = 0;
            let rc = ffi::sqlite3_stmt_scanstatus_v2(
                found,
                idx,
                ffi::SQLITE_SCANSTAT_SELECTID as c_int,
                flags,
                (&raw mut select_id).cast::<c_void>(),
            );
            if rc != 0 {
                break;
            }
            let mut parent_id: c_int = 0;
            let _ = ffi::sqlite3_stmt_scanstatus_v2(
                found,
                idx,
                ffi::SQLITE_SCANSTAT_PARENTID as c_int,
                flags,
                (&raw mut parent_id).cast::<c_void>(),
            );
            let mut words: *const std::ffi::c_char = std::ptr::null();
            let explained = ffi::sqlite3_stmt_scanstatus_v2(
                found,
                idx,
                ffi::SQLITE_SCANSTAT_EXPLAIN as c_int,
                flags,
                (&raw mut words).cast::<c_void>(),
            );
            let explain = (explained == 0 && !words.is_null())
                .then(|| CStr::from_ptr(words).to_string_lossy().into_owned());
            let mut estimate: f64 = 0.0;
            let est = ffi::sqlite3_stmt_scanstatus_v2(
                found,
                idx,
                ffi::SQLITE_SCANSTAT_EST as c_int,
                flags,
                (&raw mut estimate).cast::<c_void>(),
            );
            out.push(Scan {
                select_id: i64::from(select_id),
                parent_id: i64::from(parent_id),
                explain,
                loops: int64(idx, ffi::SQLITE_SCANSTAT_NLOOP as c_int).filter(|n| *n >= 0),
                visited: int64(idx, ffi::SQLITE_SCANSTAT_NVISIT as c_int).filter(|n| *n >= 0),
                estimate: (est == 0 && estimate >= 0.0).then_some(estimate),
                cycles: int64(idx, ffi::SQLITE_SCANSTAT_NCYCLE as c_int).filter(|n| *n >= 0),
            });
            if idx > 100_000 {
                break;
            }
        }
        (out, total)
    }
}

/// The interrupt's reason: the deadline passed (the profile's time limit), not a cancel.
fn timed_out(limit: Duration) -> DbError {
    DbError::sql(SqlError::new(format!(
        "interrupted: the profile's time limit ({} s) was reached",
        limit.as_secs_f64()
    )))
}

/// Profiles `sql` (one statement) on `conn` by running it (see the module's doc): the plan's
/// tree with each element's real loops and rows, the statement's times and counters, a
/// write undone. `limit`: the deadline; `cancel` stops it.
pub fn measure(
    conn: &mut Connection,
    sql: &str,
    limit: Option<Duration>,
    timing: bool,
    max_raw_bytes: usize,
    cancel: &Cancel,
) -> Result<Profile, DbError> {
    let sql = sql.trim().trim_end_matches(';').trim_end();
    if let Some(words) = database::profile::actual_refused(sql, Flavor::Sqlite) {
        return Err(DbError::Unsupported(format!(
            "profiling {words} as it runs: it can't run inside a transaction"
        )));
    }
    let rows = query_plan(conn, sql)?;
    let (mut profile, _) = from_rows(&rows, ProfileDepth::Actual { timing });
    profile.engine = engine();
    profile.keep_raw(RawFormat::Rows, &rows_text(&rows), max_raw_bytes);
    let writes = !conn.prepare(sql).map_err(error)?.readonly();
    if writes {
        conn.execute_batch(&format!("SAVEPOINT {SAVEPOINT}")).map_err(error)?;
    }
    let started = Instant::now();
    let deadline = limit.map(|l| started + l);
    let watched = cancel.clone();
    let late = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let told = std::sync::Arc::clone(&late);
    let _ = conn.progress_handler(
        CHECK,
        Some(move || {
            if deadline.is_some_and(|d| Instant::now() >= d) {
                told.store(true, std::sync::atomic::Ordering::Relaxed);
                return true;
            }
            watched.is_cancelled()
        }),
    );
    let scans_on = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_STMT_SCANSTATUS, true).is_ok();
    let result = run_measured(conn, sql, &mut profile, scans_on, cancel);
    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_STMT_SCANSTATUS, false);
    let _ = conn.progress_handler(0, None::<fn() -> bool>);
    if writes {
        let _ = conn.execute_batch(&format!("ROLLBACK TO {SAVEPOINT}; RELEASE {SAVEPOINT}"));
        profile.note(ProfileNote::RolledBack);
    }
    match result {
        Err(DbError::Cancelled) if late.load(std::sync::atomic::Ordering::Relaxed) => {
            return Err(timed_out(limit.unwrap_or_default()));
        }
        Err(e) => return Err(e),
        Ok(()) => {}
    }
    profile.collected = ProfileCaps {
        estimated: true,
        actual: true,
        engine_counters: profile.statement.counters.is_some(),
        node_actuals_by_scan: profile.nodes.iter().any(|n| n.actuals.loops.is_some()),
        undo_writes: writes,
        ..ProfileCaps::NONE
    };
    Ok(profile)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn run_measured(
    conn: &Connection,
    sql: &str,
    profile: &mut Profile,
    scans_on: bool,
    cancel: &Cancel,
) -> Result<(), DbError> {
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    let started = Instant::now();
    let before = conn.total_changes();
    let mut stmt = conn.prepare(sql).map_err(error)?;
    let prepared = started.elapsed();
    let write = !stmt.readonly();
    let mut returned = 0u64;
    let mut first = None;
    {
        let mut rows = stmt.raw_query();
        while rows.next().map_err(error)?.is_some() {
            returned += 1;
            if first.is_none() {
                first = Some(started.elapsed());
            }
        }
    }
    let total = started.elapsed();
    let s = &mut profile.statement;
    s.planning_ms = Some(ms(prepared));
    s.execution_ms = Some(ms(total.saturating_sub(prepared)));
    s.first_row_ms = first.map(ms);
    s.rows_returned = (!write || returned > 0).then_some(returned);
    if write {
        let changed = conn.total_changes() != before;
        s.rows_affected = Some(if changed { conn.changes() } else { 0 });
    }
    let status = |what: StatementStatus| Some(stmt.get_status(what).max(0) as u64);
    s.counters = Some(EngineCounters {
        fullscan_steps: status(StatementStatus::FullscanStep),
        sorts: status(StatementStatus::Sort),
        autoindex: status(StatementStatus::AutoIndex),
        vm_steps: status(StatementStatus::VmStep),
        reprepare: status(StatementStatus::RePrepare),
        runs: status(StatementStatus::Run),
        filter_hits: status(StatementStatus::FilterHit),
        filter_misses: status(StatementStatus::FilterMiss),
        mem_used: status(StatementStatus::MemUsed),
        ..EngineCounters::default()
    });
    if scans_on {
        let (scans, cycles) = scanstatus(conn, sql);
        apply_scans(profile, &scans, cycles);
    }
    drop(stmt);
    Ok(())
}

/// The scanned elements' figures on their nodes, matched by their words in order (their ids
/// aren't the plan's): loops, rows per loop, the estimate, the cycles — and, as its time, its
/// share of the statement's cycles of the execution's time, its own and its children's
/// (an approximation: SQLite counts each element's own cycles, not milliseconds). An element
/// the plan didn't list (building an automatic index) is added under its parent's node.
pub fn apply_scans(profile: &mut Profile, scans: &[Scan], cycles: Option<i64>) {
    let execution = profile.statement.execution_ms;
    let order: Vec<NodeId> = profile.walk().into_iter().map(|(id, _)| id).collect();
    let mut taken = vec![false; profile.nodes.len()];
    let mut own: Vec<Option<f64>> = vec![None; profile.nodes.len()];
    // Each element's node, by its id among the elements.
    let mut placed: Vec<(i64, NodeId)> = Vec::new();
    for scan in scans {
        let words = scan.explain.as_deref().unwrap_or_default().trim();
        let matched =
            order.iter().copied().find(|n| !taken[n.index()] && profile.node(*n).name == words);
        let at = match matched {
            Some(at) => at,
            None if !words.is_empty() => {
                let parent = placed.iter().find(|(id, _)| *id == scan.parent_id).map(|(_, n)| *n);
                let n = sqlite_op(words);
                let mut node = ProfileNode::new(n.op, words);
                node.object = n.object;
                node.index = n.index;
                node.conditions = n.conditions;
                node.role = n.role;
                let id = profile.push(parent, node);
                taken.push(false);
                own.push(None);
                id
            }
            None => continue,
        };
        taken[at.index()] = true;
        placed.push((scan.select_id, at));
        let node = &mut profile.nodes[at.index()];
        if let Some(loops) = scan.loops {
            node.actuals.loops = Some(loops as f64);
            if let Some(visited) = scan.visited {
                node.actuals.rows =
                    Some(if loops > 0 { visited as f64 / loops as f64 } else { 0.0 });
            }
            node.actuals.never_executed = loops == 0;
        }
        if let Some(estimate) = scan.estimate {
            node.estimates.rows = Some(estimate);
        }
        if let Some(c) = scan.cycles {
            node.extra.push(Extra::new(
                "cycles",
                ExtraValue::Quantity(Quantity::new(c as f64, Unit::Count)),
            ));
            if let (Some(total), Some(execution)) = (cycles.filter(|t| *t > 0), execution) {
                own[at.index()] = Some(execution * (c as f64 / total as f64).min(1.0));
            }
        }
    }
    // Each node's time in all its loops: its own and its children's, then per loop.
    own.resize(profile.nodes.len(), None);
    let order: Vec<NodeId> = profile.walk().into_iter().map(|(id, _)| id).collect();
    let mut inclusive: Vec<Option<f64>> = own.clone();
    for &id in order.iter().rev() {
        let children: Vec<NodeId> = profile.node(id).children.clone();
        let below: Option<f64> = children
            .iter()
            .filter_map(|c| inclusive[c.index()])
            .fold(None, |sum, t| Some(sum.unwrap_or(0.0) + t));
        inclusive[id.index()] = match (inclusive[id.index()], below) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
    }
    for (i, t) in inclusive.into_iter().enumerate() {
        if let Some(t) = t {
            let node = &mut profile.nodes[i];
            let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
            node.actuals.total_ms = Some(t / loops);
        }
    }
}

/// Whether SQLite was built with `option` (`pragma_compile_options`).
pub fn compiled_with(conn: &Connection, option: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM pragma_compile_options WHERE compile_options = ?1",
        params![option],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

/// What SQLite knows of `tables` (see the module's doc).
pub fn table_stats(
    conn: &Connection,
    tables: &[TableRef],
    cancel: &Cancel,
) -> Result<Vec<TableStats>, DbError> {
    let dbstat = compiled_with(conn, "ENABLE_DBSTAT_VTAB");
    let mut out = Vec::new();
    for table in tables {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let schema = catalog::schema_of(table);
        let quoted = format!("\"{}\"", schema.replace('"', "\"\""));
        let mut stats = TableStats::new(table.clone());
        let estimates = catalog::estimates(conn, schema)?;
        let present = estimates.get(&table.name.to_ascii_lowercase()).copied();
        stats.stats_present = Some(present.is_some());
        stats.live_rows = present;
        let page_size: Option<i64> = conn
            .query_row(&format!("PRAGMA {quoted}.page_size"), [], |r| r.get(0))
            .optional()
            .map_err(error)?;
        let page_count: Option<i64> = conn
            .query_row(&format!("PRAGMA {quoted}.page_count"), [], |r| r.get(0))
            .optional()
            .map_err(error)?;
        if let (Some(size), Some(count)) = (page_size, page_count) {
            stats.extra.push(Extra::new(
                "page_size",
                ExtraValue::Quantity(Quantity::new(size as f64, Unit::Bytes)),
            ));
            stats.extra.push(Extra::new(
                "file_bytes",
                ExtraValue::Quantity(Quantity::new((size * count) as f64, Unit::Bytes)),
            ));
        }
        let bytes_of = |name: &str| -> Option<u64> {
            if !dbstat {
                return None;
            }
            conn.query_row(
                "SELECT sum(pgsize) FROM dbstat(?1) WHERE name = ?2",
                params![schema, name],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
            .map(|b| b.max(0) as u64)
        };
        stats.table_bytes = bytes_of(&table.name);
        let mut list = conn
            .prepare("SELECT name, \"unique\" FROM pragma_index_list(?1, ?2) ORDER BY seq")
            .map_err(error)?;
        let indexes: Vec<(String, bool)> = list
            .query_map(params![table.name, schema], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            })
            .map_err(error)?
            .collect::<Result<_, _>>()
            .map_err(error)?;
        let mut index_bytes = 0u64;
        let mut all_known = dbstat;
        for (name, unique) in indexes {
            let mut info = conn
                .prepare("SELECT name FROM pragma_index_info(?1, ?2) ORDER BY seqno")
                .map_err(error)?;
            let columns: Vec<String> = info
                .query_map(params![name, schema], |r| r.get::<_, Option<String>>(0))
                .map_err(error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(error)?
                .into_iter()
                .map(|c| c.unwrap_or_else(|| "<expression>".to_owned()))
                .collect();
            let bytes = bytes_of(&name);
            match bytes {
                Some(b) => index_bytes += b,
                None => all_known = false,
            }
            stats.indexes.push(IndexStats { name, columns, unique, bytes, scans: None });
        }
        if all_known {
            stats.index_bytes = Some(index_bytes);
            stats.total_bytes = stats.table_bytes.map(|t| t + index_bytes);
        }
        out.push(stats);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../tests/unit/profile.rs"]
mod tests;
