//! What is written: rows' changes, structure changes, scripts and rebuilds.

use serde::{Deserialize, Serialize};

use super::{ForeignKey, Generated, IndexInfo, RowKey, Structure, TableRef, Value, yes};

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
