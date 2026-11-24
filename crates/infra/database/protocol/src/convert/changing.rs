//! What is written to a database: rows' changes and structure changes.

use database as d;

use super::reading::{pairs_in, pairs_out};
use crate::dto;

impl From<dto::CellChange> for d::CellChange {
    fn from(c: dto::CellChange) -> Self {
        Self { column: c.column, old: c.old.into(), new: c.new.into() }
    }
}

impl From<d::CellChange> for dto::CellChange {
    fn from(c: d::CellChange) -> Self {
        Self { column: c.column, old: c.old.into(), new: c.new.into() }
    }
}

impl From<dto::Change> for d::Change {
    fn from(c: dto::Change) -> Self {
        match c {
            dto::Change::Insert { values } => Self::Insert { values: pairs_in(values) },
            dto::Change::Update { key, cells } => {
                Self::Update { key: key.into(), cells: cells.into_iter().map(Into::into).collect() }
            }
            dto::Change::Delete { key } => Self::Delete { key: key.into() },
        }
    }
}

impl From<d::Change> for dto::Change {
    fn from(c: d::Change) -> Self {
        match c {
            d::Change::Insert { values } => Self::Insert { values: pairs_out(values) },
            d::Change::Update { key, cells } => {
                Self::Update { key: key.into(), cells: cells.into_iter().map(Into::into).collect() }
            }
            d::Change::Delete { key } => Self::Delete { key: key.into() },
        }
    }
}

impl From<dto::ChangeSet> for d::ChangeSet {
    fn from(s: dto::ChangeSet) -> Self {
        Self { table: s.table.into(), changes: s.changes.into_iter().map(Into::into).collect() }
    }
}

impl From<d::ChangeSet> for dto::ChangeSet {
    fn from(s: d::ChangeSet) -> Self {
        Self { table: s.table.into(), changes: s.changes.into_iter().map(Into::into).collect() }
    }
}

impl From<dto::Applied> for d::Applied {
    fn from(a: dto::Applied) -> Self {
        Self { affected: a.affected, inserted: a.inserted.into_iter().map(Into::into).collect() }
    }
}

impl From<d::Applied> for dto::Applied {
    fn from(a: d::Applied) -> Self {
        Self { affected: a.affected, inserted: a.inserted.into_iter().map(Into::into).collect() }
    }
}

impl From<dto::DefaultValue> for d::DefaultValue {
    fn from(v: dto::DefaultValue) -> Self {
        match v {
            dto::DefaultValue::Value(v) => Self::Value(v.into()),
            dto::DefaultValue::CurrentTimestamp => Self::CurrentTimestamp,
            dto::DefaultValue::CurrentDate => Self::CurrentDate,
            dto::DefaultValue::CurrentTime => Self::CurrentTime,
            dto::DefaultValue::Expression(e) => Self::Expression(e),
        }
    }
}

impl From<d::DefaultValue> for dto::DefaultValue {
    fn from(v: d::DefaultValue) -> Self {
        match v {
            d::DefaultValue::Value(v) => Self::Value(v.into()),
            d::DefaultValue::CurrentTimestamp => Self::CurrentTimestamp,
            d::DefaultValue::CurrentDate => Self::CurrentDate,
            d::DefaultValue::CurrentTime => Self::CurrentTime,
            d::DefaultValue::Expression(e) => Self::Expression(e),
        }
    }
}

impl From<dto::ColumnDef> for d::ColumnDef {
    fn from(c: dto::ColumnDef) -> Self {
        Self {
            name: c.name,
            declared_type: c.declared_type,
            nullable: c.nullable,
            default: c.default.map(Into::into),
            auto_increment: c.auto_increment,
            collation: c.collation,
            generated: c.generated.map(Into::into),
        }
    }
}

impl From<d::ColumnDef> for dto::ColumnDef {
    fn from(c: d::ColumnDef) -> Self {
        Self {
            name: c.name,
            declared_type: c.declared_type,
            nullable: c.nullable,
            default: c.default.map(Into::into),
            auto_increment: c.auto_increment,
            collation: c.collation,
            generated: c.generated.map(Into::into),
        }
    }
}

impl From<dto::TableDef> for d::TableDef {
    fn from(t: dto::TableDef) -> Self {
        Self {
            table: t.table.into(),
            columns: t.columns.into_iter().map(Into::into).collect(),
            primary_key: t.primary_key,
            foreign_keys: t.foreign_keys.into_iter().map(Into::into).collect(),
            uniques: t.uniques,
            checks: t.checks,
            without_rowid: t.without_rowid,
        }
    }
}

impl From<d::TableDef> for dto::TableDef {
    fn from(t: d::TableDef) -> Self {
        Self {
            table: t.table.into(),
            columns: t.columns.into_iter().map(Into::into).collect(),
            primary_key: t.primary_key,
            foreign_keys: t.foreign_keys.into_iter().map(Into::into).collect(),
            uniques: t.uniques,
            checks: t.checks,
            without_rowid: t.without_rowid,
        }
    }
}

impl From<dto::SchemaChange> for d::SchemaChange {
    fn from(c: dto::SchemaChange) -> Self {
        use dto::SchemaChange as C;
        match c {
            C::CreateTable(def) => Self::CreateTable(def.into()),
            C::RenameTable { table, to } => Self::RenameTable { table: table.into(), to },
            C::DropTable { table } => Self::DropTable { table: table.into() },
            C::AddColumn { table, column } => {
                Self::AddColumn { table: table.into(), column: column.into() }
            }
            C::RenameColumn { table, from, to } => {
                Self::RenameColumn { table: table.into(), from, to }
            }
            C::AlterColumn { table, column, to } => {
                Self::AlterColumn { table: table.into(), column, to: to.into() }
            }
            C::DropColumn { table, column } => Self::DropColumn { table: table.into(), column },
            C::CreateIndex { table, index } => {
                Self::CreateIndex { table: table.into(), index: index.into() }
            }
            C::DropIndex { table, name } => Self::DropIndex { table: table.into(), name },
            C::AddForeignKey { table, key } => {
                Self::AddForeignKey { table: table.into(), key: key.into() }
            }
            C::DropForeignKey { table, key } => {
                Self::DropForeignKey { table: table.into(), key: key.into() }
            }
            C::MoveColumn { table, column, to } => Self::MoveColumn {
                table: table.into(),
                column,
                to: usize::try_from(to).unwrap_or(usize::MAX),
            },
        }
    }
}

impl From<d::SchemaChange> for dto::SchemaChange {
    fn from(c: d::SchemaChange) -> Self {
        use d::SchemaChange as C;
        match c {
            C::CreateTable(def) => Self::CreateTable(def.into()),
            C::RenameTable { table, to } => Self::RenameTable { table: table.into(), to },
            C::DropTable { table } => Self::DropTable { table: table.into() },
            C::AddColumn { table, column } => {
                Self::AddColumn { table: table.into(), column: column.into() }
            }
            C::RenameColumn { table, from, to } => {
                Self::RenameColumn { table: table.into(), from, to }
            }
            C::AlterColumn { table, column, to } => {
                Self::AlterColumn { table: table.into(), column, to: to.into() }
            }
            C::DropColumn { table, column } => Self::DropColumn { table: table.into(), column },
            C::CreateIndex { table, index } => {
                Self::CreateIndex { table: table.into(), index: index.into() }
            }
            C::DropIndex { table, name } => Self::DropIndex { table: table.into(), name },
            C::AddForeignKey { table, key } => {
                Self::AddForeignKey { table: table.into(), key: key.into() }
            }
            C::DropForeignKey { table, key } => {
                Self::DropForeignKey { table: table.into(), key: key.into() }
            }
            C::MoveColumn { table, column, to } => {
                Self::MoveColumn { table: table.into(), column, to: to as u64 }
            }
        }
    }
}
