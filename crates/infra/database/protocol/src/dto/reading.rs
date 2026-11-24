//! What is read: the tree's nodes, tables, structures, values, pages of rows.

use serde::{Deserialize, Serialize};

use super::{is_true, yes};

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
