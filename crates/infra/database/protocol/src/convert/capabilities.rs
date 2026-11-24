//! What a driver can do and what a console runs: capabilities, outcomes, plans,
//! transactions, routines and errors.

use database as d;

use crate::dto;

impl From<dto::SchemaChanges> for d::SchemaChanges {
    fn from(c: dto::SchemaChanges) -> Self {
        Self {
            tables: c.tables,
            columns: c.columns,
            alter_columns: c.alter_columns,
            indexes: c.indexes,
            renames: c.renames,
            foreign_keys: c.foreign_keys,
            reorder: c.reorder,
        }
    }
}

impl From<d::SchemaChanges> for dto::SchemaChanges {
    fn from(c: d::SchemaChanges) -> Self {
        Self {
            tables: c.tables,
            columns: c.columns,
            alter_columns: c.alter_columns,
            indexes: c.indexes,
            renames: c.renames,
            foreign_keys: c.foreign_keys,
            reorder: c.reorder,
        }
    }
}

impl From<dto::Sorting> for d::Sorting {
    fn from(s: dto::Sorting) -> Self {
        match s {
            dto::Sorting::None => Self::None,
            dto::Sorting::Any => Self::Any,
            dto::Sorting::UpTo(n) => Self::UpTo(n),
        }
    }
}

impl From<d::Sorting> for dto::Sorting {
    fn from(s: d::Sorting) -> Self {
        match s {
            d::Sorting::None => Self::None,
            d::Sorting::Any => Self::Any,
            d::Sorting::UpTo(n) => Self::UpTo(n),
        }
    }
}

impl From<dto::Capabilities> for d::Capabilities {
    fn from(c: dto::Capabilities) -> Self {
        Self {
            transactions: c.transactions,
            edit_rows: c.edit_rows,
            schema_changes: c.schema_changes.into(),
            schemas: c.schemas,
            file_based: c.file_based,
            cancel: c.cancel,
            returning: c.returning,
            sorting: c.sorting.into(),
            console: c.console.map(Into::into),
        }
    }
}

impl From<d::Capabilities> for dto::Capabilities {
    fn from(c: d::Capabilities) -> Self {
        Self {
            transactions: c.transactions,
            edit_rows: c.edit_rows,
            schema_changes: c.schema_changes.into(),
            schemas: c.schemas,
            file_based: c.file_based,
            cancel: c.cancel,
            returning: c.returning,
            sorting: c.sorting.into(),
            console: c.console.map(Into::into),
        }
    }
}

/// A console as the driver says it (never `limited`: that's the editor's to tell, from the
/// protocol's version).
impl From<dto::ConsoleCaps> for d::ConsoleCaps {
    fn from(c: dto::ConsoleCaps) -> Self {
        Self {
            cursors: c.cursors,
            explain: d::ExplainKinds { plan: c.explain.plan, analyze: c.explain.analyze },
            isolation: c.isolation.into_iter().map(d::Isolation::from).collect(),
            read_only: c.read_only,
            switch_schema: c.switch_schema.into(),
            databases: c.databases,
            error_positions: c.error_positions,
            notices: c.notices,
            limited: false,
            profile: c.profile.map(d::ProfileCaps::from).unwrap_or_else(|| {
                d::ProfileCaps::from_explain(d::ExplainKinds {
                    plan: c.explain.plan,
                    analyze: c.explain.analyze,
                })
            }),
        }
    }
}

impl From<d::ConsoleCaps> for dto::ConsoleCaps {
    fn from(c: d::ConsoleCaps) -> Self {
        Self {
            cursors: c.cursors,
            explain: dto::ExplainKinds { plan: c.explain.plan, analyze: c.explain.analyze },
            isolation: c.isolation.iter().map(Into::into).collect(),
            read_only: c.read_only,
            switch_schema: c.switch_schema.into(),
            databases: c.databases,
            error_positions: c.error_positions,
            notices: c.notices,
            profile: Some(c.profile.into()),
        }
    }
}

// ─── The console ───

impl From<dto::Notice> for d::Notice {
    fn from(n: dto::Notice) -> Self {
        Self { severity: n.severity, message: n.message, detail: n.detail, hint: n.hint }
    }
}

impl From<d::Notice> for dto::Notice {
    fn from(n: d::Notice) -> Self {
        Self { severity: n.severity, message: n.message, detail: n.detail, hint: n.hint }
    }
}

impl From<dto::RunOutcome> for d::RunOutcome {
    fn from(o: dto::RunOutcome) -> Self {
        Self {
            affected: o.affected,
            rows: o.rows,
            truncated: o.truncated,
            more: o.more.map(d::CursorToken),
            stopped: o.stopped,
            notices: o.notices.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<d::RunOutcome> for dto::RunOutcome {
    fn from(o: d::RunOutcome) -> Self {
        Self {
            affected: o.affected,
            rows: o.rows,
            truncated: o.truncated,
            more: o.more.map(|c| c.0),
            stopped: o.stopped,
            notices: o.notices.into_iter().map(Into::into).collect(),
            in_transaction: None,
        }
    }
}

impl From<dto::PlanNode> for d::PlanNode {
    fn from(n: dto::PlanNode) -> Self {
        Self {
            operation: n.operation,
            object: n.object,
            detail: n.detail,
            startup_cost: n.startup_cost,
            total_cost: n.total_cost,
            plan_rows: n.plan_rows,
            actual_rows: n.actual_rows,
            actual_time_ms: n.actual_time_ms,
            loops: n.loops,
            children: n.children.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<d::PlanNode> for dto::PlanNode {
    fn from(n: d::PlanNode) -> Self {
        Self {
            operation: n.operation,
            object: n.object,
            detail: n.detail,
            startup_cost: n.startup_cost,
            total_cost: n.total_cost,
            plan_rows: n.plan_rows,
            actual_rows: n.actual_rows,
            actual_time_ms: n.actual_time_ms,
            loops: n.loops,
            children: n.children.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<dto::Begin> for d::TxOptions {
    fn from(b: dto::Begin) -> Self {
        Self { isolation: b.isolation.map(Into::into), read_only: b.read_only }
    }
}

impl From<&d::TxOptions> for dto::Begin {
    fn from(o: &d::TxOptions) -> Self {
        Self { isolation: o.isolation.map(Into::into), read_only: o.read_only }
    }
}

impl From<dto::Place> for d::Place {
    fn from(p: dto::Place) -> Self {
        Self { database: p.database, schema: p.schema }
    }
}

impl From<d::Place> for dto::Place {
    fn from(p: d::Place) -> Self {
        Self { database: p.database, schema: p.schema }
    }
}

impl From<dto::ColumnBrief> for d::ColumnBrief {
    fn from(c: dto::ColumnBrief) -> Self {
        Self {
            name: c.name,
            type_name: c.type_name,
            nullable: c.nullable,
            default: c.default,
            comment: c.comment,
            key_position: c.key_position,
            values: c.values,
        }
    }
}

impl From<d::ColumnBrief> for dto::ColumnBrief {
    fn from(c: d::ColumnBrief) -> Self {
        Self {
            name: c.name,
            type_name: c.type_name,
            nullable: c.nullable,
            default: c.default,
            comment: c.comment,
            key_position: c.key_position,
            values: c.values,
        }
    }
}

impl From<dto::TableColumns> for d::TableColumns {
    fn from(t: dto::TableColumns) -> Self {
        (t.table.into(), t.columns.into_iter().map(Into::into).collect())
    }
}

impl From<d::TableColumns> for dto::TableColumns {
    fn from((table, columns): d::TableColumns) -> Self {
        Self { table: table.into(), columns: columns.into_iter().map(Into::into).collect() }
    }
}

impl From<dto::Routine> for d::Routine {
    fn from(r: dto::Routine) -> Self {
        Self {
            schema: r.schema,
            name: r.name,
            arguments: r.arguments,
            result: r.result,
            kind: r.kind.into(),
            arity: r.arity,
            builtin: r.builtin,
        }
    }
}

impl From<d::Routine> for dto::Routine {
    fn from(r: d::Routine) -> Self {
        Self {
            schema: r.schema,
            name: r.name,
            arguments: r.arguments,
            result: r.result,
            kind: r.kind.into(),
            arity: r.arity,
            builtin: r.builtin,
        }
    }
}

/// A refused statement's details as an error's `data`.
impl From<&d::SqlError> for dto::ErrorData {
    fn from(e: &d::SqlError) -> Self {
        let (position, offset) = match e.position {
            Some(d::sql::EnginePosition::Character(n)) => (Some(n as u64), None),
            Some(d::sql::EnginePosition::Byte(n)) => (None, Some(n as u64)),
            None => (None, None),
        };
        Self {
            sqlstate: e.sqlstate.clone(),
            position,
            offset,
            detail: e.detail.clone(),
            hint: e.hint.clone(),
        }
    }
}

impl dto::ErrorData {
    /// The domain's error for `message` with these details.
    pub fn to_sql(&self, message: String) -> d::SqlError {
        let position = match (self.position, self.offset) {
            (Some(n), _) => {
                usize::try_from(n).ok().filter(|n| *n > 0).map(d::sql::EnginePosition::Character)
            }
            (None, Some(n)) => usize::try_from(n).ok().map(d::sql::EnginePosition::Byte),
            (None, None) => None,
        };
        d::SqlError {
            message,
            sqlstate: self.sqlstate.clone(),
            position,
            detail: self.detail.clone(),
            hint: self.hint.clone(),
        }
    }
}
