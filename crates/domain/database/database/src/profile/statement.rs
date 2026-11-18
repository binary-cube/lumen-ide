//! What a profile knows of the statement as a whole ([`StatementProfile`]): the engine's
//! planning and execution times, the runner's own clock, rows, I/O, JIT, triggers, the
//! planner's settings, the engine's counters and the server's history of the same query.

use crate::profile::extra::Extra;
use crate::profile::node::Io;

/// The planner's memory (PostgreSQL 17's `MEMORY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanningMemory {
    pub used_bytes: Option<u64>,
    pub allocated_bytes: Option<u64>,
}

/// Turning the rows into what the client gets (PostgreSQL 17's `SERIALIZE`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Serialize {
    pub ms: Option<f64>,
    pub bytes: Option<u64>,
    /// `text` or `binary`.
    pub format: Option<String>,
}

/// Just-in-time compilation (PostgreSQL 11+).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Jit {
    pub functions: Option<u64>,
    pub generation_ms: Option<f64>,
    /// `None`: not done (its option off).
    pub inlining_ms: Option<f64>,
    pub optimization_ms: Option<f64>,
    pub emission_ms: Option<f64>,
    /// Deforming tuples (PostgreSQL 17, part of the generation).
    pub deform_ms: Option<f64>,
    pub total_ms: Option<f64>,
}

/// A trigger's time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriggerTime {
    pub name: String,
    pub relation: Option<String>,
    pub calls: Option<u64>,
    pub ms: Option<f64>,
}

/// A planner setting different from its default (PostgreSQL 12+'s `SETTINGS`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Setting {
    pub name: String,
    pub value: String,
}

/// The engine's own counters for the statement: SQLite's `sqlite3_stmt_status`, MySQL's
/// session status differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineCounters {
    /// Rows stepped over in full scans (SQLite `FULLSCAN_STEP`).
    pub fullscan_steps: Option<u64>,
    pub sorts: Option<u64>,
    /// Rows put in automatic indexes.
    pub autoindex: Option<u64>,
    pub vm_steps: Option<u64>,
    pub reprepare: Option<u64>,
    pub runs: Option<u64>,
    /// A Bloom filter's hits and misses.
    pub filter_hits: Option<u64>,
    pub filter_misses: Option<u64>,
    pub mem_used: Option<u64>,
    /// MySQL (later): `Handler_read_*`, `Select_scan`, `Sort_merge_passes`,
    /// `Created_tmp_disk_tables`.
    pub rows_examined: Option<u64>,
    pub select_scan: Option<u64>,
    pub sort_merge_passes: Option<u64>,
    pub tmp_disk_tables: Option<u64>,
}

impl EngineCounters {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What the server remembers of the same query (`pg_stat_statements`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StatementStats {
    pub calls: Option<u64>,
    pub mean_ms: Option<f64>,
    pub stddev_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub rows: Option<u64>,
    /// Shared blocks found in memory, of those asked (0–1).
    pub hit_ratio: Option<f64>,
    pub temp_blocks: Option<u64>,
    pub wal_bytes: Option<u64>,
}

/// What it waited for (MySQL's `performance_schema`, later: the field is here so the
/// format doesn't change).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wait {
    pub event: String,
    pub ms: Option<f64>,
    pub count: Option<u64>,
}

/// The statement's profile as a whole.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatementProfile {
    /// The engine's (PostgreSQL's `Planning Time` / `Execution Time`; `Total runtime`
    /// before 9.4).
    pub planning_ms: Option<f64>,
    pub execution_ms: Option<f64>,
    /// The runner's clock: the whole profile (the network included), its first row, the
    /// rows brought.
    pub wall_ms: Option<f64>,
    pub first_row_ms: Option<f64>,
    pub fetch_ms: Option<f64>,
    pub rows_returned: Option<u64>,
    pub rows_affected: Option<u64>,
    pub planning_io: Option<Io>,
    /// Every node's I/O added up (worked out).
    pub io_total: Option<Io>,
    pub planning_memory: Option<PlanningMemory>,
    pub serialize: Option<Serialize>,
    pub jit: Option<Jit>,
    pub triggers: Vec<TriggerTime>,
    pub settings: Vec<Setting>,
    pub query_id: Option<i64>,
    pub counters: Option<EngineCounters>,
    pub history: Option<StatementStats>,
    pub waits: Vec<Wait>,
    pub extra: Vec<Extra>,
}
