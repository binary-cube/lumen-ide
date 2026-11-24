//! What is read from a database: the tree's nodes, tables, structures, values, pages.

use database as d;

use crate::dto;

/// The node kinds the editor knows (a node's icon and what it offers); another is `other`.
const NODE_KINDS: &[&str] = &[
    "database",
    "schema",
    "table",
    "view",
    "column",
    "index",
    "key",
    "function",
    "sequence",
    "collection",
    "other",
];

impl From<dto::Node> for d::Node {
    fn from(n: dto::Node) -> Self {
        let kind = NODE_KINDS.iter().find(|k| **k == n.kind).copied().unwrap_or("other");
        Self { kind, name: n.name, detail: n.detail, path: n.path, expandable: n.expandable }
    }
}

impl From<d::Node> for dto::Node {
    fn from(n: d::Node) -> Self {
        Self {
            kind: n.kind.to_owned(),
            name: n.name,
            detail: n.detail,
            path: n.path,
            expandable: n.expandable,
        }
    }
}

impl From<dto::TableRef> for d::TableRef {
    fn from(t: dto::TableRef) -> Self {
        Self { schema: t.schema, name: t.name }
    }
}

impl From<&d::TableRef> for dto::TableRef {
    fn from(t: &d::TableRef) -> Self {
        Self { schema: t.schema.clone(), name: t.name.clone() }
    }
}

impl From<d::TableRef> for dto::TableRef {
    fn from(t: d::TableRef) -> Self {
        Self { schema: t.schema, name: t.name }
    }
}

impl From<dto::TableInfo> for d::TableInfo {
    fn from(t: dto::TableInfo) -> Self {
        Self { table: t.table.into(), kind: t.kind.into(), rows: t.rows }
    }
}

impl From<d::TableInfo> for dto::TableInfo {
    fn from(t: d::TableInfo) -> Self {
        Self { table: t.table.into(), kind: t.kind.into(), rows: t.rows }
    }
}

impl From<dto::Generated> for d::Generated {
    fn from(g: dto::Generated) -> Self {
        Self { expression: g.expression, stored: g.stored }
    }
}

impl From<d::Generated> for dto::Generated {
    fn from(g: d::Generated) -> Self {
        Self { expression: g.expression, stored: g.stored }
    }
}

impl From<dto::ColumnInfo> for d::ColumnInfo {
    fn from(c: dto::ColumnInfo) -> Self {
        Self {
            class: c.class.map_or_else(|| d::TypeClass::of(&c.declared_type), Into::into),
            name: c.name,
            declared_type: c.declared_type,
            nullable: c.nullable,
            default: c.default,
            in_key: c.in_key,
            key_position: c.key_position,
            auto_increment: c.auto_increment,
            collation: c.collation,
            generated: c.generated.map(Into::into),
            values: c.values,
        }
    }
}

impl From<d::ColumnInfo> for dto::ColumnInfo {
    fn from(c: d::ColumnInfo) -> Self {
        Self {
            class: Some(c.class.into()),
            name: c.name,
            declared_type: c.declared_type,
            nullable: c.nullable,
            default: c.default,
            in_key: c.in_key,
            key_position: c.key_position,
            auto_increment: c.auto_increment,
            collation: c.collation,
            generated: c.generated.map(Into::into),
            values: c.values,
        }
    }
}

impl From<dto::IndexInfo> for d::IndexInfo {
    fn from(i: dto::IndexInfo) -> Self {
        Self {
            name: i.name,
            columns: i.columns,
            unique: i.unique,
            implicit: i.implicit,
            descending: i.descending,
            predicate: i.predicate,
        }
    }
}

impl From<d::IndexInfo> for dto::IndexInfo {
    fn from(i: d::IndexInfo) -> Self {
        Self {
            name: i.name,
            columns: i.columns,
            unique: i.unique,
            implicit: i.implicit,
            descending: i.descending,
            predicate: i.predicate,
        }
    }
}

impl From<dto::ForeignKey> for d::ForeignKey {
    fn from(k: dto::ForeignKey) -> Self {
        Self {
            name: k.name,
            columns: k.columns,
            table: k.table.into(),
            references: k.references,
            on_update: k.on_update.into(),
            on_delete: k.on_delete.into(),
        }
    }
}

impl From<d::ForeignKey> for dto::ForeignKey {
    fn from(k: d::ForeignKey) -> Self {
        Self {
            name: k.name,
            columns: k.columns,
            table: k.table.into(),
            references: k.references,
            on_update: k.on_update.into(),
            on_delete: k.on_delete.into(),
        }
    }
}

impl From<dto::Structure> for d::Structure {
    fn from(s: dto::Structure) -> Self {
        Self {
            table: s.table.into(),
            columns: s.columns.into_iter().map(Into::into).collect(),
            indexes: s.indexes.into_iter().map(Into::into).collect(),
            foreign_keys: s.foreign_keys.into_iter().map(Into::into).collect(),
            rowid: s.rowid,
            lines: s.lines,
        }
    }
}

impl From<d::Structure> for dto::Structure {
    fn from(s: d::Structure) -> Self {
        Self {
            table: s.table.into(),
            columns: s.columns.into_iter().map(Into::into).collect(),
            indexes: s.indexes.into_iter().map(Into::into).collect(),
            foreign_keys: s.foreign_keys.into_iter().map(Into::into).collect(),
            rowid: s.rowid,
            lines: s.lines,
        }
    }
}

impl From<dto::Value> for d::Value {
    fn from(v: dto::Value) -> Self {
        match v {
            dto::Value::Null => Self::Null,
            dto::Value::Bool(b) => Self::Bool(b),
            dto::Value::Int(i) => Self::Int(i),
            dto::Value::Float(f) => Self::Float(f),
            dto::Value::Text(t) => Self::Text(t),
            dto::Value::Bytes(b) => Self::Bytes(b),
            dto::Value::Json(t) => Self::Json(t),
            dto::Value::Other(t) => Self::Other(t),
        }
    }
}

impl From<d::Value> for dto::Value {
    fn from(v: d::Value) -> Self {
        match v {
            d::Value::Null => Self::Null,
            d::Value::Bool(b) => Self::Bool(b),
            d::Value::Int(i) => Self::Int(i),
            d::Value::Float(f) => Self::Float(f),
            d::Value::Text(t) => Self::Text(t),
            d::Value::Bytes(b) => Self::Bytes(b),
            d::Value::Json(t) => Self::Json(t),
            d::Value::Other(t) => Self::Other(t),
        }
    }
}

impl From<&d::Value> for dto::Value {
    fn from(v: &d::Value) -> Self {
        v.clone().into()
    }
}

pub(super) fn pairs_in(values: Vec<(String, dto::Value)>) -> Vec<(String, d::Value)> {
    values.into_iter().map(|(c, v)| (c, v.into())).collect()
}

pub(super) fn pairs_out(values: Vec<(String, d::Value)>) -> Vec<(String, dto::Value)> {
    values.into_iter().map(|(c, v)| (c, v.into())).collect()
}

impl From<dto::Order> for d::Order {
    fn from(o: dto::Order) -> Self {
        Self { column: o.column, descending: o.descending }
    }
}

impl From<d::Order> for dto::Order {
    fn from(o: d::Order) -> Self {
        Self { column: o.column, descending: o.descending }
    }
}

impl From<dto::Filter> for d::Filter {
    fn from(f: dto::Filter) -> Self {
        let all = |fs: Vec<dto::Filter>| fs.into_iter().map(Into::into).collect();
        match f {
            dto::Filter::Compare { column, op, value } => {
                Self::Compare { column, op: op.into(), value: value.into() }
            }
            dto::Filter::IsNull(c) => Self::IsNull(c),
            dto::Filter::NotNull(c) => Self::NotNull(c),
            dto::Filter::Contains { column, text } => Self::Contains { column, text },
            dto::Filter::And(fs) => Self::And(all(fs)),
            dto::Filter::Or(fs) => Self::Or(all(fs)),
            dto::Filter::Not(f) => Self::Not(Box::new((*f).into())),
        }
    }
}

impl From<d::Filter> for dto::Filter {
    fn from(f: d::Filter) -> Self {
        let all = |fs: Vec<d::Filter>| fs.into_iter().map(Into::into).collect();
        match f {
            d::Filter::Compare { column, op, value } => {
                Self::Compare { column, op: op.into(), value: value.into() }
            }
            d::Filter::IsNull(c) => Self::IsNull(c),
            d::Filter::NotNull(c) => Self::NotNull(c),
            d::Filter::Contains { column, text } => Self::Contains { column, text },
            d::Filter::And(fs) => Self::And(all(fs)),
            d::Filter::Or(fs) => Self::Or(all(fs)),
            d::Filter::Not(f) => Self::Not(Box::new((*f).into())),
        }
    }
}

impl From<dto::Page> for d::Page {
    fn from(p: dto::Page) -> Self {
        Self {
            offset: p.offset,
            limit: p.limit,
            order: p.order.into_iter().map(Into::into).collect(),
            filter: p.filter.map(Into::into),
        }
    }
}

impl From<d::Page> for dto::Page {
    fn from(p: d::Page) -> Self {
        Self {
            offset: p.offset,
            limit: p.limit,
            order: p.order.into_iter().map(Into::into).collect(),
            filter: p.filter.map(Into::into),
        }
    }
}

impl From<dto::Column> for d::Column {
    fn from(c: dto::Column) -> Self {
        Self { name: c.name, type_name: c.type_name }
    }
}

impl From<d::Column> for dto::Column {
    fn from(c: d::Column) -> Self {
        Self { name: c.name, type_name: c.type_name }
    }
}

impl From<dto::RowKey> for d::RowKey {
    fn from(k: dto::RowKey) -> Self {
        match k {
            dto::RowKey::Primary(values) => Self::Primary(pairs_in(values)),
            dto::RowKey::RowId(id) => Self::RowId(id),
            dto::RowKey::Line { line, fingerprint } => Self::Line { line, fingerprint },
        }
    }
}

impl From<d::RowKey> for dto::RowKey {
    fn from(k: d::RowKey) -> Self {
        match k {
            d::RowKey::Primary(values) => Self::Primary(pairs_out(values)),
            d::RowKey::RowId(id) => Self::RowId(id),
            d::RowKey::Line { line, fingerprint } => Self::Line { line, fingerprint },
        }
    }
}
