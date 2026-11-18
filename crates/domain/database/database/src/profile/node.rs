//! A step of a profile ([`ProfileNode`]): what it does, normalized for every engine
//! ([`OpKind`]) next to the engine's own words, what it works on, its conditions, the
//! planner's estimates, what it really did when it ran, its I/O, memory, spill, workers and
//! WAL, and the rest as [`Extra`]. Every number is an `Option`: unknown is never zero.

use crate::profile::extra::Extra;

/// A node's place in its profile's flat list (its identity there: a profile never
/// changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// How an aggregate groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggStrategy {
    Plain,
    Sorted,
    Hashed,
    Mixed,
}

/// What a write step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteKind {
    Insert,
    Update,
    Delete,
    Merge,
}

/// What a step does, the same for every engine (its own words are [`ProfileNode::name`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpKind {
    SeqScan,
    IndexScan,
    IndexOnlyScan,
    BitmapHeapScan,
    BitmapIndexScan,
    /// SQLite's `INTEGER PRIMARY KEY` search.
    RowidLookup,
    TidScan,
    FunctionScan,
    ValuesScan,
    CteScan,
    SubqueryScan,
    NestedLoop,
    HashJoin,
    MergeJoin,
    Hash,
    Sort,
    IncrementalSort,
    /// SQLite's `USE TEMP B-TREE` (a sort, a grouping, a distinct).
    TempBTree,
    Aggregate(AggStrategy),
    WindowAgg,
    Unique,
    SetOp,
    Limit,
    Materialize,
    Memoize,
    Gather,
    GatherMerge,
    Append,
    MergeAppend,
    Result,
    ModifyTable(WriteKind),
    LockRows,
    /// An index the engine builds for the statement alone (SQLite's automatic index).
    AutoIndex,
    BloomFilter,
    MultiIndexOr,
    Coroutine,
    Compound,
    Other,
}

impl OpKind {
    /// Stable (the exported JSON's, the protocol's): `seq_scan`, `aggregate_hashed`…
    pub fn id(self) -> &'static str {
        use OpKind as O;
        match self {
            O::SeqScan => "seq_scan",
            O::IndexScan => "index_scan",
            O::IndexOnlyScan => "index_only_scan",
            O::BitmapHeapScan => "bitmap_heap_scan",
            O::BitmapIndexScan => "bitmap_index_scan",
            O::RowidLookup => "rowid_lookup",
            O::TidScan => "tid_scan",
            O::FunctionScan => "function_scan",
            O::ValuesScan => "values_scan",
            O::CteScan => "cte_scan",
            O::SubqueryScan => "subquery_scan",
            O::NestedLoop => "nested_loop",
            O::HashJoin => "hash_join",
            O::MergeJoin => "merge_join",
            O::Hash => "hash",
            O::Sort => "sort",
            O::IncrementalSort => "incremental_sort",
            O::TempBTree => "temp_btree",
            O::Aggregate(AggStrategy::Plain) => "aggregate_plain",
            O::Aggregate(AggStrategy::Sorted) => "aggregate_sorted",
            O::Aggregate(AggStrategy::Hashed) => "aggregate_hashed",
            O::Aggregate(AggStrategy::Mixed) => "aggregate_mixed",
            O::WindowAgg => "window_agg",
            O::Unique => "unique",
            O::SetOp => "set_op",
            O::Limit => "limit",
            O::Materialize => "materialize",
            O::Memoize => "memoize",
            O::Gather => "gather",
            O::GatherMerge => "gather_merge",
            O::Append => "append",
            O::MergeAppend => "merge_append",
            O::Result => "result",
            O::ModifyTable(WriteKind::Insert) => "insert",
            O::ModifyTable(WriteKind::Update) => "update",
            O::ModifyTable(WriteKind::Delete) => "delete",
            O::ModifyTable(WriteKind::Merge) => "merge",
            O::LockRows => "lock_rows",
            O::AutoIndex => "auto_index",
            O::BloomFilter => "bloom_filter",
            O::MultiIndexOr => "multi_index_or",
            O::Coroutine => "coroutine",
            O::Compound => "compound",
            O::Other => "other",
        }
    }

    pub const ALL: [OpKind; 44] = {
        use OpKind as O;
        [
            O::SeqScan,
            O::IndexScan,
            O::IndexOnlyScan,
            O::BitmapHeapScan,
            O::BitmapIndexScan,
            O::RowidLookup,
            O::TidScan,
            O::FunctionScan,
            O::ValuesScan,
            O::CteScan,
            O::SubqueryScan,
            O::NestedLoop,
            O::HashJoin,
            O::MergeJoin,
            O::Hash,
            O::Sort,
            O::IncrementalSort,
            O::TempBTree,
            O::Aggregate(AggStrategy::Plain),
            O::Aggregate(AggStrategy::Sorted),
            O::Aggregate(AggStrategy::Hashed),
            O::Aggregate(AggStrategy::Mixed),
            O::WindowAgg,
            O::Unique,
            O::SetOp,
            O::Limit,
            O::Materialize,
            O::Memoize,
            O::Gather,
            O::GatherMerge,
            O::Append,
            O::MergeAppend,
            O::Result,
            O::ModifyTable(WriteKind::Insert),
            O::ModifyTable(WriteKind::Update),
            O::ModifyTable(WriteKind::Delete),
            O::ModifyTable(WriteKind::Merge),
            O::LockRows,
            O::AutoIndex,
            O::BloomFilter,
            O::MultiIndexOr,
            O::Coroutine,
            O::Compound,
            O::Other,
        ]
    };

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.id() == id)
    }

    /// The kind an engine's words most likely mean (`Seq Scan`, `Hash Join (Left)`,
    /// `Aggregate`): a plan of an older driver, which gives only its words. `Other` when
    /// nothing fits.
    pub fn guess(name: &str) -> Self {
        use OpKind as O;
        let lower = name.trim().to_ascii_lowercase();
        let starts = |p: &str| lower.starts_with(p);
        // PostgreSQL's names first (exactly as it writes them), then a few common words.
        let exact: &[(&str, OpKind)] = &[
            ("parallel seq scan", O::SeqScan),
            ("seq scan", O::SeqScan),
            ("parallel index only scan", O::IndexOnlyScan),
            ("index only scan", O::IndexOnlyScan),
            ("parallel index scan", O::IndexScan),
            ("index scan", O::IndexScan),
            ("parallel bitmap heap scan", O::BitmapHeapScan),
            ("bitmap heap scan", O::BitmapHeapScan),
            ("bitmap index scan", O::BitmapIndexScan),
            ("tid range scan", O::TidScan),
            ("tid scan", O::TidScan),
            ("function scan", O::FunctionScan),
            ("table function scan", O::FunctionScan),
            ("values scan", O::ValuesScan),
            ("cte scan", O::CteScan),
            ("subquery scan", O::SubqueryScan),
            ("nested loop", O::NestedLoop),
            ("parallel hash join", O::HashJoin),
            ("hash join", O::HashJoin),
            ("merge join", O::MergeJoin),
            ("parallel hash", O::Hash),
            ("hash", O::Hash),
            ("incremental sort", O::IncrementalSort),
            ("sort", O::Sort),
            ("windowagg", O::WindowAgg),
            ("unique", O::Unique),
            ("setop", O::SetOp),
            ("hashsetop", O::SetOp),
            ("limit", O::Limit),
            ("materialize", O::Materialize),
            ("memoize", O::Memoize),
            ("gather merge", O::GatherMerge),
            ("gather", O::Gather),
            ("parallel append", O::Append),
            ("append", O::Append),
            ("merge append", O::MergeAppend),
            ("result", O::Result),
            ("projectset", O::Result),
            ("lockrows", O::LockRows),
        ];
        for (words, kind) in exact {
            if lower == *words || lower.starts_with(&format!("{words} (")) {
                return *kind;
            }
        }
        if starts("aggregate") || starts("groupaggregate") || starts("group") {
            return O::Aggregate(if starts("group") {
                AggStrategy::Sorted
            } else {
                AggStrategy::Plain
            });
        }
        if starts("hashaggregate") {
            return O::Aggregate(AggStrategy::Hashed);
        }
        if starts("mixedaggregate") {
            return O::Aggregate(AggStrategy::Mixed);
        }
        if starts("insert") {
            return O::ModifyTable(WriteKind::Insert);
        }
        if starts("update") {
            return O::ModifyTable(WriteKind::Update);
        }
        if starts("delete") {
            return O::ModifyTable(WriteKind::Delete);
        }
        if starts("merge") {
            return O::ModifyTable(WriteKind::Merge);
        }
        if starts("seq") || lower.contains("table scan") || lower.contains("full scan") {
            return O::SeqScan;
        }
        if lower.contains("index scan") {
            return O::IndexScan;
        }
        O::Other
    }

    /// A scan of a table or an index (what reads rows from storage).
    pub fn is_scan(self) -> bool {
        use OpKind as O;
        matches!(
            self,
            O::SeqScan
                | O::IndexScan
                | O::IndexOnlyScan
                | O::BitmapHeapScan
                | O::BitmapIndexScan
                | O::RowidLookup
                | O::TidScan
                | O::AutoIndex
        )
    }

    /// It brings the rows of several workers together (PostgreSQL's `Gather`).
    pub fn gathers(self) -> bool {
        matches!(self, OpKind::Gather | OpKind::GatherMerge)
    }
}

/// How a join keeps its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Semi,
    Anti,
    RightSemi,
    RightAnti,
}

impl JoinKind {
    pub const ALL: [JoinKind; 8] = [
        JoinKind::Inner,
        JoinKind::Left,
        JoinKind::Right,
        JoinKind::Full,
        JoinKind::Semi,
        JoinKind::Anti,
        JoinKind::RightSemi,
        JoinKind::RightAnti,
    ];

    pub fn id(self) -> &'static str {
        match self {
            JoinKind::Inner => "inner",
            JoinKind::Left => "left",
            JoinKind::Right => "right",
            JoinKind::Full => "full",
            JoinKind::Semi => "semi",
            JoinKind::Anti => "anti",
            JoinKind::RightSemi => "right_semi",
            JoinKind::RightAnti => "right_anti",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|j| j.id() == id)
    }

    /// PostgreSQL's words (`Left`, `Right Anti`).
    pub fn words(self) -> &'static str {
        match self {
            JoinKind::Inner => "Inner",
            JoinKind::Left => "Left",
            JoinKind::Right => "Right",
            JoinKind::Full => "Full",
            JoinKind::Semi => "Semi",
            JoinKind::Anti => "Anti",
            JoinKind::RightSemi => "Right Semi",
            JoinKind::RightAnti => "Right Anti",
        }
    }

    pub fn from_words(words: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|j| j.words().eq_ignore_ascii_case(words.trim()))
    }
}

/// What a child is to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildRole {
    Outer,
    Inner,
    /// Run once, before its parent needs it (PostgreSQL's `InitPlan`).
    InitPlan,
    /// Run for each row (or once, cached) of its parent (a correlated subquery).
    SubPlan,
    /// A member of an `Append` (a partition, a part of a `UNION`).
    Member,
    Subquery,
    /// A common table expression's plan (SQLite's `CO-ROUTINE` / `MATERIALIZE`).
    Cte,
}

impl ChildRole {
    pub const ALL: [ChildRole; 7] = [
        ChildRole::Outer,
        ChildRole::Inner,
        ChildRole::InitPlan,
        ChildRole::SubPlan,
        ChildRole::Member,
        ChildRole::Subquery,
        ChildRole::Cte,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ChildRole::Outer => "outer",
            ChildRole::Inner => "inner",
            ChildRole::InitPlan => "init_plan",
            ChildRole::SubPlan => "sub_plan",
            ChildRole::Member => "member",
            ChildRole::Subquery => "subquery",
            ChildRole::Cte => "cte",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }
}

/// What an object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Table,
    Index,
    Cte,
    Function,
    Subquery,
    Other,
}

impl ObjectKind {
    pub const ALL: [ObjectKind; 6] = [
        ObjectKind::Table,
        ObjectKind::Index,
        ObjectKind::Cte,
        ObjectKind::Function,
        ObjectKind::Subquery,
        ObjectKind::Other,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ObjectKind::Table => "table",
            ObjectKind::Index => "index",
            ObjectKind::Cte => "cte",
            ObjectKind::Function => "function",
            ObjectKind::Subquery => "subquery",
            ObjectKind::Other => "other",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// What a step works on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    pub kind: ObjectKind,
    pub schema: Option<String>,
    pub name: String,
    /// Its alias in the statement, when it has one.
    pub alias: Option<String>,
}

impl ObjectRef {
    pub fn new(kind: ObjectKind, name: impl Into<String>) -> Self {
        Self { kind, schema: None, name: name.into(), alias: None }
    }

    /// As a plan names it: `schema.name`, else `name alias` (an alias of its own), else
    /// `name`.
    pub fn label(&self) -> String {
        match (&self.schema, &self.alias) {
            (Some(schema), _) => format!("{schema}.{}", self.name),
            (None, Some(alias)) if *alias != self.name => format!("{} {alias}", self.name),
            _ => self.name.clone(),
        }
    }
}

/// Which way an index is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScanDirection {
    Forward,
    Backward,
    NoMovement,
}

impl ScanDirection {
    pub fn id(self) -> &'static str {
        match self {
            ScanDirection::Forward => "forward",
            ScanDirection::Backward => "backward",
            ScanDirection::NoMovement => "no_movement",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Forward, Self::Backward, Self::NoMovement].into_iter().find(|d| d.id() == id)
    }
}

/// What a condition is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CondKind {
    IndexCond,
    Recheck,
    Filter,
    JoinFilter,
    HashCond,
    MergeCond,
    OneTimeFilter,
    SortKey,
    GroupKey,
    Presorted,
    CacheKey,
    /// Another kind, by the engine's label (empty: no label, the engine's text alone).
    Other(String),
}

impl CondKind {
    /// Its label in a plan (PostgreSQL's words: `Index Cond`, `Sort Key`).
    pub fn label(&self) -> &str {
        match self {
            CondKind::IndexCond => "Index Cond",
            CondKind::Recheck => "Recheck Cond",
            CondKind::Filter => "Filter",
            CondKind::JoinFilter => "Join Filter",
            CondKind::HashCond => "Hash Cond",
            CondKind::MergeCond => "Merge Cond",
            CondKind::OneTimeFilter => "One-Time Filter",
            CondKind::SortKey => "Sort Key",
            CondKind::GroupKey => "Group Key",
            CondKind::Presorted => "Presorted Key",
            CondKind::CacheKey => "Cache Key",
            CondKind::Other(label) => label,
        }
    }

    /// Stable (the exported JSON's): `index_cond`… (`other` with its label apart).
    pub fn id(&self) -> &'static str {
        match self {
            CondKind::IndexCond => "index_cond",
            CondKind::Recheck => "recheck",
            CondKind::Filter => "filter",
            CondKind::JoinFilter => "join_filter",
            CondKind::HashCond => "hash_cond",
            CondKind::MergeCond => "merge_cond",
            CondKind::OneTimeFilter => "one_time_filter",
            CondKind::SortKey => "sort_key",
            CondKind::GroupKey => "group_key",
            CondKind::Presorted => "presorted",
            CondKind::CacheKey => "cache_key",
            CondKind::Other(_) => "other",
        }
    }

    /// From its id (`other` takes `label`).
    pub fn from_id(id: &str, label: Option<&str>) -> Option<Self> {
        Some(match id {
            "index_cond" => CondKind::IndexCond,
            "recheck" => CondKind::Recheck,
            "filter" => CondKind::Filter,
            "join_filter" => CondKind::JoinFilter,
            "hash_cond" => CondKind::HashCond,
            "merge_cond" => CondKind::MergeCond,
            "one_time_filter" => CondKind::OneTimeFilter,
            "sort_key" => CondKind::SortKey,
            "group_key" => CondKind::GroupKey,
            "presorted" => CondKind::Presorted,
            "cache_key" => CondKind::CacheKey,
            "other" => CondKind::Other(label.unwrap_or_default().to_owned()),
            _ => return None,
        })
    }
}

/// A condition of a step, as the engine writes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Condition {
    pub kind: CondKind,
    pub text: String,
}

impl Condition {
    pub fn new(kind: CondKind, text: impl Into<String>) -> Self {
        Self { kind, text: text.into() }
    }
}

/// What the planner expected.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Estimates {
    pub startup_cost: Option<f64>,
    pub total_cost: Option<f64>,
    /// Rows per loop.
    pub rows: Option<f64>,
    /// Bytes a row.
    pub width: Option<f64>,
}

/// What a step really did when it ran (per loop, as the engines give them).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Actuals {
    pub rows: Option<f64>,
    pub loops: Option<f64>,
    pub startup_ms: Option<f64>,
    pub total_ms: Option<f64>,
    /// The plan had it, but it never ran (a branch skipped).
    pub never_executed: bool,
    pub rows_removed_by_filter: Option<f64>,
    pub rows_removed_by_join_filter: Option<f64>,
    pub rows_removed_by_recheck: Option<f64>,
    pub heap_fetches: Option<f64>,
    /// The index's descents (PostgreSQL 18's `Index Searches`).
    pub index_searches: Option<f64>,
    /// A bitmap's exact and lossy heap blocks.
    pub exact_blocks: Option<f64>,
    pub lossy_blocks: Option<f64>,
}

/// I/O, in blocks of [`Io::block_size`] bytes, with its times.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Io {
    pub shared_hit: Option<u64>,
    pub shared_read: Option<u64>,
    pub shared_dirtied: Option<u64>,
    pub shared_written: Option<u64>,
    pub local_hit: Option<u64>,
    pub local_read: Option<u64>,
    pub local_dirtied: Option<u64>,
    pub local_written: Option<u64>,
    pub temp_read: Option<u64>,
    pub temp_written: Option<u64>,
    /// Reading and writing shared blocks (with `track_io_timing`).
    pub read_ms: Option<f64>,
    pub write_ms: Option<f64>,
    pub local_read_ms: Option<f64>,
    pub local_write_ms: Option<f64>,
    pub temp_read_ms: Option<f64>,
    pub temp_write_ms: Option<f64>,
    pub block_size: Option<u32>,
}

impl Io {
    /// Nothing is known.
    pub fn is_empty(&self) -> bool {
        *self == Io { block_size: self.block_size, ..Io::default() }
    }

    /// The sum of two (what each knows; a block size kept).
    pub fn add(&self, other: &Io) -> Io {
        fn n(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            match (a, b) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            }
        }
        fn t(a: Option<f64>, b: Option<f64>) -> Option<f64> {
            match (a, b) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
            }
        }
        Io {
            shared_hit: n(self.shared_hit, other.shared_hit),
            shared_read: n(self.shared_read, other.shared_read),
            shared_dirtied: n(self.shared_dirtied, other.shared_dirtied),
            shared_written: n(self.shared_written, other.shared_written),
            local_hit: n(self.local_hit, other.local_hit),
            local_read: n(self.local_read, other.local_read),
            local_dirtied: n(self.local_dirtied, other.local_dirtied),
            local_written: n(self.local_written, other.local_written),
            temp_read: n(self.temp_read, other.temp_read),
            temp_written: n(self.temp_written, other.temp_written),
            read_ms: t(self.read_ms, other.read_ms),
            write_ms: t(self.write_ms, other.write_ms),
            local_read_ms: t(self.local_read_ms, other.local_read_ms),
            local_write_ms: t(self.local_write_ms, other.local_write_ms),
            temp_read_ms: t(self.temp_read_ms, other.temp_read_ms),
            temp_write_ms: t(self.temp_write_ms, other.temp_write_ms),
            block_size: self.block_size.or(other.block_size),
        }
    }

    /// Blocks as bytes (`block_size`, else PostgreSQL's 8 KB).
    pub fn bytes(&self, blocks: u64) -> u64 {
        blocks * u64::from(self.block_size.unwrap_or(8192))
    }
}

/// Where something was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Storage {
    Memory,
    Disk,
}

impl Storage {
    pub fn id(self) -> &'static str {
        match self {
            Storage::Memory => "memory",
            Storage::Disk => "disk",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_ascii_lowercase().as_str() {
            "memory" => Some(Storage::Memory),
            "disk" => Some(Storage::Disk),
            _ => None,
        }
    }
}

/// A hash table's sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HashStats {
    pub buckets: Option<u64>,
    pub original_buckets: Option<u64>,
    pub batches: Option<u64>,
    pub original_batches: Option<u64>,
}

/// A cache's use (PostgreSQL's `Memoize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub hits: Option<u64>,
    pub misses: Option<u64>,
    pub evictions: Option<u64>,
    pub overflows: Option<u64>,
}

/// Memory a step used.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Memory {
    pub peak_bytes: Option<u64>,
    /// `quicksort`, `top-N heapsort`, `external merge`…
    pub sort_method: Option<String>,
    pub sort_space: Option<(u64, Storage)>,
    pub hash: Option<HashStats>,
    pub cache: Option<CacheStats>,
    /// Where a material, window or CTE kept its rows (PostgreSQL 18).
    pub storage: Option<Storage>,
}

/// What spilled to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpillKind {
    Sort,
    Hash,
    HashAgg,
    Material,
    Window,
    Cte,
}

impl SpillKind {
    pub const ALL: [SpillKind; 6] = [
        SpillKind::Sort,
        SpillKind::Hash,
        SpillKind::HashAgg,
        SpillKind::Material,
        SpillKind::Window,
        SpillKind::Cte,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SpillKind::Sort => "sort",
            SpillKind::Hash => "hash",
            SpillKind::HashAgg => "hash_agg",
            SpillKind::Material => "material",
            SpillKind::Window => "window",
            SpillKind::Cte => "cte",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// A step's work that didn't fit in memory (worked out by the driver from what the engine
/// says).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spill {
    pub kind: SpillKind,
    pub disk_bytes: Option<u64>,
    pub batches: Option<u64>,
}

/// What one parallel worker did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkerStats {
    pub number: u32,
    pub rows: Option<f64>,
    pub loops: Option<f64>,
    pub startup_ms: Option<f64>,
    pub total_ms: Option<f64>,
    pub io: Option<Io>,
    pub extra: Vec<Extra>,
}

/// Parallelism.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parallel {
    pub planned: Option<u32>,
    pub launched: Option<u32>,
    /// It is the parallel form of the step (`Parallel Seq Scan`).
    pub aware: bool,
    pub per_worker: Vec<WorkerStats>,
}

/// The write-ahead log a step wrote (PostgreSQL 13+).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wal {
    pub records: Option<u64>,
    pub full_page_images: Option<u64>,
    pub bytes: Option<u64>,
    pub buffers_full: Option<u64>,
}

/// Marks of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeFlags {
    /// The planner was told not to use it (PostgreSQL 18's `Disabled`, or a cost of 1e10
    /// before).
    pub disabled: bool,
    pub async_capable: bool,
    /// Part of the work: a partial aggregate, a partial scan.
    pub partial: bool,
}

/// A step of a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileNode {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub role: Option<ChildRole>,
    /// `InitPlan 1 (returns $0)`, `CTE totals`, `SubPlan 2`.
    pub subplan_name: Option<String>,
    pub op: OpKind,
    /// The engine's words (`Seq Scan`, `SEARCH users USING INDEX …`), always shown.
    pub name: String,
    pub join: Option<JoinKind>,
    pub object: Option<ObjectRef>,
    pub index: Option<String>,
    pub scan_direction: Option<ScanDirection>,
    pub conditions: Vec<Condition>,
    /// The columns it gives (PostgreSQL's `VERBOSE`).
    pub output: Vec<String>,
    pub estimates: Estimates,
    pub actuals: Actuals,
    pub io: Option<Io>,
    pub memory: Option<Memory>,
    pub spill: Option<Spill>,
    pub parallel: Option<Parallel>,
    pub wal: Option<Wal>,
    pub flags: NodeFlags,
    pub extra: Vec<Extra>,
}

impl ProfileNode {
    pub fn new(op: OpKind, name: impl Into<String>) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            role: None,
            subplan_name: None,
            op,
            name: name.into(),
            join: None,
            object: None,
            index: None,
            scan_direction: None,
            conditions: Vec::new(),
            output: Vec::new(),
            estimates: Estimates::default(),
            actuals: Actuals::default(),
            io: None,
            memory: None,
            spill: None,
            parallel: None,
            wal: None,
            flags: NodeFlags::default(),
            extra: Vec::new(),
        }
    }

    /// Its rows in all its loops, when it ran.
    pub fn total_rows(&self) -> Option<f64> {
        self.actuals.rows.map(|r| r * self.actuals.loops.unwrap_or(1.0).max(1.0))
    }

    /// It ran (real rows or times are known).
    pub fn ran(&self) -> bool {
        self.actuals.rows.is_some() || self.actuals.total_ms.is_some()
    }

    /// The first condition of `kind`.
    pub fn condition(&self, kind: &CondKind) -> Option<&str> {
        self.conditions.iter().find(|c| c.kind == *kind).map(|c| c.text.as_str())
    }
}
