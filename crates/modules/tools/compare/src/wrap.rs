//! The rows' heights when lines wrap ([`ui_components::TextOptions::word_wrap`]): a row of
//! the comparison holds a line on each side (and the result's), so it takes as many rows as
//! the longest of them needs at its column's width; the others leave the rest blank. Kept
//! as each row's first screen row, made again only when the rows or the widths change;
//! off, every row is one high and none of this is made.

use std::ops::Range;

use super::*;

/// What the heights were made for: the rows' generation, each column's width in columns.
type HeightsKey = (u64, [usize; 3]);

/// The rows' screen rows (see the module's doc).
#[derive(Debug, Default)]
pub(crate) struct Heights {
    key: Option<HeightsKey>,
    /// Each row's first screen row; one more at the end: all of them.
    firsts: Vec<u32>,
    /// Scratch: a line as drawn (tabs as four spaces) and its parts.
    drawn: String,
    parts: Vec<Range<usize>>,
}

impl Heights {
    /// Made again for `rows` (their texts on `left` / `right`) at `columns` per column when
    /// either changed (`generation`: the rows').
    pub(crate) fn refresh(
        &mut self,
        (rows, left, right): (&[Row], &[String], &[String]),
        generation: u64,
        columns: [usize; 3],
    ) {
        let key = (generation, columns);
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.firsts.clear();
        self.firsts.reserve(rows.len() + 1);
        let mut at: u32 = 0;
        for row in rows {
            self.firsts.push(at);
            let texts = [
                row.left.and_then(|i| left.get(i)).map(String::as_str),
                row.result.as_deref(),
                row.right.and_then(|i| right.get(i)).map(String::as_str),
            ];
            let mut most = 1;
            for (which, text) in texts.into_iter().enumerate() {
                let (Some(text), true) = (text, columns[which] > 0) else { continue };
                // Short enough for any break: one row, not read.
                if text.len() * 4 <= columns[which] {
                    continue;
                }
                most = most.max(self.count(text, columns[which]));
            }
            at = at.saturating_add(most);
        }
        self.firsts.push(at);
    }

    /// How many rows `line` takes at `columns`.
    fn count(&mut self, line: &str, columns: usize) -> u32 {
        drawn_into(line, &mut self.drawn);
        ui_components::text_options::wrap_rows(&self.drawn, columns, 4, &mut self.parts);
        self.parts.len() as u32
    }

    /// Row `row`'s first screen row.
    pub(crate) fn first(&self, row: usize) -> u32 {
        self.firsts.get(row).copied().unwrap_or_else(|| self.total())
    }

    /// How many screen rows row `row` takes.
    pub(crate) fn rows(&self, row: usize) -> u32 {
        self.first(row + 1).saturating_sub(self.first(row)).max(1)
    }

    /// All the screen rows.
    pub(crate) fn total(&self) -> u32 {
        self.firsts.last().copied().unwrap_or(0)
    }

    /// The row holding screen row `screen`, and which of its rows that is.
    pub(crate) fn at(&self, screen: u32) -> (usize, u32) {
        let rows = self.firsts.len().saturating_sub(1);
        let row = self
            .firsts
            .partition_point(|&f| f <= screen)
            .saturating_sub(1)
            .min(rows.saturating_sub(1));
        (row, screen.saturating_sub(self.first(row)))
    }
}

impl Compare {
    /// Lines wrap now (on, and the columns' widths known since a frame).
    pub(crate) fn wraps(&self) -> bool {
        self.wrap && self.wrap_columns.iter().any(|c| *c > 0)
    }

    /// Row `row`'s top, from the rows' top.
    pub(crate) fn row_top(&self, row: usize) -> f32 {
        let at = if self.wraps() { self.heights.first(row) as usize } else { row };
        at as f32 * self.row_h
    }

    /// Row `row`'s height.
    pub(crate) fn row_height(&self, row: usize) -> f32 {
        let rows = if self.wraps() { self.heights.rows(row) } else { 1 };
        rows as f32 * self.row_h
    }

    /// All the rows' height.
    pub(crate) fn rows_height(&self) -> f32 {
        let rows = if self.wraps() { self.heights.total() as usize } else { self.rows.len() };
        rows as f32 * self.row_h
    }

    /// The row at `y` from the rows' top (scrolled), and which of its screen rows.
    pub(crate) fn row_at(&self, y: f32) -> (usize, u32) {
        let screen = (y / self.row_h.max(1.0)).floor().max(0.0) as u32;
        if self.wraps() { self.heights.at(screen) } else { (screen as usize, 0) }
    }

    /// The rows in view (of `rows`, the paint holding them meanwhile): the first, and how
    /// many.
    pub(crate) fn rows_in_view_of(&self, rows: usize) -> (usize, usize) {
        let (first, _) = self.row_at(self.scroll);
        let bottom = self.scroll + self.list.size.height;
        let mut count = 0;
        while first + count < rows && self.row_top(first + count) < bottom {
            count += 1;
        }
        (first, count + 1)
    }

    /// A line's rows in column `which`, as the columns (characters drawn, a tab four)
    /// each starts and ends at: one, the whole line, when nothing wraps.
    pub(crate) fn part_columns(&self, line: &str, which: usize) -> Vec<(usize, usize)> {
        let columns = self.wrap_columns.get(which).copied().unwrap_or(0);
        let mut drawn = String::new();
        drawn_into(line, &mut drawn);
        if !self.wraps() || columns == 0 {
            return vec![(0, drawn.chars().count())];
        }
        let mut parts = Vec::new();
        ui_components::text_options::wrap_rows(&drawn, columns, 4, &mut parts);
        let col = |byte: usize| drawn[..byte].chars().count();
        parts.iter().map(|p| (col(p.start), col(p.end))).collect()
    }
}

/// `line` as drawn (a tab as four spaces) into `out`.
pub(crate) fn drawn_into(line: &str, out: &mut String) {
    out.clear();
    for c in line.chars() {
        if c == '\t' {
            out.push_str("    ");
        } else {
            out.push(c);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/wrap.rs"]
mod tests;
