//! A dialect as the data extractors write SQL by ([`DialectSql`]): its names' quoting and its
//! literals (what it refuses — a NUL in a text, a float that isn't a number — written as
//! standard SQL does instead).

use std::sync::Arc;

use extractors::{AnsiSql, Cell, SqlStyle};

use crate::{Dialect, Value};

/// `dialect` as an [`SqlStyle`].
#[derive(Clone)]
pub struct DialectSql(pub Arc<dyn Dialect>);

impl std::fmt::Debug for DialectSql {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DialectSql").field(&self.0.name()).finish()
    }
}

impl SqlStyle for DialectSql {
    fn name(&self, name: &str) -> String {
        self.0.quote_identifier(name).unwrap_or_else(|_| AnsiSql.name(name))
    }

    fn literal(&self, value: Cell<'_>) -> String {
        if matches!(value, Cell::Float(f) if !f.is_finite()) {
            return "NULL".to_owned();
        }
        self.0.literal(&value_of(value)).unwrap_or_else(|_| AnsiSql.literal(value))
    }
}

/// `dialect`'s style, shared.
pub fn sql_style(dialect: Arc<dyn Dialect>) -> Arc<dyn SqlStyle> {
    Arc::new(DialectSql(dialect))
}

/// An extractor's cell as a value of a result.
pub fn value_of(cell: Cell<'_>) -> Value {
    match cell {
        Cell::Null => Value::Null,
        Cell::Bool(b) => Value::Bool(b),
        Cell::Int(i) => Value::Int(i),
        Cell::Float(f) => Value::Float(f),
        Cell::Text(t) => Value::Text(t.to_owned()),
        Cell::Bytes(b) => Value::Bytes(b.to_vec()),
        Cell::Json(t) => Value::Json(t.to_owned()),
        Cell::Other(t) => Value::Other(t.to_owned()),
    }
}

/// A result's value as an extractor's cell.
pub fn cell_of(value: &Value) -> Cell<'_> {
    match value {
        Value::Null => Cell::Null,
        Value::Bool(b) => Cell::Bool(*b),
        Value::Int(i) => Cell::Int(*i),
        Value::Float(f) => Cell::Float(*f),
        Value::Text(t) => Cell::Text(t),
        Value::Bytes(b) => Cell::Bytes(b),
        Value::Json(t) => Cell::Json(t),
        Value::Other(t) => Cell::Other(t),
    }
}

#[cfg(test)]
#[path = "../tests/unit/extract.rs"]
mod tests;
