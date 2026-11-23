//! Databases, as a bounded context: what every engine's driver offers through one interface
//! — relational ones (PostgreSQL, MySQL / MariaDB, SQLite), and later documents (MongoDB),
//! search (Elasticsearch, Manticore) or key-value (Redis) — so the explorer, the console
//! and the results work the same with any of them.
//!
//! A driver is known by its [`DriverFactory`] (contributed by a module): its id, name and
//! kind, and a function creating it — nothing of it runs (no runtime, no thread, no TLS
//! setup) until a connection opens. A [`Session`] is one open connection: its explorer's
//! tree ([`Node`]s, as deep as the engine goes), queries executed with their rows streamed
//! to a [`RowSink`] (which can stop them), and a [`Cancel`] to interrupt one running.
//!
//! What the user configures is a [`DataSource`] (kept by a [`DataSourceStore`], its
//! password in a [`kernel::SecretStore`]). A session reads a table's [`Structure`] and its
//! rows a [`Page`] at a time, applies a [`ChangeSet`] or [`SchemaChange`]s in one
//! transaction each, as far as the driver's [`Capabilities`] go; an SQL engine's
//! [`Dialect`] writes the statements (shown before they run), with the dangerous ones
//! found first ([`danger`]). [`csv`] guesses a CSV file's dialect and types (and reads and
//! writes its records by the same rules), [`eval`] sorts and filters rows for a source that
//! writes no SQL, [`sqlite`]
//! rebuilds a table for what SQLite's `ALTER TABLE` can't do. A [`TableRepository`] is a
//! table as the grid sees it (pages, total, structure, changes), whatever is underneath;
//! [`transaction`] holds the rules every change runs by (their order, each update or
//! delete changing exactly one row, a rebuild's steps). The drivers the modules contribute,
//! and the files they open by themselves, are listed in [`drivers`]. [`sql`] reads the SQL a
//! user writes (its tokens and statements per dialect, what each does, its parameters, its
//! secrets, an engine's error position), before anything runs; a console runs it through a
//! session's [`Session::run`] and the rest of the console's contract ([`run`]: cursors,
//! transactions, the catalog in bulk; [`plan`]: a plan as a tree; [`profile`]: a statement's
//! profile, normalized for every engine), its errors with their place ([`SqlError`]).

pub mod capabilities;
pub mod changes;
pub mod completion;
pub mod console;
pub mod csv;
pub mod danger;
pub mod diagram;
pub mod dialect;
pub mod drivers;
pub mod edits;
pub mod eval;
pub mod events;
pub mod extract;
pub mod fields;
pub mod folders;
pub mod history;
pub mod identity;
pub mod layouts;
pub mod lent;
pub mod mysql;
pub mod navigation;
pub mod plan;
pub mod postgres;
pub mod profile;
pub mod relations;
pub mod repository;
pub mod run;
pub mod schema;
pub mod schema_check;
pub mod schema_index;
pub mod source;
pub mod sql;
pub mod sqlite;
pub mod structure;
pub mod tls;
pub mod transaction;

pub use capabilities::{
    Capabilities, ConsoleCaps, ExplainKinds, Isolation, Isolations, ProfileCaps, SchemaChanges,
    SchemaSwitch, Sorting,
};
pub use changes::{
    Applied, CellChange, Change, ChangeSet, CompareOp, Filter, Order, Page, RowKey, fingerprint,
};
pub use danger::{AlterColumnLoss, Danger, DropKind};
pub use dialect::{Dialect, GenericSql};
pub use drivers::DataFiles;
pub use fields::{Binding, ConnectionSpec, FieldError, FieldKind, FieldSpec, FieldTab, UrlError};
pub use folders::{FolderId, SourceColor, SourceFolder, SourceList};
pub use plan::PlanNode;
pub use repository::{Fetch, RepositoryEvent, Rows, TableRepository};
pub use run::{
    ColumnBrief, CursorToken, Notice, Place, Routine, RoutineKind, RunOptions, RunOutcome,
    TableColumns, TableKeys, TxOptions,
};
pub use schema::{ColumnDef, DefaultValue, SchemaChange, TableDef};
pub use source::{
    DataSource, DataSourceId, DataSourceStore, NoStore, Scope, SecretPolicy, Tag, recent_drivers,
    unique_name,
};
pub use structure::{
    ColumnInfo, FkAction, ForeignKey, Generated, IndexInfo, KeyKind, SchemaObject, Structure,
    TableInfo, TableKind, TableRef, TypeClass,
};
pub use tls::SslMode;
pub use transaction::{Expect, Script, Step};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// What kind of engine it is (what the explorer and the console show for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Tables, SQL, transactions.
    Relational,
    /// Collections of JSON documents.
    Document,
    /// Search indexes (JSON or SQL queries).
    Search,
    /// Keys and commands.
    KeyValue,
}

/// The language a session's console is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Sql,
    Json,
    Commands,
}

/// Makes a driver (nothing of it runs until it connects).
pub type CreateDriver = Arc<dyn Fn() -> Arc<dyn Driver> + Send + Sync>;

/// A driver, before it's created: what the app lists (a connection's type) and how to make
/// it when one opens.
#[derive(Clone)]
pub struct DriverFactory {
    /// Stable (saved with connections): `postgres`, `mysql`, `sqlite`…
    pub id: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub kind: Kind,
    /// Its default port (`None`: no server, e.g. SQLite's file).
    pub default_port: Option<u16>,
    pub create: CreateDriver,
    /// Not one of the app's: a program installed apart, speaking the drivers' protocol
    /// (listed with a mark saying so).
    pub external: bool,
    /// How its sources are shown, when it says (none: a server's or a file's icon).
    pub icon: Option<DriverIcon>,
}

/// A driver's icon: the name of a drawing in the app's icon set (the driver's module
/// contributes it), and the tone it's drawn in — one of the theme's terminal colors, so it
/// suits every theme (a theme's `icon.<name>` color comes first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverIcon {
    pub name: &'static str,
    pub tone: IconTone,
}

/// A driver icon's tone: one of the theme's terminal colors, or the muted text's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconTone {
    #[default]
    Neutral,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
}

impl DriverFactory {
    /// One of the app's drivers, made by `create`.
    pub fn new(
        id: &'static str,
        name: &'static str,
        kind: Kind,
        default_port: Option<u16>,
        create: fn() -> Arc<dyn Driver>,
    ) -> Self {
        Self {
            id: Cow::Borrowed(id),
            name: Cow::Borrowed(name),
            kind,
            default_port,
            create: Arc::new(create),
            external: false,
            icon: None,
        }
    }

    /// The same, its sources shown with icon `name` in `tone`.
    pub fn with_icon(mut self, name: &'static str, tone: IconTone) -> Self {
        self.icon = Some(DriverIcon { name, tone });
        self
    }
}

impl std::fmt::Debug for DriverFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriverFactory")
            .field("id", &self.id)
            .field("external", &self.external)
            .finish_non_exhaustive()
    }
}

/// Where a connection goes and as whom (its password is kept apart: [`Secret`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    /// The database to open (a server's), or the file (SQLite).
    pub database: String,
    /// The engine's own options (`sslmode=require`, `charset=utf8mb4`…).
    pub options: BTreeMap<String, String>,
    /// TLS to the server.
    pub tls: Tls,
}

impl Target {
    /// The option asking a session to write nothing (a read-only source): `read_only=true`.
    pub const READ_ONLY: &'static str = "read_only";
    /// The option holding statements run each time a session opens (`SET search_path …`):
    /// the app's, never the engine's.
    pub const STARTUP: &'static str = "startup_script";

    /// Option `name` as a switch (`true` / `false`, `on` / `off`, `1` / `0`); `None`: not
    /// given, or neither.
    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.options.get(name)?.trim().to_ascii_lowercase().as_str() {
            "true" | "on" | "yes" | "1" => Some(true),
            "false" | "off" | "no" | "0" => Some(false),
            _ => None,
        }
    }

    /// The session must write nothing.
    pub fn read_only(&self) -> bool {
        self.flag(Self::READ_ONLY).unwrap_or(false)
    }
}

/// TLS to the server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tls {
    /// Plain (a local server, a tunnel).
    #[default]
    Off,
    /// Encrypted, the certificate checked against the system's / Mozilla's roots.
    Verify,
    /// Encrypted, any certificate (a self-signed server the user trusts).
    Trust,
}

/// A password: never printed, wiped from memory once dropped.
pub use kernel::Secret;

/// An engine's driver: it opens sessions.
pub trait Driver: Send + Sync {
    fn kind(&self) -> Kind;
    fn language(&self) -> Language;
    /// What its sessions can do (the default: only what every one does).
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    /// How its SQL is written (`None`: it isn't SQL, or it writes none for the user).
    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        None
    }
    /// Its connection's settings, as the data sources' modal shows them (the default: a
    /// server's host, port, user, password and database; a file-based driver its file).
    fn connection(&self) -> ConnectionSpec {
        if self.capabilities().file_based {
            ConnectionSpec::file(&[], "file")
        } else {
            ConnectionSpec::server(0, &[])
        }
    }
    /// Opens a session (on the calling thread: the app calls it on a background one).
    fn connect(&self, target: &Target, secret: &Secret) -> Result<Box<dyn Session>, DbError>;
}

/// A node of a session's explorer: a database, a schema, a table, a column, an index…
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// What it is (`database`, `schema`, `table`, `view`, `column`, `index`, `key`…): its
    /// icon and what it offers.
    pub kind: &'static str,
    pub name: String,
    /// Shown after its name (a column's type, a table's row estimate).
    pub detail: String,
    /// The path to it from the root (its parents' names, then its own).
    pub path: Vec<String>,
    /// It has children (listed when opened).
    pub expandable: bool,
}

/// A result's column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// The engine's type name (`int4`, `varchar(255)`, `TEXT`…).
    pub type_name: String,
}

/// A value from a result.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    /// JSON, as the engine gave it.
    Json(String),
    /// Anything else, as text (dates, decimals, UUIDs…: shown and edited as written).
    Other(String),
}

impl Value {
    /// As shown in a grid (bytes: their size; null: empty).
    pub fn display(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Text(t) | Value::Json(t) | Value::Other(t) => t.clone(),
            Value::Bytes(b) => format!("({} bytes)", b.len()),
        }
    }
}

/// Go on with the rows, or stop (enough shown: the rest isn't read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

/// Where a query's results go, as they come.
pub trait RowSink {
    /// A result set starts (a query may give several).
    fn columns(&mut self, columns: &[Column]);
    fn row(&mut self, values: Vec<Value>) -> Flow;
    /// The next row's key, before its [`RowSink::row`] (when a table is read to be edited).
    fn row_key(&mut self, _key: RowKey) {}
}

/// How a statement ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Rows changed (an `UPDATE`, an `INSERT`), when the engine says.
    pub affected: Option<u64>,
    /// Rows read into the sink.
    pub rows: u64,
    /// It stopped early (the sink had enough, or it was cancelled).
    pub stopped: bool,
}

/// Interrupts a running query (from another thread): the session asks its server to stop.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// An open connection (used from one thread at a time).
pub trait Session: Send {
    /// The server's name and version (`PostgreSQL 16.4`).
    fn server(&self) -> String;
    /// How its SQL is written for the server it's connected to (a version's own rules);
    /// `None`: as its driver's [`Driver::dialect`] says.
    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        None
    }
    /// The explorer's nodes under `path` (`[]`: the top: databases / schemas).
    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError>;
    /// Runs `query` (one statement), its rows to `sink`; `cancel` stops it.
    fn execute(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError>;
    /// The tables and views under `path` (`[]`: the top; a schema's name for its own).
    fn tables(&mut self, path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        let _ = path;
        Err(unsupported("listing tables"))
    }
    /// `table`'s columns, indexes and foreign keys.
    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        let _ = table;
        Err(unsupported("reading a table's structure"))
    }
    /// A page of `table`'s rows to `sink` (each with its key, when it has one); `cancel`
    /// stops it.
    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let _ = (table, page, sink, cancel);
        Err(unsupported("reading a table's rows"))
    }
    /// Applies `changes` in one transaction: all, or none (a row changed since it was read
    /// is a [`DbError::Conflict`]). For a source that writes no SQL (a CSV file): an SQL
    /// one's changes run as its dialect's statements, through [`Self::execute`]
    /// ([`transaction::apply_script`]), so what runs is what the preview showed.
    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, DbError> {
        let _ = changes;
        Err(unsupported("editing rows"))
    }
    /// Makes `changes` in one transaction: all, or none. For a source that writes no SQL,
    /// as [`Self::apply`].
    fn alter(&mut self, changes: &[SchemaChange]) -> Result<Applied, DbError> {
        let _ = changes;
        Err(unsupported("changing the structure"))
    }
    /// For a source without one of the editor's dialects that writes its own statements
    /// for `changes` (an external driver's): the transaction it plans, shown before
    /// anything runs and then run exactly as shown, step by step through [`Self::execute`]
    /// ([`transaction::run`]). `None` (the default): it makes them itself through
    /// [`Self::alter`], described in words.
    fn plan_alter(&mut self, changes: &[SchemaChange]) -> Result<Option<Script>, DbError> {
        let _ = changes;
        Ok(None)
    }
    /// How many of `table`'s rows `filter` keeps (a source that writes no SQL: an SQL one is
    /// counted with its dialect's `SELECT COUNT(*)`); `cancel` stops it.
    fn count(
        &mut self,
        table: &TableRef,
        filter: Option<&Filter>,
        cancel: &Cancel,
    ) -> Result<u64, DbError> {
        let _ = (table, filter, cancel);
        Err(unsupported("counting a table's rows"))
    }
    /// What its query console can do, as the server it's connected to says (`None`: as
    /// its driver's [`Capabilities::console`] says).
    fn console(&self) -> Option<ConsoleCaps> {
        None
    }
    /// Runs `statement` (one) for a console: at most `options.fetch_limit` rows of each
    /// result set to `sink`, whether more were left, the rest kept in a cursor when asked
    /// and the driver can ([`ConsoleCaps::cursors`]), the server's notices. The default:
    /// [`Self::execute`], stopped one row past the limit ([`run::run_by_execute`]).
    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        run::run_by_execute(self, statement, options, sink, cancel)
    }
    /// The next `rows` rows of the result `cursor` holds, to `sink` (its columns first);
    /// the outcome's `more` is the cursor again while rows are left (closed once none are).
    fn fetch(
        &mut self,
        cursor: &CursorToken,
        rows: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let _ = (cursor, rows, sink, cancel);
        Err(unsupported("reading a result's next rows"))
    }
    /// Lets `cursor` go (a result's tab closed); one already gone is no error.
    fn close_cursor(&mut self, cursor: &CursorToken) -> Result<(), DbError> {
        let _ = cursor;
        Ok(())
    }
    /// `statement`'s plan (`analyze`: as it runs — the driver undoes what a write did).
    fn explain(
        &mut self,
        statement: &str,
        analyze: bool,
        cancel: &Cancel,
    ) -> Result<Vec<PlanNode>, DbError> {
        let _ = (statement, analyze, cancel);
        Err(unsupported("explaining a statement"))
    }
    /// `statement`'s profile (plan 39h2): its plan, or as it runs (`options.depth`: the
    /// driver undoes what a write did, and stops it past `options.timeout`), normalized
    /// ([`profile::Profile`]). The default: [`Self::explain`] as a profile
    /// ([`profile::by_explain`]).
    fn profile(
        &mut self,
        statement: &str,
        options: &profile::ProfileOptions,
        cancel: &Cancel,
    ) -> Result<profile::Profile, DbError> {
        profile::by_explain(self, statement, options, cancel)
    }
    /// What the server knows of `tables` (their rows, sizes, scans, indexes). The default:
    /// none ([`DbError::Unsupported`]: a table's row estimate is in [`TableInfo`] anyway).
    fn table_stats(
        &mut self,
        tables: &[TableRef],
        cancel: &Cancel,
    ) -> Result<Vec<profile::TableStats>, DbError> {
        let _ = (tables, cancel);
        Err(unsupported("reading tables' statistics"))
    }
    /// Begins a transaction (the default: its dialect's words through [`Self::execute`]).
    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        run::begin_by_execute(self, options)
    }
    fn commit(&mut self) -> Result<(), DbError> {
        run::commit_by_execute(self)
    }
    fn rollback(&mut self) -> Result<(), DbError> {
        run::rollback_by_execute(self)
    }
    /// A transaction is open (begun by [`Self::begin`] or by a statement run); the default
    /// doesn't know: `false`.
    fn in_transaction(&self) -> bool {
        false
    }
    /// Makes `schema` the one unqualified names are looked for in.
    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        let _ = schema;
        Err(unsupported("changing the schema"))
    }
    /// Its database and default schema.
    fn current(&mut self) -> Result<Place, DbError> {
        Ok(Place::default())
    }
    /// The databases on its server it can open (the default: none said).
    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(Vec::new())
    }
    /// Every table's and view's columns, of `schema` (`None`: every schema), in one go —
    /// the default asks [`Self::tables`] and each one's [`Self::structure`].
    fn catalog_columns(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableColumns>, DbError> {
        run::catalog_columns_by_structure(self, schema, cancel)
    }
    /// Every table's foreign keys, of `schema` (`None`: every schema), in one go — the
    /// default asks [`Self::tables`] and each one's [`Self::structure`]. Tables without
    /// any are left out.
    fn catalog_keys(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableKeys>, DbError> {
        run::catalog_keys_by_structure(self, schema, cancel)
    }
    /// The relations a query may read from that [`Self::tables`] doesn't list (PostgreSQL's
    /// partitions and sequences), in one go: a name the console's diagnostics must not call
    /// unknown. The default: none.
    fn catalog_relations(&mut self, cancel: &Cancel) -> Result<Vec<TableRef>, DbError> {
        let _ = cancel;
        Ok(Vec::new())
    }
    /// The functions and procedures of `schema` (`None`: every one the console may call),
    /// the engine's own included where it lists them.
    fn routines(&mut self, schema: Option<&str>, cancel: &Cancel) -> Result<Vec<Routine>, DbError> {
        let _ = (schema, cancel);
        Err(unsupported("listing routines"))
    }
    /// What rebuilding `table` needs beyond its structure (its `CHECK`s, triggers and
    /// views) and whether foreign keys are on: given by a driver whose dialect rebuilds
    /// tables ([`Dialect::needs_rebuild`]).
    fn rebuild_source(&mut self, table: &TableRef) -> Result<sqlite::RebuildSource, DbError> {
        let _ = table;
        Err(unsupported("rebuilding a table"))
    }
}

fn unsupported(what: &str) -> DbError {
    DbError::Unsupported(what.to_owned())
}

/// Why something failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    /// The server can't be reached.
    Connect(String),
    /// The user or the password were refused.
    Auth(String),
    /// The query was refused (its message, as the engine said it).
    Query(String),
    /// The query was refused, with what the engine said of it: where, its code, its
    /// detail and hint.
    Sql(Box<SqlError>),
    /// It was cancelled.
    Cancelled,
    /// The driver can't do that (an engine without it).
    Unsupported(String),
    /// A row changed or went away since it was read: nothing was applied.
    Conflict(String),
    /// The source is read-only, or the table can't be edited (no key).
    ReadOnly(String),
    /// Its file is held by another program (a spreadsheet keeping it open on Windows):
    /// nothing was changed, it can be tried again once that program lets it go.
    InUse(String),
    /// The driver itself failed: an external driver's program didn't start, speaks
    /// another version of the protocol, stopped or stopped answering. Its session is gone;
    /// the next request starts it again.
    Driver(DriverFailure),
}

/// How an external driver (a program speaking the drivers' protocol) failed. `driver` is
/// its name as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverFailure {
    /// Its program couldn't be started (not found, not allowed to run).
    NotStarted { driver: String, why: String },
    /// It speaks versions `theirs` of the protocol, the editor `ours` (inclusive ranges):
    /// none in common. `theirs` below `ours`: the driver is too old; above: the editor is.
    Protocol { driver: String, theirs: (u32, u32), ours: (u32, u32) },
    /// It stopped (it crashed, was killed) or broke the protocol: `why`, with the last
    /// line it wrote on its error output (its secrets taken out).
    Stopped { driver: String, why: String },
    /// It didn't answer `what` in time (it was stopped).
    TimedOut { driver: String, what: String },
}

impl DriverFailure {
    /// The driver is older than the editor (it has to be updated), for a protocol refusal.
    pub fn driver_too_old(&self) -> bool {
        matches!(self, Self::Protocol { theirs, ours, .. } if theirs.1 < ours.0)
    }
}

impl std::fmt::Display for DriverFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotStarted { driver, why } => {
                write!(f, "the driver {driver} didn't start: {why}")
            }
            Self::Protocol { driver, theirs, ours } => write!(
                f,
                "the driver {driver} speaks protocol {}–{}, the editor {}–{}",
                theirs.0, theirs.1, ours.0, ours.1
            ),
            Self::Stopped { driver, why } => write!(f, "the driver {driver} stopped: {why}"),
            Self::TimedOut { driver, what } => {
                write!(f, "the driver {driver} didn't answer in time ({what})")
            }
        }
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(why) => write!(f, "can't connect: {why}"),
            Self::Auth(why) => write!(f, "sign-in refused: {why}"),
            Self::Query(why) => write!(f, "{why}"),
            Self::Sql(error) => write!(f, "{error}"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Unsupported(what) => write!(f, "not supported: {what}"),
            Self::Conflict(why) => write!(f, "changed since it was read: {why}"),
            Self::ReadOnly(why) => write!(f, "read-only: {why}"),
            Self::InUse(why) => write!(f, "used by another program: {why}"),
            Self::Driver(failure) => write!(f, "{failure}"),
        }
    }
}

impl std::error::Error for DbError {}

impl DbError {
    /// An engine's refusal with what it said of it.
    pub fn sql(error: SqlError) -> Self {
        Self::Sql(Box::new(error))
    }

    /// What the engine said of a refused query, when it said more than its message.
    pub fn sql_error(&self) -> Option<&SqlError> {
        match self {
            Self::Sql(error) => Some(error),
            _ => None,
        }
    }
}

/// A statement refused by the engine, with what it said of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SqlError {
    pub message: String,
    /// The SQLSTATE code (`42P01`), when the engine has them.
    pub sqlstate: Option<String>,
    /// Where in the statement sent ([`sql::locate`] takes it back to the document).
    pub position: Option<sql::EnginePosition>,
    pub detail: Option<String>,
    pub hint: Option<String>,
}

impl SqlError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), ..Self::default() }
    }
}

/// As a refusal always read: its message, then its detail in parentheses.
impl std::fmt::Display for SqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => write!(f, "{} ({detail})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// The rows of a query kept in memory (a sink for tests and small results), up to `limit`.
#[derive(Debug, Default)]
pub struct Collect {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    /// The rows' keys, when the session gave them.
    pub keys: Vec<RowKey>,
    pub limit: Option<usize>,
}

impl RowSink for Collect {
    fn columns(&mut self, columns: &[Column]) {
        self.columns = columns.to_vec();
        self.rows.clear();
        self.keys.clear();
    }

    fn row_key(&mut self, key: RowKey) {
        self.keys.push(key);
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        self.rows.push(values);
        match self.limit {
            Some(limit) if self.rows.len() >= limit => Flow::Stop,
            _ => Flow::Continue,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
