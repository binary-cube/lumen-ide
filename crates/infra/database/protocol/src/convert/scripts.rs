//! Scripts run as a whole and the sources of a table's rebuild.

use database as d;

use crate::dto;

impl From<dto::KeyKind> for d::KeyKind {
    fn from(k: dto::KeyKind) -> Self {
        match k {
            dto::KeyKind::Primary(columns) => Self::Primary(columns),
            dto::KeyKind::RowId => Self::RowId,
            dto::KeyKind::Line => Self::Line,
            dto::KeyKind::None => Self::None,
        }
    }
}

impl From<d::KeyKind> for dto::KeyKind {
    fn from(k: d::KeyKind) -> Self {
        match k {
            d::KeyKind::Primary(columns) => Self::Primary(columns),
            d::KeyKind::RowId => Self::RowId,
            d::KeyKind::Line => Self::Line,
            d::KeyKind::None => Self::None,
        }
    }
}

impl From<dto::Expect> for d::Expect {
    fn from(e: dto::Expect) -> Self {
        match e {
            dto::Expect::Any => Self::Any,
            dto::Expect::OneRow => Self::OneRow,
            dto::Expect::NoRows => Self::NoRows,
            dto::Expect::Inserted(key) => Self::Inserted(key.into()),
            dto::Expect::Baseline => Self::Baseline,
            dto::Expect::NoNewViolations { tables } => Self::NoNewViolations { tables },
        }
    }
}

impl From<d::Expect> for dto::Expect {
    fn from(e: d::Expect) -> Self {
        match e {
            d::Expect::Any => Self::Any,
            d::Expect::OneRow => Self::OneRow,
            d::Expect::NoRows => Self::NoRows,
            d::Expect::Inserted(key) => Self::Inserted(key.into()),
            d::Expect::Baseline => Self::Baseline,
            d::Expect::NoNewViolations { tables } => Self::NoNewViolations { tables },
        }
    }
}

impl From<dto::Script> for d::Script {
    fn from(s: dto::Script) -> Self {
        Self {
            before: s.before,
            begin: s.begin,
            steps: s
                .steps
                .into_iter()
                .map(|step| d::Step { sql: step.sql, expect: step.expect.into() })
                .collect(),
            commit: s.commit,
            rollback: s.rollback,
            after: s.after,
            stale: s
                .stale
                .into_iter()
                .map(|x| d::sqlite::Stale {
                    table: x.table.into(),
                    kind: x.kind.into(),
                    name: x.name,
                })
                .collect(),
        }
    }
}

impl From<d::Script> for dto::Script {
    fn from(s: d::Script) -> Self {
        Self {
            before: s.before,
            begin: s.begin,
            steps: s
                .steps
                .into_iter()
                .map(|step| dto::Step { sql: step.sql, expect: step.expect.into() })
                .collect(),
            commit: s.commit,
            rollback: s.rollback,
            after: s.after,
            stale: s
                .stale
                .into_iter()
                .map(|x| dto::Stale { table: x.table.into(), kind: x.kind.into(), name: x.name })
                .collect(),
        }
    }
}

fn objects_in(objects: Vec<dto::SchemaObject>) -> Vec<d::SchemaObject> {
    objects.into_iter().map(|o| d::SchemaObject { name: o.name, sql: o.sql }).collect()
}

fn objects_out(objects: Vec<d::SchemaObject>) -> Vec<dto::SchemaObject> {
    objects.into_iter().map(|o| dto::SchemaObject { name: o.name, sql: o.sql }).collect()
}

impl From<dto::RebuildSource> for d::sqlite::RebuildSource {
    fn from(r: dto::RebuildSource) -> Self {
        Self {
            table: d::sqlite::SqliteTable {
                structure: r.structure.into(),
                checks: r.checks,
                triggers: objects_in(r.triggers),
                written_indexes: objects_in(r.written_indexes),
                views: objects_in(r.views),
            },
            foreign_keys: r.foreign_keys,
        }
    }
}

impl From<d::sqlite::RebuildSource> for dto::RebuildSource {
    fn from(r: d::sqlite::RebuildSource) -> Self {
        Self {
            structure: r.table.structure.into(),
            checks: r.table.checks,
            triggers: objects_out(r.table.triggers),
            written_indexes: objects_out(r.table.written_indexes),
            views: objects_out(r.table.views),
            foreign_keys: r.foreign_keys,
        }
    }
}
