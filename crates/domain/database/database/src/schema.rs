//! Changes to a database's structure: tables, columns and indexes, as the model says them
//! (each dialect writes their DDL).

use crate::{ColumnInfo, ForeignKey, Generated, IndexInfo, TableRef, Value};

/// A column's default.
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    /// A value (quoted as a literal).
    Value(Value),
    /// The time of the insert.
    CurrentTimestamp,
    CurrentDate,
    CurrentTime,
    /// An SQL expression: as the engine's own catalog wrote it (a column kept by a table's
    /// rebuild), or typed by the user — written only once the dialect has checked it is one
    /// expression ([`crate::dialect::check_expression`]).
    Expression(String),
}

/// A column as it's created or changed to.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    /// The type as declared (`VARCHAR(255)`, `INTEGER`): checked by the dialect to be only
    /// a type's name and its arguments.
    pub declared_type: String,
    pub nullable: bool,
    pub default: Option<DefaultValue>,
    /// Numbered by the engine (a single-column integer primary key).
    pub auto_increment: bool,
    /// Its collation (`NOCASE`), when it has one.
    pub collation: Option<String>,
    /// Computed from the row's other columns: never written, never copied.
    pub generated: Option<Generated>,
}

impl ColumnDef {
    /// A nullable column of `declared_type`, without a default.
    pub fn new(name: impl Into<String>, declared_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            declared_type: declared_type.into(),
            nullable: true,
            default: None,
            auto_increment: false,
            collation: None,
            generated: None,
        }
    }

    /// `column` as it is (its default the catalog's expression): what a rebuild keeps.
    pub fn from_info(column: &ColumnInfo) -> Self {
        Self {
            name: column.name.clone(),
            declared_type: column.declared_type.clone(),
            nullable: column.nullable,
            default: column.default.clone().map(DefaultValue::Expression),
            auto_increment: column.auto_increment,
            collation: column.collation.clone(),
            generated: column.generated.clone(),
        }
    }

    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    pub fn default(mut self, default: DefaultValue) -> Self {
        self.default = Some(default);
        self
    }
}

/// A table as it's created: its columns and its constraints.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableDef {
    pub table: TableRef,
    pub columns: Vec<ColumnDef>,
    /// Its primary key's columns, in the key's order (none: a table without one).
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    /// `UNIQUE` constraints, each on its columns.
    pub uniques: Vec<Vec<String>>,
    /// `CHECK` constraints' expressions (checked to be one expression each).
    pub checks: Vec<String>,
    /// Its rows have no row id, only their primary key (SQLite's `WITHOUT ROWID`).
    pub without_rowid: bool,
}

impl TableDef {
    pub fn new(table: TableRef, columns: Vec<ColumnDef>) -> Self {
        Self { table, columns, ..Self::default() }
    }
}

/// A change to the structure.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaChange {
    CreateTable(TableDef),
    RenameTable {
        table: TableRef,
        to: String,
    },
    DropTable {
        table: TableRef,
    },
    AddColumn {
        table: TableRef,
        column: ColumnDef,
    },
    RenameColumn {
        table: TableRef,
        from: String,
        to: String,
    },
    /// `column` becomes `to` (its type, nullability, default; its name too, if it differs).
    AlterColumn {
        table: TableRef,
        column: String,
        to: ColumnDef,
    },
    DropColumn {
        table: TableRef,
        column: String,
    },
    CreateIndex {
        table: TableRef,
        index: IndexInfo,
    },
    DropIndex {
        table: TableRef,
        name: String,
    },
    /// A foreign key added to an existing table.
    AddForeignKey {
        table: TableRef,
        key: ForeignKey,
    },
    /// A foreign key of the table dropped (found by its columns, the table and columns it
    /// points to; by its name where the engine drops by name).
    DropForeignKey {
        table: TableRef,
        key: ForeignKey,
    },
    /// `column` moved to place `to` (0-based) among the columns.
    MoveColumn {
        table: TableRef,
        column: String,
        to: usize,
    },
}

impl SchemaChange {
    /// The table it changes.
    pub fn table(&self) -> &TableRef {
        match self {
            Self::CreateTable(def) => &def.table,
            Self::RenameTable { table, .. }
            | Self::DropTable { table }
            | Self::AddColumn { table, .. }
            | Self::RenameColumn { table, .. }
            | Self::AlterColumn { table, .. }
            | Self::DropColumn { table, .. }
            | Self::CreateIndex { table, .. }
            | Self::DropIndex { table, .. }
            | Self::AddForeignKey { table, .. }
            | Self::DropForeignKey { table, .. }
            | Self::MoveColumn { table, .. } => table,
        }
    }
}
