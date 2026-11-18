//! What's selected in the grid: cells, rows or columns, in rectangles (the last one being
//! extended with ⇧, another added with ⌘), and where the keyboard is (the cursor).
//! Rows are the source's (0-based), columns their places as shown.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

/// A cell, by its row and its column's place as shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellPos {
    pub row: u64,
    pub col: usize,
}

impl CellPos {
    pub fn new(row: u64, col: usize) -> Self {
        Self { row, col }
    }
}

/// A rectangle of cells, from where it started to where it reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub anchor: CellPos,
    pub cursor: CellPos,
}

impl Span {
    pub fn rows(&self) -> RangeInclusive<u64> {
        self.anchor.row.min(self.cursor.row)..=self.anchor.row.max(self.cursor.row)
    }

    pub fn cols(&self) -> RangeInclusive<usize> {
        self.anchor.col.min(self.cursor.col)..=self.anchor.col.max(self.cursor.col)
    }

    pub fn contains(&self, p: CellPos) -> bool {
        self.rows().contains(&p.row) && self.cols().contains(&p.col)
    }
}

/// The selection (see the module's doc).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    spans: Vec<Span>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn clear(&mut self) {
        self.spans.clear();
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Where the keyboard is: the last span's moving end.
    pub fn cursor(&self) -> Option<CellPos> {
        self.spans.last().map(|s| s.cursor)
    }

    /// Only `p`.
    pub fn set(&mut self, p: CellPos) {
        self.spans = vec![Span { anchor: p, cursor: p }];
    }

    /// The last span reaching `p` (⇧); only `p` when there's none.
    pub fn extend(&mut self, p: CellPos) {
        match self.spans.last_mut() {
            Some(span) => span.cursor = p,
            None => self.set(p),
        }
    }

    /// `p` added as a span of its own (⌘), or taken out when it was the only cell of one.
    pub fn toggle(&mut self, p: CellPos) {
        if let Some(i) = self.spans.iter().position(|s| s.anchor == p && s.cursor == p) {
            self.spans.remove(i);
        } else {
            self.spans.push(Span { anchor: p, cursor: p });
        }
    }

    /// Whole rows `from`..=`to` of `cols` columns (`add`: beside what's selected).
    pub fn rows(&mut self, from: u64, to: u64, cols: usize, add: bool) {
        let span = Span {
            anchor: CellPos::new(from, 0),
            cursor: CellPos::new(to, cols.saturating_sub(1)),
        };
        if !add {
            self.spans.clear();
        }
        self.spans.push(span);
    }

    /// A whole column over rows `first`..=`last` (`add`: beside what's selected).
    pub fn column(&mut self, col: usize, first: u64, last: u64, add: bool) {
        let span = Span { anchor: CellPos::new(first, col), cursor: CellPos::new(last, col) };
        if !add {
            self.spans.clear();
        }
        self.spans.push(span);
    }

    /// Every cell of rows `first`..=`last` and `cols` columns.
    pub fn all(&mut self, first: u64, last: u64, cols: usize) {
        self.rows(first, last, cols, false);
        // The keyboard stays at the top.
        if let Some(span) = self.spans.last_mut() {
            std::mem::swap(&mut span.anchor, &mut span.cursor);
        }
    }

    pub fn contains(&self, p: CellPos) -> bool {
        self.spans.iter().any(|s| s.contains(p))
    }

    /// The rows touched, in order.
    pub fn row_set(&self) -> BTreeSet<u64> {
        self.spans.iter().flat_map(|s| s.rows()).collect()
    }

    /// Whether row `row` is touched (a visible row's number painted: no set of them all).
    pub fn has_row(&self, row: u64) -> bool {
        self.spans.iter().any(|s| s.rows().contains(&row))
    }

    /// The columns touched, in order.
    pub fn col_set(&self) -> BTreeSet<usize> {
        self.spans.iter().flat_map(|s| s.cols()).collect()
    }

    /// Every selected cell once, by row then column.
    pub fn cells(&self) -> Vec<CellPos> {
        let mut cells: BTreeSet<CellPos> = BTreeSet::new();
        for span in &self.spans {
            for row in span.rows() {
                for col in span.cols() {
                    cells.insert(CellPos::new(row, col));
                }
            }
        }
        cells.into_iter().collect()
    }

    /// How many cells (counted without listing them: a column of a million rows).
    pub fn count(&self) -> u64 {
        if self.spans.len() == 1 {
            let s = self.spans[0];
            let rows = s.rows().end() - s.rows().start() + 1;
            let cols = (s.cols().end() - s.cols().start() + 1) as u64;
            return rows * cols;
        }
        self.cells().len() as u64
    }

    /// The cursor moved by `rows` / `cols` within rows `first`..=`last` and `ncols` columns
    /// (`extend`: the span grows to it, ⇧). `false` when there was nothing to move.
    pub fn step(
        &mut self,
        rows: i64,
        cols: i64,
        extend: bool,
        (first, last): (u64, u64),
        ncols: usize,
    ) -> bool {
        if ncols == 0 || last < first {
            return false;
        }
        let Some(cursor) = self.cursor() else {
            self.set(CellPos::new(first, 0));
            return true;
        };
        let row = (cursor.row as i64 + rows).clamp(first as i64, last as i64) as u64;
        let col = (cursor.col as i64 + cols).clamp(0, ncols as i64 - 1) as usize;
        let to = CellPos::new(row, col);
        if extend {
            self.extend(to)
        } else {
            self.set(to)
        }
        true
    }

    /// Kept within rows `first`..=`last` and `ncols` columns (a page or the columns
    /// changed): what falls outside goes.
    pub fn clamp(&mut self, (first, last): (u64, u64), ncols: usize) {
        if ncols == 0 || last < first {
            self.spans.clear();
            return;
        }
        let fit = |p: CellPos| CellPos::new(p.row.clamp(first, last), p.col.min(ncols - 1));
        for span in &mut self.spans {
            span.anchor = fit(span.anchor);
            span.cursor = fit(span.cursor);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/selection.rs"]
mod tests;
