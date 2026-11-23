//! The grid's selection as a table of its own (`charts_model::Tabular`), for whoever draws
//! from part of the rows (a chart of the rows selected): the rows it touches, in the
//! source's order; the columns selected, or every column shown when whole rows are. Only
//! rows already read are taken (the selection is what the user sees); at most
//! [`MOST_PICKED_ROWS`] are looked at, the others only counted (a range at once, never
//! walked). Its values are copied once, when asked.

use std::borrow::Cow;

use charts_model::{Column, ColumnKind, Tabular};

use super::*;

/// The most rows taken from a selection (a chart of more draws from all the rows).
pub const MOST_PICKED_ROWS: usize = 20_000;

/// A cell taken: a number, a text, or nothing.
#[derive(Debug, Clone, PartialEq)]
enum Taken {
    Empty,
    Number(f64),
    Text(String),
}

/// The selection's rows and columns, their values copied (see the module's doc).
#[derive(Debug, Clone, Default)]
pub struct Picked {
    columns: Vec<Column>,
    cells: Vec<Vec<Taken>>,
    /// Rows selected but not taken (not read yet, or past the most taken).
    pub left_out: u64,
}

impl Tabular for Picked {
    fn columns(&self) -> &[Column] {
        &self.columns
    }

    fn rows(&self) -> usize {
        self.cells.len()
    }

    fn number(&self, row: usize, col: usize) -> Option<f64> {
        let moment = self.columns.get(col).is_some_and(|c| c.kind == ColumnKind::Time);
        match self.cells.get(row)?.get(col)? {
            Taken::Number(n) => Some(*n),
            Taken::Text(t) if moment => charts_model::moment(t),
            Taken::Text(t) => t.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
            Taken::Empty => None,
        }
    }

    fn text(&self, row: usize, col: usize) -> Option<Cow<'_, str>> {
        match self.cells.get(row)?.get(col)? {
            Taken::Number(n) => Some(Cow::Owned(n.to_string())),
            Taken::Text(t) => Some(Cow::Borrowed(t)),
            Taken::Empty => None,
        }
    }
}

impl<S: GridSource> DataGrid<S> {
    /// Whether the selection is enough to draw from: two cells or more.
    pub fn has_picked(&self) -> bool {
        self.selection.count() >= 2
    }

    /// The selection as a table (see the module's doc); `None`: less than two cells.
    pub fn picked(&self) -> Option<Picked> {
        if !self.has_picked() {
            return None;
        }
        let shown = self.layout.visible();
        let whole = self
            .selection
            .spans()
            .iter()
            .all(|s| *s.cols().start() == 0 && *s.cols().end() + 1 >= shown.len());
        let places: Vec<usize> = if whole {
            (0..shown.len()).collect()
        } else {
            self.selection.col_set().into_iter().filter(|c| *c < shown.len()).collect()
        };
        let source_columns = self.source.columns();
        let at: Vec<usize> = places.iter().map(|p| shown[*p]).collect();
        let columns =
            at.iter().filter_map(|c| source_columns.get(*c)).map(|c| c.chart_column()).collect();
        let mut picked = Picked { columns, ..Picked::default() };
        // At most this many rows looked at (read or not): past it, the rest only counted.
        let mut looked = 0usize;
        for (from, to) in merged_ranges(self.selection.spans()) {
            let mut row = from;
            while row <= to && looked < MOST_PICKED_ROWS {
                looked += 1;
                if self.source.row_state(row) == RowState::Ready {
                    picked
                        .cells
                        .push(at.iter().map(|c| taken(self.source.cell(row, *c))).collect());
                } else {
                    picked.left_out = picked.left_out.saturating_add(1);
                }
                row += 1;
            }
            if row <= to {
                picked.left_out = picked.left_out.saturating_add(to - row + 1);
            }
        }
        Some(picked)
    }
}

/// A value as it's taken: numbers as numbers, the rest as its text.
fn taken(cell: CellRef<'_>) -> Taken {
    match cell {
        CellRef::Null => Taken::Empty,
        CellRef::Int(i) => Taken::Number(i as f64),
        CellRef::Float(f) if f.is_finite() => Taken::Number(f),
        CellRef::Bool(b) => Taken::Number(f64::from(u8::from(b))),
        other => Taken::Text(other.display()),
    }
}

/// The rows the spans touch, as ranges `[from, to]` each once and in order (never listed
/// one by one: a selection of a million rows is a range).
fn merged_ranges(spans: &[super::super::selection::Span]) -> Vec<(u64, u64)> {
    let mut ranges: Vec<(u64, u64)> =
        spans.iter().map(|s| (*s.rows().start(), *s.rows().end())).collect();
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (from, to) in ranges {
        match merged.last_mut() {
            Some(last) if from <= last.1.saturating_add(1) => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    merged
}
