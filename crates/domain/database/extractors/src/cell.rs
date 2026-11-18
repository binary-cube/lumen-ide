//! What an extractor reads: the columns ([`Column`]: name, type, what kind of values, the
//! key, computed or generated) and each row's values ([`Cell`], borrowed: nothing is copied
//! to write them).

use std::borrow::Cow;
use std::fmt::Write as _;

/// A value as its source holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cell<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(&'a str),
    Bytes(&'a [u8]),
    /// JSON, as the source gave it.
    Json(&'a str),
    /// Anything else, as text (dates, decimals, UUIDs).
    Other(&'a str),
}

impl<'a> Cell<'a> {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Its plain text: `NULL` empty, bytes as `0x…` hexadecimal, numbers and booleans as
    /// written, the rest as it is.
    pub fn text(&self) -> Cow<'a, str> {
        match self {
            Self::Null => Cow::Borrowed(""),
            Self::Bool(b) => Cow::Borrowed(if *b { "true" } else { "false" }),
            Self::Int(i) => Cow::Owned(i.to_string()),
            Self::Float(f) => Cow::Owned(f.to_string()),
            Self::Text(t) | Self::Json(t) | Self::Other(t) => Cow::Borrowed(*t),
            Self::Bytes(b) => Cow::Owned(format!("0x{}", hex(b))),
        }
    }

    /// A number (an integer or a finite float: written unquoted).
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Int(_)) || matches!(self, Self::Float(f) if f.is_finite())
    }
}

/// `bytes` in lowercase hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// What kind of values a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Kind {
    #[default]
    Text,
    Integer,
    Decimal,
    Bool,
    DateTime,
    Binary,
    Json,
    Other,
}

impl Kind {
    pub fn is_number(self) -> bool {
        matches!(self, Self::Integer | Self::Decimal)
    }

    /// Its name in a template (`{{column.kind}}`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Bool => "bool",
            Self::DateTime => "datetime",
            Self::Binary => "binary",
            Self::Json => "json",
            Self::Other => "other",
        }
    }
}

/// A column as an extractor writes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Column {
    pub name: String,
    /// The engine's type (`INTEGER`, `varchar(255)`; empty: not known).
    pub type_name: String,
    pub kind: Kind,
    /// Part of the key its rows are found by (SQL `UPDATE`s, a `WHERE` clause).
    pub key: bool,
    /// Its values are worked out by the engine from others (`GENERATED ALWAYS AS (…)`,
    /// an expression of a query): "Skip Computed Columns" leaves it out.
    pub computed: bool,
    /// Its values are made by the engine (an auto-increment, an identity): "Skip Generated
    /// Columns" leaves it out.
    pub generated: bool,
}

impl Column {
    /// A plain column (no key, neither computed nor generated).
    pub fn new(name: impl Into<String>, kind: Kind) -> Self {
        Self { name: name.into(), kind, ..Self::default() }
    }

    pub fn with_type(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = type_name.into();
        self
    }

    pub fn keyed(mut self) -> Self {
        self.key = true;
        self
    }
}
