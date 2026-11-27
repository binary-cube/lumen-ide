//! What a driver can do: the UI offers only that.

/// Which schema changes a driver makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchemaChanges {
    /// Create and drop tables.
    pub tables: bool,
    /// Add and drop columns.
    pub columns: bool,
    /// Change a column's type, nullability or default.
    pub alter_columns: bool,
    /// Create and drop indexes.
    pub indexes: bool,
    /// Rename tables and columns.
    pub renames: bool,
    /// Add and drop foreign keys of an existing table.
    pub foreign_keys: bool,
    /// Move a column to another place among the columns.
    pub reorder: bool,
    /// Create databases (MySQL, PostgreSQL).
    pub databases: bool,
    /// Create schemas (PostgreSQL).
    pub schemas: bool,
}

impl SchemaChanges {
    pub const ALL: Self = Self {
        tables: true,
        columns: true,
        alter_columns: true,
        indexes: true,
        renames: true,
        foreign_keys: true,
        reorder: true,
        databases: true,
        schemas: true,
    };

    pub fn any(self) -> bool {
        self.tables
            || self.columns
            || self.alter_columns
            || self.indexes
            || self.renames
            || self.foreign_keys
            || self.reorder
            || self.databases
            || self.schemas
    }

    /// Whether `change` is one of these.
    pub fn allows(self, change: &crate::SchemaChange) -> bool {
        use crate::SchemaChange as C;
        match change {
            C::CreateTable(_) | C::DropTable { .. } => self.tables,
            C::RenameTable { .. } => self.tables && self.renames,
            C::AddColumn { .. } | C::DropColumn { .. } => self.columns,
            C::RenameColumn { .. } => self.renames,
            C::AlterColumn { .. } => self.alter_columns,
            C::CreateIndex { .. } | C::DropIndex { .. } => self.indexes,
            C::AddForeignKey { .. } | C::DropForeignKey { .. } => self.foreign_keys,
            C::MoveColumn { .. } => self.reorder,
        }
    }
}

/// How far a source sorts and filters a page's rows itself ([`crate::Page::order`] and
/// [`crate::Page::filter`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sorting {
    /// Never: only pages in the source's own order, unfiltered.
    #[default]
    None,
    /// Always (an SQL engine: `ORDER BY`, `WHERE`).
    Any,
    /// Only for a source of at most this many bytes (a file read whole to sort it); a
    /// larger one refuses ([`crate::DbError::Unsupported`]).
    UpTo(u64),
}

impl Sorting {
    /// A source of `bytes` is sorted and filtered.
    pub fn allows(self, bytes: u64) -> bool {
        match self {
            Self::None => false,
            Self::Any => true,
            Self::UpTo(max) => bytes <= max,
        }
    }
}

/// A driver's abilities (the default: none, only what [`crate::Session`] always offers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// Changes are applied all or none.
    pub transactions: bool,
    /// Rows are inserted, updated and deleted ([`crate::Session::apply`]).
    pub edit_rows: bool,
    /// Structure changed ([`crate::Session::alter`]).
    pub schema_changes: SchemaChanges,
    /// Tables are grouped in schemas.
    pub schemas: bool,
    /// The source is a file (no server, no password).
    pub file_based: bool,
    /// A running query can be interrupted.
    pub cancel: bool,
    /// An insert gives back the row's key (`RETURNING`).
    pub returning: bool,
    /// Pages sorted and filtered at the source.
    pub sorting: Sorting,
    /// What its query console can do (`None`: no console — it writes no SQL, or nothing a
    /// user could run).
    pub console: Option<ConsoleCaps>,
}

/// Which plans a console shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExplainKinds {
    /// The plan the engine would follow (`EXPLAIN`, `EXPLAIN QUERY PLAN`).
    pub plan: bool,
    /// The plan as it ran, with its real rows and times (`EXPLAIN ANALYZE`: the statement
    /// runs).
    pub analyze: bool,
}

/// How a console's default schema changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SchemaSwitch {
    /// It can't.
    #[default]
    None,
    /// By a statement on the same connection (PostgreSQL's `search_path`).
    Statement,
    /// By connecting again.
    Reconnect,
    /// The engine has no default schema to change (SQLite's attached files): only what the
    /// console's completion assumes changes.
    NamesOnly,
}

/// What a driver's query console can do (plan 39c), beyond running statements one at a
/// time with their rows streamed ([`crate::Session::run`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsoleCaps {
    /// A result's next page comes from the same cursor ([`crate::Session::fetch`]); without
    /// one, a pure query is run again from the row after.
    pub cursors: bool,
    pub explain: ExplainKinds,
    /// The isolation levels a transaction may begin with.
    pub isolation: Isolations,
    /// A transaction may begin read-only.
    pub read_only: bool,
    pub switch_schema: SchemaSwitch,
    /// It has several databases on the same server ([`crate::Session::databases`]; another
    /// one is opened by connecting again).
    pub databases: bool,
    /// Its errors say where in the statement they are.
    pub error_positions: bool,
    /// The server's messages (`RAISE NOTICE`, warnings) come with the outcome.
    pub notices: bool,
    /// An older driver (an external one of the protocol's first version): results come
    /// whole, at most so many rows, without positions — the console says so.
    pub limited: bool,
    /// What its profiles hold ([`crate::Session::profile`]; the default: what its plans
    /// give, [`ProfileCaps::from_explain`]).
    pub profile: ProfileCaps,
}

impl ConsoleCaps {
    /// What a session without any console method of its own still does: its statements
    /// through `execute`, the first page of rows, transactions by its dialect's words.
    pub const BASIC: Self = Self {
        cursors: false,
        explain: ExplainKinds { plan: false, analyze: false },
        isolation: Isolations::NONE,
        read_only: false,
        switch_schema: SchemaSwitch::None,
        databases: false,
        error_positions: false,
        notices: false,
        limited: false,
        profile: ProfileCaps::NONE,
    };

    /// An older external driver's ([`ConsoleCaps::limited`]).
    pub const LIMITED: Self = Self { limited: true, ..Self::BASIC };
}

/// What a driver's profiles hold (plan 39h2): as its server's version allows, per session
/// ([`crate::Session::console`]). A profile says what it really got in
/// [`crate::profile::Profile::collected`] (unavailable is not zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ProfileCaps {
    /// The plan without running it.
    pub estimated: bool,
    /// The statement run, measured (a write undone, [`ProfileCaps::undo_writes`]).
    pub actual: bool,
    /// Times per node (not only per statement).
    pub node_timing: bool,
    /// Blocks read, found in memory, written, per node.
    pub buffers: bool,
    /// The time spent reading and writing them.
    pub io_timing: bool,
    pub wal: bool,
    /// Sorts' and hashes' memory.
    pub memory: bool,
    pub spill: bool,
    pub parallel: bool,
    /// The planner's settings that differ from their defaults.
    pub settings: bool,
    /// The columns each step gives.
    pub verbose_output: bool,
    pub jit: bool,
    pub triggers: bool,
    /// The engine's own counters for the statement (SQLite's `sqlite3_stmt_status`).
    pub engine_counters: bool,
    /// Real rows per node counted by the engine's scans (SQLite's `scanstatus`).
    pub node_actuals_by_scan: bool,
    /// The server's history of the same query (`pg_stat_statements`).
    pub statement_history: bool,
    /// [`crate::Session::table_stats`].
    pub table_stats: bool,
    /// The driver undoes a profiled write by itself.
    pub undo_writes: bool,
}

impl ProfileCaps {
    pub const NONE: Self = Self {
        estimated: false,
        actual: false,
        node_timing: false,
        buffers: false,
        io_timing: false,
        wal: false,
        memory: false,
        spill: false,
        parallel: false,
        settings: false,
        verbose_output: false,
        jit: false,
        triggers: false,
        engine_counters: false,
        node_actuals_by_scan: false,
        statement_history: false,
        table_stats: false,
        undo_writes: false,
    };

    /// What a profile made from a driver's plans holds (the default
    /// [`crate::Session::profile`]): the plan, and as it ran when it explains so.
    pub const fn from_explain(explain: ExplainKinds) -> Self {
        Self {
            estimated: explain.plan,
            actual: explain.analyze,
            node_timing: explain.analyze,
            undo_writes: explain.analyze,
            ..Self::NONE
        }
    }

    /// It profiles at all.
    pub fn any(self) -> bool {
        self.estimated || self.actual
    }

    /// Every capability by its stable name (the protocol's, the tests'), in order.
    pub fn fields(self) -> [(&'static str, bool); 18] {
        [
            ("estimated", self.estimated),
            ("actual", self.actual),
            ("node_timing", self.node_timing),
            ("buffers", self.buffers),
            ("io_timing", self.io_timing),
            ("wal", self.wal),
            ("memory", self.memory),
            ("spill", self.spill),
            ("parallel", self.parallel),
            ("settings", self.settings),
            ("verbose_output", self.verbose_output),
            ("jit", self.jit),
            ("triggers", self.triggers),
            ("engine_counters", self.engine_counters),
            ("node_actuals_by_scan", self.node_actuals_by_scan),
            ("statement_history", self.statement_history),
            ("table_stats", self.table_stats),
            ("undo_writes", self.undo_writes),
        ]
    }

    /// Set from [`ProfileCaps::fields`]' names (one unknown is ignored).
    pub fn set(&mut self, name: &str, on: bool) {
        let field = match name {
            "estimated" => &mut self.estimated,
            "actual" => &mut self.actual,
            "node_timing" => &mut self.node_timing,
            "buffers" => &mut self.buffers,
            "io_timing" => &mut self.io_timing,
            "wal" => &mut self.wal,
            "memory" => &mut self.memory,
            "spill" => &mut self.spill,
            "parallel" => &mut self.parallel,
            "settings" => &mut self.settings,
            "verbose_output" => &mut self.verbose_output,
            "jit" => &mut self.jit,
            "triggers" => &mut self.triggers,
            "engine_counters" => &mut self.engine_counters,
            "node_actuals_by_scan" => &mut self.node_actuals_by_scan,
            "statement_history" => &mut self.statement_history,
            "table_stats" => &mut self.table_stats,
            "undo_writes" => &mut self.undo_writes,
            _ => return,
        };
        *field = on;
    }
}

/// How a transaction is isolated from the others: PostgreSQL's levels, SQLite's kinds of
/// `BEGIN` (when it takes its locks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Isolation {
    ReadCommitted,
    RepeatableRead,
    Serializable,
    /// SQLite: locks taken when first needed (its default).
    Deferred,
    /// SQLite: the write lock taken at once.
    Immediate,
    /// SQLite: no other connection reads either (outside WAL).
    Exclusive,
}

impl Isolation {
    pub const ALL: [Isolation; 6] = [
        Isolation::ReadCommitted,
        Isolation::RepeatableRead,
        Isolation::Serializable,
        Isolation::Deferred,
        Isolation::Immediate,
        Isolation::Exclusive,
    ];

    /// Stable (the protocol's, a console's saved choice): `read_committed`, `deferred`…
    pub fn id(self) -> &'static str {
        match self {
            Isolation::ReadCommitted => "read_committed",
            Isolation::RepeatableRead => "repeatable_read",
            Isolation::Serializable => "serializable",
            Isolation::Deferred => "deferred",
            Isolation::Immediate => "immediate",
            Isolation::Exclusive => "exclusive",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.id() == id)
    }

    /// Its words in a `BEGIN`: `ISOLATION LEVEL READ COMMITTED`, `IMMEDIATE`…
    pub fn sql(self) -> &'static str {
        match self {
            Isolation::ReadCommitted => "ISOLATION LEVEL READ COMMITTED",
            Isolation::RepeatableRead => "ISOLATION LEVEL REPEATABLE READ",
            Isolation::Serializable => "ISOLATION LEVEL SERIALIZABLE",
            Isolation::Deferred => "DEFERRED",
            Isolation::Immediate => "IMMEDIATE",
            Isolation::Exclusive => "EXCLUSIVE",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of [`Isolation`]s (copied freely, as the capabilities are).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Isolations(u8);

impl Isolations {
    pub const NONE: Self = Self(0);

    pub const fn of(list: &[Isolation]) -> Self {
        let mut bits = 0;
        let mut i = 0;
        while i < list.len() {
            bits |= list[i].bit();
            i += 1;
        }
        Self(bits)
    }

    pub fn contains(self, isolation: Isolation) -> bool {
        self.0 & isolation.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Its levels, in [`Isolation::ALL`]'s order.
    pub fn iter(self) -> impl Iterator<Item = Isolation> {
        Isolation::ALL.into_iter().filter(move |i| self.contains(*i))
    }
}

impl FromIterator<Isolation> for Isolations {
    fn from_iter<T: IntoIterator<Item = Isolation>>(iter: T) -> Self {
        Self(iter.into_iter().fold(0, |bits, i| bits | i.bit()))
    }
}
