//! A result's rows lent to whoever reads them as a table (`charts_model::Tabular`: a pane
//! over a result, a chart): the rows' chunks shared as they are (none copied), read as
//! numbers, moments or text. Both a console's result and a table's rows read through it.

use std::borrow::Cow;
use std::sync::Arc;

use charts_model::{Column, ColumnKind, Tabular};

use crate::Value;

/// Rows in chunks (each an `Arc`, shared with whoever holds them), under named columns.
#[derive(Debug, Clone)]
pub struct Lent<R> {
    columns: Vec<Column>,
    chunks: Vec<Arc<[R]>>,
    /// The first row of each chunk.
    starts: Vec<u64>,
    len: usize,
}

impl<R: AsRef<[Value]>> Lent<R> {
    /// `chunks` (whose first rows are `starts`) under `columns`.
    pub fn new(columns: Vec<Column>, chunks: Vec<Arc<[R]>>, starts: Vec<u64>) -> Self {
        let len = chunks.iter().map(|c| c.len()).sum();
        Self { columns, chunks, starts, len }
    }

    /// The chunks as they follow one another (their starts counted).
    pub fn chained(columns: Vec<Column>, chunks: Vec<Arc<[R]>>) -> Self {
        let mut at = 0u64;
        let starts = chunks
            .iter()
            .map(|c| {
                let start = at;
                at += c.len() as u64;
                start
            })
            .collect();
        Self::new(columns, chunks, starts)
    }

    fn cell(&self, row: usize, col: usize) -> Option<&Value> {
        let i = row as u64;
        let at = self.starts.partition_point(|&s| s <= i).checked_sub(1)?;
        let chunk = self.chunks.get(at)?;
        chunk.get((i - self.starts[at]) as usize)?.as_ref().get(col)
    }
}

impl<R: AsRef<[Value]> + Send + Sync> Tabular for Lent<R> {
    fn columns(&self) -> &[Column] {
        &self.columns
    }

    fn rows(&self) -> usize {
        self.len
    }

    fn number(&self, row: usize, col: usize) -> Option<f64> {
        let moment = self.columns.get(col).is_some_and(|c| c.kind == ColumnKind::Time);
        let n = match self.cell(row, col)? {
            Value::Int(i) => *i as f64,
            Value::Float(f) => *f,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Text(t) | Value::Other(t) if moment => charts_model::moment(t)?,
            Value::Text(t) | Value::Other(t) => t.trim().parse().ok()?,
            _ => return None,
        };
        n.is_finite().then_some(n)
    }

    fn text(&self, row: usize, col: usize) -> Option<Cow<'_, str>> {
        match self.cell(row, col)? {
            Value::Null => None,
            Value::Text(t) | Value::Json(t) | Value::Other(t) => Some(Cow::Borrowed(t)),
            other => Some(Cow::Owned(other.display())),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lent.rs"]
mod tests;
