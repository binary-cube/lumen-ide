//! What PostgreSQL knows of a profile's tables and of the query itself, pure (tested
//! without a server): the SQL each version takes and its rows read into the domain's types.
//!
//! - **The tables' statistics** ([`tables_sql`], [`table_row`]): one query for all of them,
//!   the tables given as `VALUES ($1, $2), …` (a schema `''`: the one `search_path` shows,
//!   `pg_table_is_visible`), `pg_class` for the planner's `reltuples` / `relpages`,
//!   `pg_stat_user_tables` for the live and dead rows, the scans, the last `ANALYZE` /
//!   `VACUUM` (and their automatic forms), the rows changed since (`n_mod_since_analyze`,
//!   9.4+), `pg_relation_size` / `pg_indexes_size` / `pg_total_relation_size`; the counters,
//!   `n_ins_since_vacuum` (13+), `last_seq_scan` / `last_idx_scan` (16+) kept as extras.
//! - **Their indexes** ([`indexes_sql`], [`index_row`]): `pg_index` with
//!   `pg_stat_user_indexes`, each key column as `pg_get_indexdef` writes it (`indnkeyatts`
//!   from 11, the `INCLUDE` columns left out; `indnatts` before), its size and scans, its
//!   predicate and an invalid index as extras.
//! - **The query's history** (`pg_stat_statements`, [`EXTENSION_SQL`], [`history_sql`],
//!   [`history_row`]): only when the extension is installed (never installed by us), and
//!   only by the server's own `queryid` — which `EXPLAIN` gives from 14 (`Query
//!   Identifier`): below it nothing is matched, a text normalized by us isn't the server's.
//!   Its rows of the current database added up (all users, top level or not): the calls,
//!   the mean time, the least and most, the rows, the share of blocks found in memory, the
//!   temporary blocks, the WAL (13+). The columns are `total_time`… up to 12,
//!   `total_exec_time`… from 13 ([`history_query`] writes either).
//!
//! Timestamps stay the server's text, to the second (`2026-10-30 12:00:00+00`): the domain
//! has no clock. Every value is read as text (`::text`), as [`crate`]'s catalog reads.

use database::TableRef;
use database::profile::{
    Extra, ExtraValue, IndexStats, Quantity, StatementStats, TableStats, Unit,
};

/// The savepoint an auxiliary query inside the user's transaction is undone to (a failing
/// one never aborts the user's transaction).
pub const SAVEPOINT: &str = "console_stats";

/// Whether `pg_stat_statements` is installed, and in which schema.
pub const EXTENSION_SQL: &str = "SELECT e.extversion::text, n.nspname::text \
     FROM pg_catalog.pg_extension e \
     JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace \
     WHERE e.extname = 'pg_stat_statements'";

/// The `wanted(schema, name)` list of `tables` placeholders (`$1, $2`, `$3, $4`…).
fn wanted(tables: usize) -> String {
    let rows: Vec<String> = (0..tables.max(1))
        .map(|i| format!("(${}::text, ${}::text)", 2 * i + 1, 2 * i + 2))
        .collect();
    format!("WITH wanted(schema, name) AS (VALUES {})", rows.join(", "))
}

/// The parameters of [`tables_sql`] / [`indexes_sql`]: each table's schema (`''`: the one
/// visible) and name.
pub fn params(tables: &[TableRef]) -> Vec<String> {
    tables.iter().flat_map(|t| [t.schema.clone().unwrap_or_default(), t.name.clone()]).collect()
}

/// A wanted table's row of `pg_class` (tables, materialized views, partitioned tables).
const MATCH: &str = "JOIN pg_catalog.pg_class c ON c.relname = w.name AND c.relkind IN ('r', 'm', 'p') \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace";
const WHERE: &str =
    "WHERE (w.schema = '' AND pg_catalog.pg_table_is_visible(c.oid)) OR n.nspname = w.schema";

/// A timestamp to the second, as text.
fn stamp(column: &str) -> String {
    format!("date_trunc('second', {column})::text")
}

/// The statistics of `tables` tables on `version` (columns in [`table_row`]'s order).
pub fn tables_sql(version: u32, tables: usize) -> String {
    let or_null =
        |since: u32, expr: String| if version >= since { expr } else { "NULL::text".to_owned() };
    let columns = [
        "n.nspname::text".to_owned(),
        "c.relname::text".to_owned(),
        "c.reltuples::text".to_owned(),
        "c.relpages::text".to_owned(),
        "s.n_live_tup::text".to_owned(),
        "s.n_dead_tup::text".to_owned(),
        "s.seq_scan::text".to_owned(),
        "s.idx_scan::text".to_owned(),
        stamp("s.last_analyze"),
        stamp("s.last_autoanalyze"),
        stamp("s.last_vacuum"),
        stamp("s.last_autovacuum"),
        or_null(90_400, "s.n_mod_since_analyze::text".to_owned()),
        "pg_catalog.pg_relation_size(c.oid)::text".to_owned(),
        "pg_catalog.pg_indexes_size(c.oid)::text".to_owned(),
        "pg_catalog.pg_total_relation_size(c.oid)::text".to_owned(),
        "s.vacuum_count::text".to_owned(),
        "s.autovacuum_count::text".to_owned(),
        "s.analyze_count::text".to_owned(),
        "s.autoanalyze_count::text".to_owned(),
        or_null(130_000, "s.n_ins_since_vacuum::text".to_owned()),
        or_null(160_000, stamp("s.last_seq_scan")),
        or_null(160_000, stamp("s.last_idx_scan")),
    ];
    format!(
        "{} SELECT {} FROM wanted w {MATCH} \
         LEFT JOIN pg_catalog.pg_stat_user_tables s ON s.relid = c.oid {WHERE} \
         ORDER BY n.nspname, c.relname",
        wanted(tables),
        columns.join(", ")
    )
}

/// The indexes of `tables` tables on `version` (columns in [`index_row`]'s order).
pub fn indexes_sql(version: u32, tables: usize) -> String {
    let keys = if version >= 110_000 { "x.indnkeyatts" } else { "x.indnatts" };
    format!(
        "{} SELECT n.nspname::text, c.relname::text, i.relname::text, x.indisunique::text, \
         x.indisprimary::text, \
         array_to_string(ARRAY(SELECT pg_catalog.pg_get_indexdef(x.indexrelid, k, true) \
         FROM pg_catalog.generate_series(1, {keys}) k ORDER BY k), chr(31)), \
         pg_catalog.pg_relation_size(x.indexrelid)::text, s.idx_scan::text, \
         s.idx_tup_read::text, s.idx_tup_fetch::text, \
         pg_catalog.pg_get_expr(x.indpred, x.indrelid), x.indisvalid::text \
         FROM wanted w {MATCH} \
         JOIN pg_catalog.pg_index x ON x.indrelid = c.oid \
         JOIN pg_catalog.pg_class i ON i.oid = x.indexrelid \
         LEFT JOIN pg_catalog.pg_stat_user_indexes s ON s.indexrelid = x.indexrelid {WHERE} \
         ORDER BY n.nspname, c.relname, i.relname",
        wanted(tables)
    )
}

/// `schema` quoted as an identifier.
fn quoted(schema: &str) -> String {
    format!("\"{}\"", schema.replace('"', "\"\""))
}

/// The history of one query (`$1`: its `queryid` as text) in `pg_stat_statements` of
/// `schema`, as `version` names its columns (any version from 9.4; columns in
/// [`history_row`]'s order). Matching needs the server's `queryid`: see [`history_sql`].
pub fn history_query(version: u32, schema: &str) -> String {
    let exec = if version >= 130_000 { "exec_" } else { "" };
    let timed = version >= 90_500;
    let agg = |f: &str, col: &str| {
        if timed { format!("{f}({col})::text") } else { "NULL::text".to_owned() }
    };
    let stddev = if timed {
        format!("(CASE WHEN count(*) = 1 THEN max(stddev_{exec}time) END)::text")
    } else {
        "NULL::text".to_owned()
    };
    let wal = if version >= 130_000 { "sum(wal_bytes)::text" } else { "NULL::text" };
    format!(
        "SELECT sum(calls)::text, \
         (sum(total_{exec}time) / NULLIF(sum(calls), 0))::text, \
         {}, {}, {stddev}, sum(rows)::text, \
         (sum(shared_blks_hit)::float8 / NULLIF(sum(shared_blks_hit) + sum(shared_blks_read), 0))::text, \
         sum(temp_blks_read + temp_blks_written)::text, {wal} \
         FROM {}.pg_stat_statements \
         WHERE queryid = $1::text::bigint \
         AND dbid = (SELECT oid FROM pg_catalog.pg_database WHERE datname = current_database())",
        agg("min", &format!("min_{exec}time")),
        agg("max", &format!("max_{exec}time")),
        quoted(schema)
    )
}

/// [`history_query`] where a query can be matched: from 14 (`EXPLAIN`'s `Query
/// Identifier`); `None` before (never guessed).
pub fn history_sql(version: u32, schema: &str) -> Option<String> {
    (version >= 140_000).then(|| history_query(version, schema))
}

fn text(row: &[Option<String>], i: usize) -> Option<String> {
    row.get(i).cloned().flatten().filter(|t| !t.is_empty())
}

fn number(row: &[Option<String>], i: usize) -> Option<f64> {
    text(row, i).and_then(|t| t.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
}

fn count(row: &[Option<String>], i: usize) -> Option<u64> {
    number(row, i).filter(|v| *v >= 0.0).map(|v| v.round() as u64)
}

fn quantity(key: &str, value: Option<u64>, unit: Unit) -> Option<Extra> {
    value.map(|v| Extra::new(key, ExtraValue::Quantity(Quantity::new(v as f64, unit))))
}

/// A row of [`tables_sql`] as the domain's statistics.
pub fn table_row(row: &[Option<String>]) -> TableStats {
    let table = TableRef { schema: text(row, 0), name: text(row, 1).unwrap_or_default() };
    let mut s = TableStats::new(table);
    s.live_rows = count(row, 4);
    s.dead_rows = count(row, 5);
    s.seq_scans = count(row, 6);
    s.idx_scans = count(row, 7);
    s.last_analyze = text(row, 8);
    s.last_autoanalyze = text(row, 9);
    let (vacuum, autovacuum) = (text(row, 10), text(row, 11));
    // The later of the two (the same form: ISO text compares as time, in one zone).
    s.last_vacuum = match (&vacuum, &autovacuum) {
        (Some(a), Some(b)) => Some(if b > a { b.clone() } else { a.clone() }),
        (a, b) => a.clone().or_else(|| b.clone()),
    };
    s.modified_since_analyze = count(row, 12);
    s.table_bytes = count(row, 13);
    s.index_bytes = count(row, 14);
    s.total_bytes = count(row, 15);
    if s.last_analyze.is_some() || s.last_autoanalyze.is_some() {
        s.stats_present = Some(true);
    }
    // The planner's own estimate (`-1` on 14+: never analyzed nor vacuumed).
    if let Some(tuples) = number(row, 2).filter(|t| *t >= 0.0) {
        s.extra.push(Extra::new(
            "reltuples",
            ExtraValue::Quantity(Quantity::new(tuples.round(), Unit::Rows)),
        ));
    }
    let extras = [
        quantity("relpages", count(row, 3), Unit::Pages),
        quantity("vacuum_count", count(row, 16), Unit::Count),
        quantity("autovacuum_count", count(row, 17), Unit::Count),
        quantity("analyze_count", count(row, 18), Unit::Count),
        quantity("autoanalyze_count", count(row, 19), Unit::Count),
        quantity("n_ins_since_vacuum", count(row, 20), Unit::Rows),
    ];
    s.extra.extend(extras.into_iter().flatten());
    for (key, value) in [
        ("last_vacuum", vacuum),
        ("last_autovacuum", autovacuum),
        ("last_seq_scan", text(row, 21)),
        ("last_idx_scan", text(row, 22)),
    ] {
        if let Some(v) = value {
            s.extra.push(Extra::new(key, ExtraValue::Text(v)));
        }
    }
    s
}

/// A row of [`indexes_sql`]: its table, and the index (its predicate, its counters and an
/// invalid one in `extra`, keyed by the index's name).
pub fn index_row(row: &[Option<String>]) -> (TableRef, IndexStats, Vec<Extra>) {
    let table = TableRef { schema: text(row, 0), name: text(row, 1).unwrap_or_default() };
    let columns =
        text(row, 5).map(|c| c.split('\u{1f}').map(str::to_owned).collect()).unwrap_or_default();
    let index = IndexStats {
        name: text(row, 2).unwrap_or_default(),
        columns,
        unique: text(row, 3).as_deref() == Some("true"),
        bytes: count(row, 6),
        scans: count(row, 7),
    };
    let mut extra: Vec<Extra> = [
        quantity("idx_tup_read", count(row, 8), Unit::Rows),
        quantity("idx_tup_fetch", count(row, 9), Unit::Rows),
    ]
    .into_iter()
    .flatten()
    .collect();
    if text(row, 4).as_deref() == Some("true") {
        extra.push(Extra::new("primary", ExtraValue::Bool(true)));
    }
    if let Some(predicate) = text(row, 10) {
        extra.push(Extra::new("predicate", ExtraValue::Text(predicate)));
    }
    if text(row, 11).as_deref() == Some("false") {
        extra.push(Extra::new("valid", ExtraValue::Bool(false)));
    }
    let group = format!("index {}", index.name);
    let extra = extra.into_iter().map(|e| e.in_group(group.clone())).collect();
    (table, index, extra)
}

/// The tables' rows with their indexes' attached (each index's extras grouped under its
/// name in its table's `extra`).
pub fn assemble(
    tables: &[Vec<Option<String>>],
    indexes: &[Vec<Option<String>>],
) -> Vec<TableStats> {
    let mut out: Vec<TableStats> = tables.iter().map(|r| table_row(r)).collect();
    for row in indexes {
        let (table, index, extra) = index_row(row);
        if let Some(t) = out.iter_mut().find(|t| t.table == table) {
            t.indexes.push(index);
            t.extra.extend(extra);
        }
    }
    out
}

/// A row of [`history_query`] (`None`: the query isn't in the history).
pub fn history_row(row: &[Option<String>]) -> Option<StatementStats> {
    let calls = count(row, 0).filter(|c| *c > 0)?;
    Some(StatementStats {
        calls: Some(calls),
        mean_ms: number(row, 1),
        min_ms: number(row, 2),
        max_ms: number(row, 3),
        stddev_ms: number(row, 4),
        rows: count(row, 5),
        hit_ratio: number(row, 6).map(|r| r.clamp(0.0, 1.0)),
        temp_blocks: count(row, 7),
        wal_bytes: count(row, 8),
    })
}

#[cfg(test)]
#[path = "../tests/unit/stats.rs"]
mod tests;
