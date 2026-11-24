//! Profiles (protocol 3): a statement's plan, its nodes' figures, the options asked.
//!
//! The same names as the editor's exported JSON (`database::profile::export::to_json`): a
//! profile written by one reads as the other. `format` is the profile's own (separate from
//! the protocol's version): a field added is optional, a larger format is read as far as it's
//! understood.

use super::TableRef;
use serde::{Deserialize, Serialize};

/// What a driver's profiles hold (`[driver.capabilities.console.profile]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileCaps {
    pub estimated: bool,
    pub actual: bool,
    pub node_timing: bool,
    pub buffers: bool,
    pub io_timing: bool,
    pub wal: bool,
    pub memory: bool,
    pub spill: bool,
    pub parallel: bool,
    pub settings: bool,
    pub verbose_output: bool,
    pub jit: bool,
    pub triggers: bool,
    pub engine_counters: bool,
    pub node_actuals_by_scan: bool,
    pub statement_history: bool,
    pub table_stats: bool,
    pub undo_writes: bool,
}

/// `profile`'s parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProfileParams {
    pub sql: String,
    #[serde(default)]
    pub options: ProfileOptions,
}

/// What a profiling asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileOptions {
    /// `estimated`, `actual` or `actual_no_timing`.
    pub depth: String,
    pub buffers: bool,
    pub verbose: bool,
    pub settings: bool,
    pub wal: bool,
    pub serialize: bool,
    pub statement_history: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_raw_bytes: Option<u64>,
}

impl Default for ProfileOptions {
    fn default() -> Self {
        Self {
            depth: "estimated".to_owned(),
            buffers: true,
            verbose: false,
            settings: true,
            wal: true,
            serialize: false,
            statement_history: false,
            timeout_ms: None,
            max_raw_bytes: None,
        }
    }
}

/// `table_stats`' parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TableStatsParams {
    pub tables: Vec<TableRef>,
}

macro_rules! optional_fields {
    ($(#[$meta:meta])* pub struct $name:ident { $($(#[$fmeta:meta])* pub $field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name {
            $(
                $(#[$fmeta])*
                #[serde(skip_serializing_if = "Option::is_none")]
                pub $field: Option<$ty>,
            )*
        }
    };
}

/// A profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub format: u16,
    /// A UUID (the editor makes one when it's missing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub engine: Engine,
    /// `estimated`, `actual`, `actual_no_timing`.
    pub depth: String,
    pub statement: StatementProfile,
    pub nodes: Vec<ProfileNode>,
    pub roots: Vec<u32>,
    pub collected: ProfileCaps,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<ProfileNote>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Engine {
    pub driver: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_num: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileNote {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RawPlan {
    /// `json`, `text`, `rows`.
    pub format: String,
    pub text: String,
}

/// A step of a profile, in the profile's flat list (`parent` and `children` are places in
/// it).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileNode {
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subplan_name: Option<String>,
    /// `seq_scan`, `hash_join`… (`other` for anything else: `name` says it).
    pub op: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_direction: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimates: Option<Estimates>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actuals: Option<Actuals>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io: Option<Io>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<Memory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill: Option<Spill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel: Option<Parallel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wal: Option<Wal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<Flags>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<Extra>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectRef {
    /// `table`, `index`, `cte`, `function`, `subquery`, `other`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Condition {
    /// `index_cond`, `filter`…, `other` (with its `label`).
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub text: String,
}

optional_fields! {
    pub struct Estimates {
        pub startup_cost: f64,
        pub total_cost: f64,
        pub rows: f64,
        pub width: f64,
    }
}

/// What a step did (per loop).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Actuals {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loops: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub never_executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_removed_by_filter: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_removed_by_join_filter: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_removed_by_recheck: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap_fetches: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_searches: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_blocks: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lossy_blocks: Option<f64>,
}

/// I/O in blocks, with its times (ms).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Io {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_hit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_dirtied: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_written: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_hit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_dirtied: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_written: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_written: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_read_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_write_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_read_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_write_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_size: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Memory {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_space: Option<SortSpace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<HashStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheStats>,
    /// `memory` or `disk`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SortSpace {
    pub bytes: u64,
    pub storage: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HashStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buckets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_buckets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batches: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_batches: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub misses: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evictions: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflows: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Spill {
    /// `sort`, `hash`, `hash_agg`, `material`, `window`, `cte`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batches: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Parallel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launched: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub aware: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub per_worker: Vec<Worker>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Worker {
    pub number: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loops: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io: Option<Io>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<Extra>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Wal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub records: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_page_images: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffers_full: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Flags {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub async_capable: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
}

/// Something an engine said beyond the model: `{key, label?, group?, value: {type, value,
/// unit?}}`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Extra {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub value: ExtraValue,
}

/// An extra's value: `type` is `bool`, `int`, `float`, `text`, `quantity` (with its `unit`),
/// `list` (its `value` a list of values) or `group` (its `value` a list of extras).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtraValue {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StatementProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planning_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_row_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_returned: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_affected: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planning_io: Option<Io>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_total: Option<Io>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planning_memory: Option<PlanningMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serialize: Option<Serialization>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jit: Option<Jit>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Trigger>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counters: Option<Counters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<History>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub waits: Vec<Wait>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<Extra>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanningMemory {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocated_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Serialization {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

optional_fields! {
    pub struct Jit {
        pub functions: u64,
        pub generation_ms: f64,
        pub inlining_ms: f64,
        pub optimization_ms: f64,
        pub emission_ms: f64,
        pub deform_ms: f64,
        pub total_ms: f64,
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Trigger {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Setting {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Counters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fullscan_steps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sorts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoindex: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm_steps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reprepare: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_hits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_misses: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_examined: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub select_scan: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_merge_passes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmp_disk_tables: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stddev_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_blocks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wal_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Wait {
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

/// A table's statistics (`table_stats`' answer is a list of them).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TableStats {
    pub table: TableRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq_scans: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idx_scans: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_analyze: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_autoanalyze: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_vacuum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_since_analyze: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats_present: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexStats>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<Extra>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexStats {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scans: Option<u64>,
}
