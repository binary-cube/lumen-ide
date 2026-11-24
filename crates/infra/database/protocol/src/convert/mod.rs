//! The messages to and from the domain's model, both ways (the editor's side and a Rust
//! driver's side use the same).

use database as d;

use crate::dto;

mod capabilities;
mod changing;
mod connection;
mod profile;
mod reading;
mod scripts;

pub use profile::profile_from;

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

#[cfg(test)]
#[path = "../../tests/unit/convert.rs"]
mod tests;
