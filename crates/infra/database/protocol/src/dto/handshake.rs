//! The handshake and the connection: versions agreed, what a driver says it can do,
//! the connection's fields and its target.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ProfileCaps, TypeClass, Versions};

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
