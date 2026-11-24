//! The connection's fields and the target a session connects to.

use database as d;

use crate::dto;

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
