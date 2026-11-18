//! What a query console asks of a session beyond the explorer's `execute` (plan 39c): a
//! statement run with its first page of rows streamed and told whether more are left
//! ([`RunOptions`], [`RunOutcome`]), the rest read from the same cursor where the engine
//! keeps one ([`CursorToken`]), the server's messages ([`Notice`]), transactions begun with
//! an isolation ([`TxOptions`]), where the session is ([`Place`]) and the catalog read in
//! bulk ([`ColumnBrief`], [`Routine`]).
//!
//! Every [`crate::Session`] method using these has a default, so a driver without them still
//! works, with a poorer console: the defaults are the functions here (`*_by_execute`, through
//! [`crate::Session::execute`]), which a driver may also call for what it doesn't do itself
//! (an external driver of the protocol's first version).

use crate::sql::{self, Flavor, SplitOptions, StatementKind, TxControl};
use crate::{Cancel, Collect, DbError, Flow, Isolation, RowKey, RowSink, Session, TableRef, Value};

/// The rows a console's first page shows by default.
pub const FETCH_LIMIT: u32 = 500;

/// How a statement is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOptions {
    /// The most rows of each result set streamed to the sink; past it the outcome says
    /// [`RunOutcome::truncated`].
    pub fetch_limit: u32,
    /// Keep the rest readable from a cursor ([`RunOutcome::more`]) when the driver can.
    pub keep_cursor: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { fetch_limit: FETCH_LIMIT, keep_cursor: false }
    }
}

/// Where the rest of a result is read from ([`crate::Session::fetch`]): the driver's own
/// name for it, opaque to the console.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CursorToken(pub String);

/// A message of the server while a statement ran (PostgreSQL's `RAISE NOTICE`, a
/// warning), shown in the console's output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notice {
    /// As the engine says it: `NOTICE`, `WARNING`, `INFO`…
    pub severity: String,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunOutcome {
    /// Rows changed, when the engine says.
    pub affected: Option<u64>,
    /// Rows streamed to the sink.
    pub rows: u64,
    /// More rows were left than [`RunOptions::fetch_limit`]: they weren't read (or wait in
    /// [`RunOutcome::more`]).
    pub truncated: bool,
    /// The cursor the rest is read from (`None`: there is no rest, or no cursor kept it).
    pub more: Option<CursorToken>,
    /// The sink stopped it (or it was cancelled) before its end.
    pub stopped: bool,
    pub notices: Vec<Notice>,
}

/// How a transaction begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TxOptions {
    /// `None`: the engine's default.
    pub isolation: Option<Isolation>,
    pub read_only: bool,
}

/// Where a session is: its database and its default schema (`None`: the engine has none,
/// or doesn't say).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Place {
    pub database: Option<String>,
    pub schema: Option<String>,
}

/// A column as the console's completion knows it (read in bulk, every table at once).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnBrief {
    pub name: String,
    /// As declared (`varchar(255)`, `INTEGER`).
    pub type_name: String,
    pub nullable: bool,
    /// Its default, as the engine writes it.
    pub default: Option<String>,
    pub comment: Option<String>,
    /// Its place in the primary key (from 1).
    pub key_position: Option<u16>,
    /// The values its type allows, when the engine lists them apart from the type
    /// (a PostgreSQL enum's labels, in their order); MySQL's are in its type
    /// (`enum('S','M')`).
    pub values: Option<Vec<String>>,
}

/// A table's columns, in order.
pub type TableColumns = (TableRef, Vec<ColumnBrief>);

/// A table's foreign keys (read in bulk, every table at once).
pub type TableKeys = (TableRef, Vec<crate::ForeignKey>);

/// What kind of routine it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutineKind {
    #[default]
    Function,
    Procedure,
    Aggregate,
    Window,
}

/// A function or procedure of the server (or the engine's own).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Routine {
    pub schema: Option<String>,
    pub name: String,
    /// Its arguments as the engine writes them (`a integer, b text DEFAULT ''`); empty when
    /// it doesn't say (SQLite: only [`Routine::arity`]).
    pub arguments: String,
    /// What it returns (`integer`, `SETOF record`).
    pub result: Option<String>,
    pub kind: RoutineKind,
    /// How many arguments it takes (`None`: any number, or not said).
    pub arity: Option<u16>,
    /// The engine's own (not the user's).
    pub builtin: bool,
}

/// What happens to the rows past a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    /// The statement is stopped (a pure query: nothing is lost).
    Stop,
    /// The statement goes on to its end, the rows dropped (a write: stopping it could undo
    /// what it did).
    Drop,
}

/// A sink passing at most `limit` rows of each result set on to another, and telling
/// whether more came.
pub struct Limited<'a> {
    sink: &'a mut dyn RowSink,
    limit: u64,
    overflow: Overflow,
    in_set: u64,
    /// Rows passed on, over every set.
    pub delivered: u64,
    /// A set had more rows than the limit.
    pub truncated: bool,
    /// The sink behind it stopped.
    pub stopped: bool,
}

impl std::fmt::Debug for Limited<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Limited")
            .field("limit", &self.limit)
            .field("delivered", &self.delivered)
            .field("truncated", &self.truncated)
            .finish_non_exhaustive()
    }
}

impl<'a> Limited<'a> {
    pub fn new(sink: &'a mut dyn RowSink, limit: u32, overflow: Overflow) -> Self {
        Self {
            sink,
            limit: u64::from(limit),
            overflow,
            in_set: 0,
            delivered: 0,
            truncated: false,
            stopped: false,
        }
    }

    fn full(&self) -> bool {
        self.in_set >= self.limit
    }
}

impl RowSink for Limited<'_> {
    fn columns(&mut self, columns: &[crate::Column]) {
        self.in_set = 0;
        self.sink.columns(columns);
    }

    fn row_key(&mut self, key: RowKey) {
        if !self.full() {
            self.sink.row_key(key);
        }
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        if self.stopped {
            return Flow::Stop;
        }
        if self.full() {
            self.truncated = true;
            return match self.overflow {
                Overflow::Stop => Flow::Stop,
                Overflow::Drop => Flow::Continue,
            };
        }
        self.in_set += 1;
        self.delivered += 1;
        let flow = self.sink.row(values);
        if flow == Flow::Stop {
            self.stopped = true;
        }
        flow
    }
}

/// [`crate::Session::run`] through [`crate::Session::execute`]: the first
/// `options.fetch_limit` rows of each set, one more read to know there are more (the
/// statement stopped there), no cursor and no notices.
pub fn run_by_execute<S: Session + ?Sized>(
    session: &mut S,
    statement: &str,
    options: &RunOptions,
    sink: &mut dyn RowSink,
    cancel: &Cancel,
) -> Result<RunOutcome, DbError> {
    let mut limited = Limited::new(sink, options.fetch_limit, Overflow::Stop);
    let outcome = session.execute(statement, &mut limited, cancel)?;
    Ok(RunOutcome {
        affected: outcome.affected,
        rows: limited.delivered,
        truncated: limited.truncated,
        more: None,
        stopped: limited.stopped || (outcome.stopped && !limited.truncated),
        notices: Vec::new(),
    })
}

/// The statement beginning a transaction by `options` after the dialect's `begin`: as it is
/// with nothing asked; SQLite's kinds as `BEGIN DEFERRED`…; otherwise the standard's `START
/// TRANSACTION ISOLATION LEVEL …, READ ONLY`.
pub fn begin_statement(begin: &str, options: &TxOptions) -> String {
    let mut modes = Vec::new();
    match options.isolation {
        None => {}
        Some(kind @ (Isolation::Deferred | Isolation::Immediate | Isolation::Exclusive)) => {
            return format!("BEGIN {}", kind.sql());
        }
        Some(level) => modes.push(level.sql()),
    }
    if options.read_only {
        modes.push("READ ONLY");
    }
    if modes.is_empty() {
        begin.to_owned()
    } else {
        format!("START TRANSACTION {}", modes.join(", "))
    }
}

/// [`crate::Session::begin`] through `execute`: the dialect's `BEGIN` (an isolation or
/// read-only asked: the standard's `START TRANSACTION …`).
pub fn begin_by_execute<S: Session + ?Sized>(
    session: &mut S,
    options: &TxOptions,
) -> Result<(), DbError> {
    let begin = session.dialect().map_or("BEGIN", |d| d.begin());
    let statement = begin_statement(begin, options);
    session.execute(&statement, &mut Collect::default(), &Cancel::default()).map(|_| ())
}

/// [`crate::Session::commit`] through `execute`, by the dialect's words.
pub fn commit_by_execute<S: Session + ?Sized>(session: &mut S) -> Result<(), DbError> {
    let commit = session.dialect().map_or("COMMIT", |d| d.commit());
    session.execute(commit, &mut Collect::default(), &Cancel::default()).map(|_| ())
}

/// [`crate::Session::rollback`] through `execute`, by the dialect's words.
pub fn rollback_by_execute<S: Session + ?Sized>(session: &mut S) -> Result<(), DbError> {
    let rollback = session.dialect().map_or("ROLLBACK", |d| d.rollback());
    session.execute(rollback, &mut Collect::default(), &Cancel::default()).map(|_| ())
}

/// [`crate::Session::catalog_columns`] from what `tables` and `structure` give, a table at
/// a time (a driver without a bulk query).
pub fn catalog_columns_by_structure<S: Session + ?Sized>(
    session: &mut S,
    schema: Option<&str>,
    cancel: &Cancel,
) -> Result<Vec<TableColumns>, DbError> {
    let path: Vec<String> = schema.map(str::to_owned).into_iter().collect();
    let mut all = Vec::new();
    for table in session.tables(&path)? {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let structure = session.structure(&table.table)?;
        let columns = structure.columns.iter().map(ColumnBrief::from).collect();
        all.push((table.table, columns));
    }
    Ok(all)
}

/// [`Session::catalog_keys`]'s default: each table's structure read.
pub fn catalog_keys_by_structure<S: Session + ?Sized>(
    session: &mut S,
    schema: Option<&str>,
    cancel: &Cancel,
) -> Result<Vec<TableKeys>, DbError> {
    let path: Vec<String> = schema.map(str::to_owned).into_iter().collect();
    let mut all = Vec::new();
    for table in session.tables(&path)? {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let structure = session.structure(&table.table)?;
        if !structure.foreign_keys.is_empty() {
            all.push((table.table, structure.foreign_keys));
        }
    }
    Ok(all)
}

impl From<&crate::ColumnInfo> for ColumnBrief {
    fn from(c: &crate::ColumnInfo) -> Self {
        Self {
            name: c.name.clone(),
            type_name: c.declared_type.clone(),
            nullable: c.nullable,
            default: c.default.clone(),
            comment: None,
            key_position: c.key_position,
            values: c.values.clone(),
        }
    }
}

/// Whether a transaction is open after `text` ran (its statements in order) on a session
/// where one was `open` before: a `BEGIN` / `START` opens one, a `COMMIT` / `END` /
/// `ROLLBACK` / `ABORT` closes it (a `ROLLBACK TO` doesn't). For an engine that doesn't say
/// (PostgreSQL's client doesn't).
pub fn transaction_after(text: &str, flavor: Flavor, open: bool) -> bool {
    let parsed = sql::parse(text, flavor, &SplitOptions::default());
    parsed.statements.iter().fold(open, |open, statement| {
        match sql::classify(text, parsed.tokens_of(statement), flavor) {
            StatementKind::Transaction(TxControl::Begin) => true,
            StatementKind::Transaction(TxControl::Commit | TxControl::Rollback) => false,
            _ => open,
        }
    })
}

#[cfg(test)]
#[path = "../tests/unit/run.rs"]
mod tests;
