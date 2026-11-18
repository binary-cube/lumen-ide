//! The messages to and from the domain's model, both ways (the editor's side and a Rust
//! driver's side use the same).

use database as d;

use crate::dto;

macro_rules! same_enum {
    ($from:ty => $to:ty { $($variant:ident),+ $(,)? }) => {
        impl From<$from> for $to {
            fn from(value: $from) -> Self {
                match value { $(<$from>::$variant => <$to>::$variant),+ }
            }
        }
    };
}

same_enum!(dto::Kind => d::Kind { Relational, Document, Search, KeyValue });
same_enum!(d::Kind => dto::Kind { Relational, Document, Search, KeyValue });
same_enum!(dto::Tls => d::Tls { Off, Verify, Trust });
same_enum!(d::Tls => dto::Tls { Off, Verify, Trust });
same_enum!(dto::TableKind => d::TableKind { Table, View });
same_enum!(d::TableKind => dto::TableKind { Table, View });
same_enum!(dto::TypeClass => d::TypeClass {
    Text, Integer, Decimal, Bool, DateTime, Binary, Json, Other
});
same_enum!(d::TypeClass => dto::TypeClass {
    Text, Integer, Decimal, Bool, DateTime, Binary, Json, Other
});
same_enum!(dto::FkAction => d::FkAction { NoAction, Restrict, Cascade, SetNull, SetDefault });
same_enum!(d::FkAction => dto::FkAction { NoAction, Restrict, Cascade, SetNull, SetDefault });
same_enum!(dto::CompareOp => d::CompareOp { Eq, NotEq, Less, LessEq, Greater, GreaterEq });
same_enum!(d::CompareOp => dto::CompareOp { Eq, NotEq, Less, LessEq, Greater, GreaterEq });
same_enum!(dto::FieldTab => d::FieldTab { General, Options, Ssl, Ssh });
same_enum!(d::FieldTab => dto::FieldTab { General, Options, Ssl, Ssh });
same_enum!(dto::UrlStyle => d::fields::UrlStyle { Server, File });
same_enum!(d::fields::UrlStyle => dto::UrlStyle { Server, File });
same_enum!(dto::ObjectKind => d::sqlite::ObjectKind { Index, Trigger, View });
same_enum!(d::sqlite::ObjectKind => dto::ObjectKind { Index, Trigger, View });
same_enum!(dto::Isolation => d::Isolation {
    ReadCommitted, RepeatableRead, Serializable, Deferred, Immediate, Exclusive
});
same_enum!(d::Isolation => dto::Isolation {
    ReadCommitted, RepeatableRead, Serializable, Deferred, Immediate, Exclusive
});
same_enum!(dto::SchemaSwitch => d::SchemaSwitch { None, Statement, Reconnect, NamesOnly });
same_enum!(d::SchemaSwitch => dto::SchemaSwitch { None, Statement, Reconnect, NamesOnly });
same_enum!(dto::RoutineKind => d::RoutineKind { Function, Procedure, Aggregate, Window });
same_enum!(d::RoutineKind => dto::RoutineKind { Function, Procedure, Aggregate, Window });

/// The language a kind's console is written in.
pub fn language(kind: dto::Kind) -> d::Language {
    match kind {
        dto::Kind::Relational => d::Language::Sql,
        dto::Kind::Document | dto::Kind::Search => d::Language::Json,
        dto::Kind::KeyValue => d::Language::Commands,
    }
}

// ─── Capabilities ───

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

// ─── The connection's fields ───

impl From<dto::FieldKind> for d::FieldKind {
    fn from(k: dto::FieldKind) -> Self {
        match k {
            dto::FieldKind::Text => Self::Text,
            dto::FieldKind::Number { min, max } => Self::Number { min, max },
            dto::FieldKind::Secret => Self::Secret,
            dto::FieldKind::File { extensions } => Self::File { extensions },
            dto::FieldKind::Choice { options } => {
                Self::Choice(options.into_iter().map(|o| (o.value, o.label)).collect())
            }
            dto::FieldKind::Flag => Self::Flag,
        }
    }
}

impl From<d::FieldKind> for dto::FieldKind {
    fn from(k: d::FieldKind) -> Self {
        match k {
            d::FieldKind::Text => Self::Text,
            d::FieldKind::Number { min, max } => Self::Number { min, max },
            d::FieldKind::Secret => Self::Secret,
            d::FieldKind::File { extensions } => Self::File { extensions },
            d::FieldKind::Choice(options) => Self::Choice {
                options: options
                    .into_iter()
                    .map(|(value, label)| dto::ChoiceOption { value, label })
                    .collect(),
            },
            d::FieldKind::Flag => Self::Flag,
        }
    }
}

impl From<dto::Binding> for d::Binding {
    fn from(b: dto::Binding) -> Self {
        match b {
            dto::Binding::Host => Self::Host,
            dto::Binding::Port => Self::Port,
            dto::Binding::User => Self::User,
            dto::Binding::Database => Self::Database,
            dto::Binding::File => Self::File,
            dto::Binding::Password => Self::Password,
            dto::Binding::Option(name) => Self::Option(name),
        }
    }
}

impl From<d::Binding> for dto::Binding {
    fn from(b: d::Binding) -> Self {
        match b {
            d::Binding::Host => Self::Host,
            d::Binding::Port => Self::Port,
            d::Binding::User => Self::User,
            d::Binding::Database => Self::Database,
            d::Binding::File => Self::File,
            d::Binding::Password => Self::Password,
            d::Binding::Option(name) => Self::Option(name),
        }
    }
}

impl From<dto::Field> for d::FieldSpec {
    fn from(f: dto::Field) -> Self {
        Self {
            id: f.id,
            label: f.label,
            kind: f.kind.into(),
            binding: f.binding.into(),
            default: f.default,
            required: f.required,
            tab: f.tab.into(),
        }
    }
}

impl From<d::FieldSpec> for dto::Field {
    fn from(f: d::FieldSpec) -> Self {
        Self {
            id: f.id,
            label: f.label,
            kind: f.kind.into(),
            binding: f.binding.into(),
            default: f.default,
            required: f.required,
            tab: f.tab.into(),
        }
    }
}

impl From<dto::Connection> for d::ConnectionSpec {
    fn from(c: dto::Connection) -> Self {
        Self {
            fields: c.fields.into_iter().map(Into::into).collect(),
            url: c.url.map(|u| d::fields::UrlSpec { schemes: u.schemes, style: u.style.into() }),
        }
    }
}

impl From<d::ConnectionSpec> for dto::Connection {
    fn from(c: d::ConnectionSpec) -> Self {
        Self {
            fields: c.fields.into_iter().map(Into::into).collect(),
            url: c.url.map(|u| dto::Url { schemes: u.schemes, style: u.style.into() }),
        }
    }
}

// ─── Targets ───

impl From<&d::Target> for dto::Target {
    fn from(t: &d::Target) -> Self {
        Self {
            host: t.host.clone(),
            port: t.port,
            user: t.user.clone(),
            database: t.database.clone(),
            options: t.options.clone(),
            tls: t.tls.into(),
        }
    }
}

impl From<dto::Target> for d::Target {
    fn from(t: dto::Target) -> Self {
        Self {
            host: t.host,
            port: t.port,
            user: t.user,
            database: t.database,
            options: t.options,
            tls: t.tls.into(),
        }
    }
}

// ─── Reading ───

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

fn pairs_in(values: Vec<(String, dto::Value)>) -> Vec<(String, d::Value)> {
    values.into_iter().map(|(c, v)| (c, v.into())).collect()
}

fn pairs_out(values: Vec<(String, d::Value)>) -> Vec<(String, dto::Value)> {
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

// ─── Changing ───

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

// ─── Scripts and rebuilds ───

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

#[cfg(test)]
#[path = "../tests/unit/convert.rs"]
mod tests;

// ─── Profiles (protocol 3) ───

macro_rules! same_fields {
    ($from:ty => $to:ty { $($field:ident),+ $(,)? }) => {
        impl From<$from> for $to {
            fn from(v: $from) -> Self {
                Self { $($field: v.$field),+ }
            }
        }
    };
}

same_fields!(d::ProfileCaps => dto::ProfileCaps {
    estimated, actual, node_timing, buffers, io_timing, wal, memory, spill, parallel, settings,
    verbose_output, jit, triggers, engine_counters, node_actuals_by_scan, statement_history,
    table_stats, undo_writes
});
same_fields!(dto::ProfileCaps => d::ProfileCaps {
    estimated, actual, node_timing, buffers, io_timing, wal, memory, spill, parallel, settings,
    verbose_output, jit, triggers, engine_counters, node_actuals_by_scan, statement_history,
    table_stats, undo_writes
});

use d::profile as p;

same_fields!(p::Io => dto::Io {
    shared_hit, shared_read, shared_dirtied, shared_written, local_hit, local_read,
    local_dirtied, local_written, temp_read, temp_written, read_ms, write_ms, local_read_ms,
    local_write_ms, temp_read_ms, temp_write_ms, block_size
});
same_fields!(dto::Io => p::Io {
    shared_hit, shared_read, shared_dirtied, shared_written, local_hit, local_read,
    local_dirtied, local_written, temp_read, temp_written, read_ms, write_ms, local_read_ms,
    local_write_ms, temp_read_ms, temp_write_ms, block_size
});
same_fields!(p::HashStats => dto::HashStats { buckets, original_buckets, batches, original_batches });
same_fields!(dto::HashStats => p::HashStats { buckets, original_buckets, batches, original_batches });
same_fields!(p::CacheStats => dto::CacheStats { hits, misses, evictions, overflows });
same_fields!(dto::CacheStats => p::CacheStats { hits, misses, evictions, overflows });
same_fields!(p::Wal => dto::Wal { records, full_page_images, bytes, buffers_full });
same_fields!(dto::Wal => p::Wal { records, full_page_images, bytes, buffers_full });
same_fields!(p::NodeFlags => dto::Flags { disabled, async_capable, partial });
same_fields!(dto::Flags => p::NodeFlags { disabled, async_capable, partial });
same_fields!(p::PlanningMemory => dto::PlanningMemory { used_bytes, allocated_bytes });
same_fields!(dto::PlanningMemory => p::PlanningMemory { used_bytes, allocated_bytes });
same_fields!(p::Serialize => dto::Serialization { ms, bytes, format });
same_fields!(dto::Serialization => p::Serialize { ms, bytes, format });
same_fields!(p::Setting => dto::Setting { name, value });
same_fields!(dto::Setting => p::Setting { name, value });
same_fields!(p::Wait => dto::Wait { event, ms, count });
same_fields!(dto::Wait => p::Wait { event, ms, count });
same_fields!(p::TriggerTime => dto::Trigger { name, relation, calls, ms });
same_fields!(dto::Trigger => p::TriggerTime { name, relation, calls, ms });
same_fields!(p::Jit => dto::Jit {
    functions, generation_ms, inlining_ms, optimization_ms, emission_ms, deform_ms, total_ms
});
same_fields!(dto::Jit => p::Jit {
    functions, generation_ms, inlining_ms, optimization_ms, emission_ms, deform_ms, total_ms
});
same_fields!(p::EngineCounters => dto::Counters {
    fullscan_steps, sorts, autoindex, vm_steps, reprepare, runs, filter_hits, filter_misses,
    mem_used, rows_examined, select_scan, sort_merge_passes, tmp_disk_tables
});
same_fields!(dto::Counters => p::EngineCounters {
    fullscan_steps, sorts, autoindex, vm_steps, reprepare, runs, filter_hits, filter_misses,
    mem_used, rows_examined, select_scan, sort_merge_passes, tmp_disk_tables
});
same_fields!(p::StatementStats => dto::History {
    calls, mean_ms, stddev_ms, min_ms, max_ms, rows, hit_ratio, temp_blocks, wal_bytes
});
same_fields!(dto::History => p::StatementStats {
    calls, mean_ms, stddev_ms, min_ms, max_ms, rows, hit_ratio, temp_blocks, wal_bytes
});
same_fields!(p::Estimates => dto::Estimates { startup_cost, total_cost, rows, width });
same_fields!(dto::Estimates => p::Estimates { startup_cost, total_cost, rows, width });
same_fields!(p::Actuals => dto::Actuals {
    rows, loops, startup_ms, total_ms, never_executed, rows_removed_by_filter,
    rows_removed_by_join_filter, rows_removed_by_recheck, heap_fetches, index_searches,
    exact_blocks, lossy_blocks
});
same_fields!(dto::Actuals => p::Actuals {
    rows, loops, startup_ms, total_ms, never_executed, rows_removed_by_filter,
    rows_removed_by_join_filter, rows_removed_by_recheck, heap_fetches, index_searches,
    exact_blocks, lossy_blocks
});

impl From<&p::Extra> for dto::Extra {
    fn from(e: &p::Extra) -> Self {
        Self {
            key: e.key.clone(),
            label: e.label.clone(),
            group: e.group.clone(),
            value: (&e.value).into(),
        }
    }
}

impl From<&p::ExtraValue> for dto::ExtraValue {
    fn from(v: &p::ExtraValue) -> Self {
        use serde_json::Value as J;
        let (value, unit) = match v {
            p::ExtraValue::Bool(b) => (J::Bool(*b), None),
            p::ExtraValue::Int(n) => (J::from(*n), None),
            p::ExtraValue::Float(n) => (J::from(*n), None),
            p::ExtraValue::Text(t) => (J::String(t.clone()), None),
            p::ExtraValue::Quantity(q) => (J::from(q.value), Some(q.unit.id().to_owned())),
            p::ExtraValue::List(items) => (
                J::Array(
                    items
                        .iter()
                        .map(|i| serde_json::to_value(dto::ExtraValue::from(i)).unwrap_or(J::Null))
                        .collect(),
                ),
                None,
            ),
            p::ExtraValue::Group(items) => (
                J::Array(
                    items
                        .iter()
                        .map(|i| serde_json::to_value(dto::Extra::from(i)).unwrap_or(J::Null))
                        .collect(),
                ),
                None,
            ),
        };
        Self { kind: v.type_id().to_owned(), value, unit }
    }
}

impl From<dto::Extra> for p::Extra {
    fn from(e: dto::Extra) -> Self {
        Self { key: e.key, label: e.label, group: e.group, value: e.value.into() }
    }
}

/// A value as far as it's understood: an unknown type is kept as its JSON's text.
impl From<dto::ExtraValue> for p::ExtraValue {
    fn from(v: dto::ExtraValue) -> Self {
        use serde_json::Value as J;
        match (v.kind.as_str(), v.value) {
            ("bool", J::Bool(b)) => p::ExtraValue::Bool(b),
            ("int", J::Number(n)) if n.as_i64().is_some() => {
                p::ExtraValue::Int(n.as_i64().unwrap_or_default())
            }
            ("float" | "int", J::Number(n)) => p::ExtraValue::Float(n.as_f64().unwrap_or_default()),
            ("text", J::String(t)) => p::ExtraValue::Text(t),
            ("quantity", J::Number(n)) => p::ExtraValue::Quantity(p::Quantity::new(
                n.as_f64().unwrap_or_default(),
                v.unit.as_deref().and_then(p::Unit::from_id).unwrap_or(p::Unit::None),
            )),
            ("list", J::Array(items)) => p::ExtraValue::List(
                items
                    .into_iter()
                    .filter_map(|i| serde_json::from_value::<dto::ExtraValue>(i).ok())
                    .map(Into::into)
                    .collect(),
            ),
            ("group", J::Array(items)) => p::ExtraValue::Group(
                items
                    .into_iter()
                    .filter_map(|i| serde_json::from_value::<dto::Extra>(i).ok())
                    .map(Into::into)
                    .collect(),
            ),
            (_, J::String(t)) => p::ExtraValue::Text(t),
            (_, J::Bool(b)) => p::ExtraValue::Bool(b),
            (_, other) => p::ExtraValue::Text(other.to_string()),
        }
    }
}

fn extras(list: &[p::Extra]) -> Vec<dto::Extra> {
    list.iter().map(Into::into).collect()
}

fn extras_back(list: Vec<dto::Extra>) -> Vec<p::Extra> {
    list.into_iter().map(Into::into).collect()
}

impl From<&p::ProfileNode> for dto::ProfileNode {
    fn from(n: &p::ProfileNode) -> Self {
        let memory = n.memory.as_ref().map(|m| dto::Memory {
            peak_bytes: m.peak_bytes,
            sort_method: m.sort_method.clone(),
            sort_space: m
                .sort_space
                .map(|(bytes, s)| dto::SortSpace { bytes, storage: s.id().to_owned() }),
            hash: m.hash.map(Into::into),
            cache: m.cache.map(Into::into),
            storage: m.storage.map(|s| s.id().to_owned()),
        });
        let parallel = n.parallel.as_ref().map(|par| dto::Parallel {
            planned: par.planned,
            launched: par.launched,
            aware: par.aware,
            per_worker: par
                .per_worker
                .iter()
                .map(|w| dto::Worker {
                    number: w.number,
                    rows: w.rows,
                    loops: w.loops,
                    startup_ms: w.startup_ms,
                    total_ms: w.total_ms,
                    io: w.io.map(Into::into),
                    extra: extras(&w.extra),
                })
                .collect(),
        });
        Self {
            parent: n.parent.map(|p| p.0),
            children: n.children.iter().map(|c| c.0).collect(),
            role: n.role.map(|r| r.id().to_owned()),
            subplan_name: n.subplan_name.clone(),
            op: n.op.id().to_owned(),
            name: n.name.clone(),
            join: n.join.map(|j| j.id().to_owned()),
            object: n.object.as_ref().map(|o| dto::ObjectRef {
                kind: o.kind.id().to_owned(),
                schema: o.schema.clone(),
                name: o.name.clone(),
                alias: o.alias.clone(),
            }),
            index: n.index.clone(),
            scan_direction: n.scan_direction.map(|s| s.id().to_owned()),
            conditions: n
                .conditions
                .iter()
                .map(|c| dto::Condition {
                    kind: c.kind.id().to_owned(),
                    label: match &c.kind {
                        p::CondKind::Other(l) if !l.is_empty() => Some(l.clone()),
                        _ => None,
                    },
                    text: c.text.clone(),
                })
                .collect(),
            output: n.output.clone(),
            estimates: (n.estimates != p::Estimates::default()).then(|| n.estimates.into()),
            actuals: (n.actuals != p::Actuals::default()).then(|| n.actuals.into()),
            io: n.io.map(Into::into),
            memory,
            spill: n.spill.map(|s| dto::Spill {
                kind: s.kind.id().to_owned(),
                disk_bytes: s.disk_bytes,
                batches: s.batches,
            }),
            parallel,
            wal: n.wal.map(Into::into),
            flags: (n.flags != p::NodeFlags::default()).then(|| n.flags.into()),
            extra: extras(&n.extra),
        }
    }
}

fn node_back(n: dto::ProfileNode) -> p::ProfileNode {
    let op = p::OpKind::from_id(&n.op).unwrap_or(p::OpKind::Other);
    let mut node = p::ProfileNode::new(op, n.name);
    node.parent = n.parent.map(p::NodeId);
    node.children = n.children.into_iter().map(p::NodeId).collect();
    node.role = n.role.as_deref().and_then(p::ChildRole::from_id);
    node.subplan_name = n.subplan_name;
    node.join = n.join.as_deref().and_then(p::JoinKind::from_id);
    node.object = n.object.map(|o| p::ObjectRef {
        kind: p::ObjectKind::from_id(&o.kind).unwrap_or(p::ObjectKind::Other),
        schema: o.schema,
        name: o.name,
        alias: o.alias,
    });
    node.index = n.index;
    node.scan_direction = n.scan_direction.as_deref().and_then(p::ScanDirection::from_id);
    node.conditions = n
        .conditions
        .into_iter()
        .map(|c| {
            let kind = p::CondKind::from_id(&c.kind, c.label.as_deref())
                .unwrap_or_else(|| p::CondKind::Other(c.label.unwrap_or(c.kind)));
            p::Condition { kind, text: c.text }
        })
        .collect();
    node.output = n.output;
    node.estimates = n.estimates.map(Into::into).unwrap_or_default();
    node.actuals = n.actuals.map(Into::into).unwrap_or_default();
    node.io = n.io.map(Into::into);
    node.memory = n.memory.map(|m| p::Memory {
        peak_bytes: m.peak_bytes,
        sort_method: m.sort_method,
        sort_space: m
            .sort_space
            .map(|s| (s.bytes, p::Storage::from_id(&s.storage).unwrap_or(p::Storage::Memory))),
        hash: m.hash.map(Into::into),
        cache: m.cache.map(Into::into),
        storage: m.storage.as_deref().and_then(p::Storage::from_id),
    });
    node.spill = n.spill.and_then(|s| {
        Some(p::Spill {
            kind: p::SpillKind::from_id(&s.kind)?,
            disk_bytes: s.disk_bytes,
            batches: s.batches,
        })
    });
    node.parallel = n.parallel.map(|par| p::Parallel {
        planned: par.planned,
        launched: par.launched,
        aware: par.aware,
        per_worker: par
            .per_worker
            .into_iter()
            .map(|w| p::WorkerStats {
                number: w.number,
                rows: w.rows,
                loops: w.loops,
                startup_ms: w.startup_ms,
                total_ms: w.total_ms,
                io: w.io.map(Into::into),
                extra: extras_back(w.extra),
            })
            .collect(),
    });
    node.wal = n.wal.map(Into::into);
    node.flags = n.flags.map(Into::into).unwrap_or_default();
    node.extra = extras_back(n.extra);
    node
}

impl From<&p::StatementProfile> for dto::StatementProfile {
    fn from(s: &p::StatementProfile) -> Self {
        Self {
            planning_ms: s.planning_ms,
            execution_ms: s.execution_ms,
            wall_ms: s.wall_ms,
            first_row_ms: s.first_row_ms,
            fetch_ms: s.fetch_ms,
            rows_returned: s.rows_returned,
            rows_affected: s.rows_affected,
            planning_io: s.planning_io.map(Into::into),
            io_total: s.io_total.map(Into::into),
            planning_memory: s.planning_memory.map(Into::into),
            serialize: s.serialize.clone().map(Into::into),
            jit: s.jit.map(Into::into),
            triggers: s.triggers.iter().cloned().map(Into::into).collect(),
            settings: s.settings.iter().cloned().map(Into::into).collect(),
            query_id: s.query_id,
            counters: s.counters.map(Into::into),
            history: s.history.map(Into::into),
            waits: s.waits.iter().cloned().map(Into::into).collect(),
            extra: extras(&s.extra),
        }
    }
}

impl From<dto::StatementProfile> for p::StatementProfile {
    fn from(s: dto::StatementProfile) -> Self {
        Self {
            planning_ms: s.planning_ms,
            execution_ms: s.execution_ms,
            wall_ms: s.wall_ms,
            first_row_ms: s.first_row_ms,
            fetch_ms: s.fetch_ms,
            rows_returned: s.rows_returned,
            rows_affected: s.rows_affected,
            planning_io: s.planning_io.map(Into::into),
            io_total: s.io_total.map(Into::into),
            planning_memory: s.planning_memory.map(Into::into),
            serialize: s.serialize.map(Into::into),
            jit: s.jit.map(Into::into),
            triggers: s.triggers.into_iter().map(Into::into).collect(),
            settings: s.settings.into_iter().map(Into::into).collect(),
            query_id: s.query_id,
            counters: s.counters.map(Into::into),
            history: s.history.map(Into::into),
            waits: s.waits.into_iter().map(Into::into).collect(),
            extra: extras_back(s.extra),
        }
    }
}

impl From<&p::Profile> for dto::Profile {
    fn from(profile: &p::Profile) -> Self {
        Self {
            format: profile.format,
            id: Some(profile.id.to_string()),
            engine: dto::Engine {
                driver: profile.engine.driver.clone(),
                name: profile.engine.name.clone(),
                version: profile.engine.version.clone(),
                version_num: profile.engine.version_num,
            },
            depth: p::export::depth_id(profile.depth).to_owned(),
            statement: (&profile.statement).into(),
            nodes: profile.nodes.iter().map(Into::into).collect(),
            roots: profile.roots.iter().map(|r| r.0).collect(),
            collected: profile.collected.into(),
            notes: profile
                .notes
                .iter()
                .map(|n| dto::ProfileNote { id: n.id().to_owned(), argument: n.argument() })
                .collect(),
            raw: profile.raw.as_ref().map(|r| dto::RawPlan {
                format: r.format.id().to_owned(),
                text: r.text.to_string(),
            }),
        }
    }
}

/// A profile as far as it's understood: of a newer format, what isn't known is left out
/// (said by [`p::ProfileNote::NewerFormat`]); a node listed where it can't be (a parent or
/// child past the list) is dropped from the tree, never a panic.
pub fn profile_from(dto: dto::Profile) -> p::Profile {
    let depth = p::export::depth_from_id(&dto.depth).unwrap_or(p::ProfileDepth::Estimated);
    let engine = p::Engine {
        driver: dto.engine.driver,
        name: dto.engine.name,
        version: dto.engine.version,
        version_num: dto.engine.version_num,
    };
    let mut profile = p::Profile::new(engine, depth);
    if let Some(id) = dto.id.as_deref().and_then(p::ProfileId::parse) {
        profile.id = id;
    }
    let count = dto.nodes.len() as u32;
    profile.nodes = dto.nodes.into_iter().map(node_back).collect();
    for node in &mut profile.nodes {
        node.parent = node.parent.filter(|p| p.0 < count);
        node.children.retain(|c| c.0 < count);
    }
    profile.roots = dto.roots.into_iter().filter(|r| *r < count).map(p::NodeId).collect();
    profile.statement = dto.statement.into();
    profile.collected = dto.collected.into();
    profile.notes =
        dto.notes.into_iter().filter_map(|n| p::ProfileNote::from_id(&n.id, n.argument)).collect();
    profile.raw = dto.raw.and_then(|r| {
        Some(p::RawPlan { format: p::RawFormat::from_id(&r.format)?, text: r.text.into() })
    });
    if dto.format > p::PROFILE_FORMAT {
        profile.note(p::ProfileNote::NewerFormat { format: dto.format });
    }
    profile.mark_size();
    profile
}

impl From<&p::ProfileOptions> for dto::ProfileOptions {
    fn from(o: &p::ProfileOptions) -> Self {
        Self {
            depth: p::export::depth_id(o.depth).to_owned(),
            buffers: o.buffers,
            verbose: o.verbose,
            settings: o.settings,
            wal: o.wal,
            serialize: o.serialize,
            statement_history: o.statement_history,
            timeout_ms: o.timeout.map(|t| t.as_millis().min(u128::from(u64::MAX)) as u64),
            max_raw_bytes: Some(o.max_raw_bytes as u64),
        }
    }
}

impl From<dto::ProfileOptions> for p::ProfileOptions {
    fn from(o: dto::ProfileOptions) -> Self {
        Self {
            depth: p::export::depth_from_id(&o.depth).unwrap_or(p::ProfileDepth::Estimated),
            buffers: o.buffers,
            verbose: o.verbose,
            settings: o.settings,
            wal: o.wal,
            serialize: o.serialize,
            statement_history: o.statement_history,
            timeout: o.timeout_ms.map(std::time::Duration::from_millis),
            max_raw_bytes: o
                .max_raw_bytes
                .map_or(p::MAX_RAW_BYTES, |b| (b as usize).min(p::MAX_RAW_BYTES)),
        }
    }
}

impl From<&p::TableStats> for dto::TableStats {
    fn from(t: &p::TableStats) -> Self {
        Self {
            table: t.table.clone().into(),
            live_rows: t.live_rows,
            dead_rows: t.dead_rows,
            table_bytes: t.table_bytes,
            index_bytes: t.index_bytes,
            total_bytes: t.total_bytes,
            seq_scans: t.seq_scans,
            idx_scans: t.idx_scans,
            last_analyze: t.last_analyze.clone(),
            last_autoanalyze: t.last_autoanalyze.clone(),
            last_vacuum: t.last_vacuum.clone(),
            modified_since_analyze: t.modified_since_analyze,
            stats_present: t.stats_present,
            indexes: t
                .indexes
                .iter()
                .map(|i| dto::IndexStats {
                    name: i.name.clone(),
                    columns: i.columns.clone(),
                    unique: i.unique,
                    bytes: i.bytes,
                    scans: i.scans,
                })
                .collect(),
            extra: extras(&t.extra),
        }
    }
}

impl From<dto::TableStats> for p::TableStats {
    fn from(t: dto::TableStats) -> Self {
        Self {
            table: t.table.into(),
            live_rows: t.live_rows,
            dead_rows: t.dead_rows,
            table_bytes: t.table_bytes,
            index_bytes: t.index_bytes,
            total_bytes: t.total_bytes,
            seq_scans: t.seq_scans,
            idx_scans: t.idx_scans,
            last_analyze: t.last_analyze,
            last_autoanalyze: t.last_autoanalyze,
            last_vacuum: t.last_vacuum,
            modified_since_analyze: t.modified_since_analyze,
            stats_present: t.stats_present,
            indexes: t
                .indexes
                .into_iter()
                .map(|i| p::IndexStats {
                    name: i.name,
                    columns: i.columns,
                    unique: i.unique,
                    bytes: i.bytes,
                    scans: i.scans,
                })
                .collect(),
            extra: extras_back(t.extra),
        }
    }
}
