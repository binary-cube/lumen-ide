//! The messages' shapes, as they cross the pipe (JSON; a manifest's description in TOML).
//! They mirror the domain's model, but they're the protocol's own: the domain changes
//! freely, these only with a new protocol version (a field added is optional, and ignored
//! by whoever doesn't know it). Names are `snake_case`; an enum is a string (`"null"`,
//! `"any"`) or an object of one key (`{"int": 5}`, `{"up_to": 1000}`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A range of protocol versions (both included).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    pub min: u32,
    pub max: u32,
}

impl Versions {
    pub fn contains(self, version: u32) -> bool {
        (self.min..=self.max).contains(&version)
    }

    /// The newest version both speak.
    pub fn common(self, other: Self) -> Option<u32> {
        let (low, high) = (self.min.max(other.min), self.max.min(other.max));
        (low <= high).then_some(high)
    }

    pub fn pair(self) -> (u32, u32) {
        (self.min, self.max)
    }
}

// ─── The handshake ───

/// `initialize`'s parameters: the versions the editor speaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Initialize {
    pub protocol: Versions,
    /// The editor's version (for the driver's own logs).
    #[serde(default)]
    pub editor: String,
}

/// `initialize`'s answer: the version chosen (among the editor's) and the driver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Initialized {
    pub protocol: u32,
    pub driver: Description,
}

/// What a driver is and can do: its manifest's `[driver]`, and what it says at the
/// handshake (where its capabilities are the truth its sessions are held to).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Description {
    /// Stable, saved with the data sources: lowercase letters, digits, `.`, `-`, `_`.
    pub id: String,
    pub name: String,
    /// The driver's own version (shown, never compared).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default)]
    pub kind: Kind,
    /// An icon of the editor's set (`database`, `server`, `hard-drive`, `cloud`…, see
    /// `manifest::ICONS`); none or another: the one of its kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Its icon's tone: `red`, `green`, `yellow`, `blue`, `magenta`, `cyan` (the theme's
    /// terminal colors); none or another: the muted text's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_tone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u16>,
    /// Who writes the SQL: the editor in one of its dialects, or nobody (`none`: the
    /// driver gets the changes as they are, and may plan its own statements).
    #[serde(default)]
    pub sql: Sql,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// The files it opens by themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Files>,
    /// Its connection's fields (none: a server's host, port, user, password and database,
    /// or a file-based driver's file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<Connection>,
    /// How the grid edits its own types.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_editors: Vec<ValueEditor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Relational,
    Document,
    Search,
    KeyValue,
}

/// Who writes the statements of a source's pages, counts and changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sql {
    /// Nobody: the driver gets the changes as they are (`apply`, `alter`; `plan_alter`
    /// for statements of its own) and counts by itself.
    #[default]
    None,
    /// The editor, in standard SQL (`execute`d by the driver).
    Generic,
    /// The editor, in SQLite's (a table rebuilt where `ALTER TABLE` can't: the driver
    /// answers `rebuild_source`).
    Sqlite,
    /// The editor, in PostgreSQL's (of the server's version, as `connect` says).
    Postgres,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Capabilities {
    pub transactions: bool,
    pub edit_rows: bool,
    pub schema_changes: SchemaChanges,
    pub schemas: bool,
    pub file_based: bool,
    pub cancel: bool,
    pub returning: bool,
    pub sorting: Sorting,
    /// What its query console does (protocol 2; none said: a console only as its `sql`
    /// allows, through `run`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console: Option<ConsoleCaps>,
}

/// A driver's console (protocol 2).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleCaps {
    /// `fetch` reads a result's next rows from a cursor `run` kept.
    pub cursors: bool,
    pub explain: ExplainKinds,
    /// The isolation levels `begin` takes.
    pub isolation: Vec<Isolation>,
    /// `begin` takes `read_only`.
    pub read_only: bool,
    pub switch_schema: SchemaSwitch,
    /// `databases` lists others on the same server.
    pub databases: bool,
    /// Its errors say where they are (`data.position`).
    pub error_positions: bool,
    /// `run` gives the server's notices.
    pub notices: bool,
    /// What its profiles hold (protocol 3; none said: what its plans give).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileCaps>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExplainKinds {
    pub plan: bool,
    pub analyze: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaSwitch {
    #[default]
    None,
    Statement,
    Reconnect,
    NamesOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    ReadCommitted,
    RepeatableRead,
    Serializable,
    Deferred,
    Immediate,
    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SchemaChanges {
    pub tables: bool,
    pub columns: bool,
    pub alter_columns: bool,
    pub indexes: bool,
    pub renames: bool,
    pub foreign_keys: bool,
    pub reorder: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sorting {
    #[default]
    None,
    Any,
    /// Up to so many bytes of data.
    UpTo(u64),
}

/// The files a driver opens by themselves: by extension, then by their first bytes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Files {
    pub extensions: Vec<String>,
    /// What they start with, as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// What they start with, in hexadecimal (bytes text can't say).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_hex: Option<String>,
    /// They're text: their data shown beside their editable text.
    #[serde(default)]
    pub text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Connection {
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Url {
    pub schemes: Vec<String>,
    #[serde(default)]
    pub style: UrlStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlStyle {
    #[default]
    Server,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    /// Shown as written (or one of the editor's own `datasources.field_*` texts).
    pub label: String,
    pub kind: FieldKind,
    pub binding: Binding,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub tab: FieldTab,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Number {
        min: i64,
        max: i64,
    },
    Secret,
    File {
        #[serde(default)]
        extensions: Vec<String>,
    },
    Choice {
        options: Vec<ChoiceOption>,
    },
    Flag,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    Host,
    Port,
    User,
    Database,
    File,
    Password,
    Option(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldTab {
    #[default]
    General,
    Options,
    Ssl,
    Ssh,
}

/// A value editor of the grid, as data (the editor's `ValueEditorSpec`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueEditor {
    pub id: String,
    #[serde(default)]
    pub classes: Vec<TypeClass>,
    /// Type names it's for (`uuid`, `geometry`), any case.
    #[serde(default)]
    pub types: Vec<String>,
    pub format: ValueFormat,
    #[serde(default)]
    pub large: bool,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueFormat {
    Text,
    Integer,
    Decimal,
    Bool,
    DateTime,
    Date,
    Time,
    Json,
    Choice(Vec<String>),
    Raw,
}

// ─── The connection ───

/// `connect`'s parameters: where, and the password — only ever in this message, on the
/// pipe (never printed: its `Debug` hides it).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Connect {
    pub target: Target,
    #[serde(default)]
    pub password: String,
}

impl std::fmt::Debug for Connect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connect").field("target", &self.target).finish_non_exhaustive()
    }
}

impl Drop for Connect {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.password);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Target {
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    pub database: String,
    pub options: BTreeMap<String, String>,
    pub tls: Tls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tls {
    #[default]
    Off,
    Verify,
    Trust,
}

/// `connect`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Connected {
    /// The server's name and version (`DuckDB 1.1`).
    #[serde(default)]
    pub server: String,
    /// Its version as a number, for a dialect that depends on it (PostgreSQL's
    /// `server_version_num`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

// ─── Reading ───

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PathParams {
    #[serde(default)]
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(default)]
    pub expandable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TableRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableParams {
    pub table: TableRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableInfo {
    pub table: TableRef,
    #[serde(default)]
    pub kind: TableKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    #[default]
    Table,
    View,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeClass {
    Text,
    Integer,
    Decimal,
    Bool,
    DateTime,
    Binary,
    Json,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Structure {
    pub table: TableRef,
    #[serde(default)]
    pub columns: Vec<ColumnInfo>,
    #[serde(default)]
    pub indexes: Vec<IndexInfo>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    /// Rows found by a `rowid` (SQLite's).
    #[serde(default)]
    pub rowid: bool,
    /// Rows found by their place and a fingerprint (a CSV file's).
    #[serde(default)]
    pub lines: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(default)]
    pub declared_type: String,
    /// None: the editor tells it from the declared type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<TypeClass>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default)]
    pub in_key: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_position: Option<u16>,
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<Generated>,
    /// A PostgreSQL enum's labels, in their order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generated {
    pub expression: String,
    #[serde(default)]
    pub stored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub implicit: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descending: Vec<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub table: TableRef,
    pub references: Vec<String>,
    #[serde(default)]
    pub on_update: FkAction,
    #[serde(default)]
    pub on_delete: FkAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FkAction {
    #[default]
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

/// A value: `"null"`, `{"bool": true}`, `{"int": 5}`, `{"float": 1.5}` (`"NaN"`,
/// `"Infinity"`, `"-Infinity"` as text), `{"text": "…"}`, `{"bytes": "<base64>"}`,
/// `{"json": "…"}` (as the engine wrote it), `{"other": "…"}` (a date, a decimal, a UUID,
/// as written). Integers are 64-bit, read exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(#[serde(with = "crate::wire::float")] f64),
    Text(String),
    Bytes(#[serde(with = "crate::wire::base64")] Vec<u8>),
    Json(String),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub column: String,
    #[serde(default)]
    pub descending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    Compare { column: String, op: CompareOp, value: Value },
    IsNull(String),
    NotNull(String),
    Contains { column: String, text: String },
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    #[serde(default)]
    pub offset: u64,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<Order>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
}

/// `read`'s parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Read {
    pub table: TableRef,
    pub page: Page,
}

/// `count`'s parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Count {
    pub table: TableRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
}

/// `execute`'s parameters: one statement; at most `max_rows` of its rows sent back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execute {
    pub sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rows: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(default)]
    pub type_name: String,
}

/// Rows (`read`'s, `execute`'s): the columns, the values, the rows' keys (`read`, one per
/// row when the table has a key), the rows changed (`execute`, when the engine says) and
/// whether it stopped before the end.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Rows {
    #[serde(default)]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub rows: Vec<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<RowKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    #[serde(default)]
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKey {
    /// The primary key's columns and values, `[["id", {"int": 1}]]`.
    Primary(Vec<(String, Value)>),
    RowId(i64),
    Line {
        line: u64,
        fingerprint: u64,
    },
}

// ─── Changing ───

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellChange {
    pub column: String,
    /// As it was read: a row changed since then is a conflict.
    pub old: Value,
    pub new: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Insert { values: Vec<(String, Value)> },
    Update { key: RowKey, cells: Vec<CellChange> },
    Delete { key: RowKey },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub table: TableRef,
    pub changes: Vec<Change>,
}

/// `apply`'s parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Apply {
    pub changes: ChangeSet,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Applied {
    #[serde(default)]
    pub affected: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inserted: Vec<RowKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultValue {
    Value(Value),
    CurrentTimestamp,
    CurrentDate,
    CurrentTime,
    Expression(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub declared_type: String,
    #[serde(default = "yes")]
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<DefaultValue>,
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<Generated>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableDef {
    pub table: TableRef,
    pub columns: Vec<ColumnDef>,
    #[serde(default)]
    pub primary_key: Vec<String>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    #[serde(default)]
    pub uniques: Vec<Vec<String>>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub without_rowid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaChange {
    CreateTable(TableDef),
    RenameTable { table: TableRef, to: String },
    DropTable { table: TableRef },
    AddColumn { table: TableRef, column: ColumnDef },
    RenameColumn { table: TableRef, from: String, to: String },
    AlterColumn { table: TableRef, column: String, to: ColumnDef },
    DropColumn { table: TableRef, column: String },
    CreateIndex { table: TableRef, index: IndexInfo },
    DropIndex { table: TableRef, name: String },
    AddForeignKey { table: TableRef, key: ForeignKey },
    DropForeignKey { table: TableRef, key: ForeignKey },
    MoveColumn { table: TableRef, column: String, to: u64 },
}

/// `alter`'s and `plan_alter`'s parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alter {
    pub changes: Vec<SchemaChange>,
}

/// A transaction a driver plans for structure changes (`plan_alter`): shown as it is,
/// then run by the editor statement by statement through `execute` — `begin`, the steps
/// (each checked by what it must do), `commit`, or `rollback` once one fails. An empty
/// statement (no words to begin a transaction) isn't shown nor run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Script {
    pub before: Vec<String>,
    pub begin: String,
    pub steps: Vec<Step>,
    pub commit: String,
    pub rollback: String,
    pub after: Vec<String>,
    /// What it drops and doesn't make again (confirmed by the user before it runs).
    pub stale: Vec<Stale>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub sql: String,
    #[serde(default)]
    pub expect: Expect,
}

/// What a step must do for the transaction to go on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    #[default]
    Any,
    /// Change exactly one row (otherwise: a conflict, all rolled back).
    OneRow,
    /// Return no row.
    NoRows,
    Inserted(KeyKind),
    Baseline,
    NoNewViolations {
        tables: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    Primary(Vec<String>),
    RowId,
    Line,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stale {
    pub table: TableRef,
    pub kind: ObjectKind,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Index,
    Trigger,
    View,
}

/// `rebuild_source`'s answer (an `sql = "sqlite"` driver's): what rebuilding a table needs
/// beyond its structure.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RebuildSource {
    pub structure: Structure,
    pub checks: Vec<String>,
    pub triggers: Vec<SchemaObject>,
    pub written_indexes: Vec<SchemaObject>,
    pub views: Vec<SchemaObject>,
    pub foreign_keys: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaObject {
    pub name: String,
    pub sql: String,
}

// ─── The console (protocol 2) ───

/// `run`'s parameters: one statement, at most `fetch_limit` rows of each result set sent
/// (as `$/rows` notifications), the rest kept in a cursor when asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub sql: String,
    pub fetch_limit: u32,
    #[serde(default)]
    pub keep_cursor: bool,
}

/// `fetch`'s parameters: the next `rows` rows of `cursor` (as `$/rows`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fetch {
    pub cursor: String,
    pub rows: u32,
}

/// `close_cursor`'s parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorParams {
    pub cursor: String,
}

/// `$/rows`'s parameters: rows of the request `id` as they come — `columns` when a result
/// set starts (alone, for a set without rows), then its rows, at most
/// [`crate::driver::CHUNK_ROWS`] rows or about [`crate::driver::CHUNK_BYTES`] a message.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RowsChunk {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<Column>>,
    #[serde(default)]
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Notice {
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `run`'s and `fetch`'s answer, once their rows were sent.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// Rows sent.
    pub rows: u64,
    /// More rows were left than the limit.
    pub truncated: bool,
    /// The cursor `fetch` reads the rest from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    pub stopped: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// A transaction is open after it (said by a driver that knows).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_transaction: Option<bool>,
}

/// `explain`'s parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explain {
    pub sql: String,
    #[serde(default)]
    pub analyze: bool,
}

/// A step of a plan (`explain`'s answer is a list of them, each with its children).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanNode {
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_rows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_rows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_time_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loops: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<PlanNode>,
}

/// `begin`'s parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Begin {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<Isolation>,
    pub read_only: bool,
}

/// `set_schema`'s parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaParams {
    pub schema: String,
}

/// `current`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Place {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

/// `catalog_columns`' and `routines`' parameters (no schema: every one).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnBrief {
    pub name: String,
    pub type_name: String,
    #[serde(default = "yes")]
    pub nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_position: Option<u16>,
    /// A PostgreSQL enum's labels, in their order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

/// A table's columns (`catalog_columns`' answer is a list of them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumns {
    pub table: TableRef,
    pub columns: Vec<ColumnBrief>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutineKind {
    #[default]
    Function,
    Procedure,
    Aggregate,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Routine {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub name: String,
    pub arguments: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    pub kind: RoutineKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arity: Option<u16>,
    pub builtin: bool,
}

/// An error's `data` for a refused statement (protocol 2): its SQLSTATE, its place —
/// `position` in characters from 1 (as PostgreSQL), or `offset` in bytes from 0 (as SQLite)
/// —, its detail and hint.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sqlstate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `cancel`'s parameters: the request to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelParams {
    pub id: u64,
}

// ─── Profiles (protocol 3) ───
//
// The same names as the editor's exported JSON (`database::profile::export::to_json`): a
// profile written by one reads as the other. `format` is the profile's own (separate from
// the protocol's version): a field added is optional, a larger format is read as far as it's
// understood.

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
