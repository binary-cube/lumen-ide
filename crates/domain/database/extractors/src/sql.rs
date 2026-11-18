//! How the SQL extractors write names and values: the source's dialect ([`SqlStyle`], given by
//! whoever has one — a database's dialect adapted to it), standard SQL otherwise
//! ([`AnsiSql`]).

use crate::cell::{Cell, hex};

/// Names and literals as an engine reads them.
pub trait SqlStyle: Send + Sync + std::fmt::Debug {
    /// A name (a table's, a column's), quoted as the engine wants it.
    fn name(&self, name: &str) -> String;

    /// A value as a literal (`NULL`, a number, a quoted text, bytes).
    fn literal(&self, value: Cell<'_>) -> String;
}

/// Standard SQL: names in double quotes, texts in single ones (theirs doubled), bytes as
/// `X'…'`, booleans `TRUE` / `FALSE`, a float that isn't finite `NULL`.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnsiSql;

impl SqlStyle for AnsiSql {
    fn name(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }

    fn literal(&self, value: Cell<'_>) -> String {
        ansi_literal(value)
    }
}

/// A value as standard SQL writes it (see [`AnsiSql`]).
pub fn ansi_literal(value: Cell<'_>) -> String {
    match value {
        Cell::Null => "NULL".to_owned(),
        Cell::Bool(b) => if b { "TRUE" } else { "FALSE" }.to_owned(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) if f.is_finite() => f.to_string(),
        Cell::Float(_) => "NULL".to_owned(),
        Cell::Bytes(b) => format!("X'{}'", hex(b)),
        Cell::Text(t) | Cell::Json(t) | Cell::Other(t) => format!("'{}'", t.replace('\'', "''")),
    }
}
