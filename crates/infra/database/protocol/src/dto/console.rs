//! The console (protocol 2): statements run, their outcomes, plans, transactions, cancel.

use serde::{Deserialize, Serialize};

use super::{Column, Isolation, TableRef, Value, yes};

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
