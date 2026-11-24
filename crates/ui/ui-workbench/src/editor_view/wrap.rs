//! The editor's rows when lines wrap ([`ui_components::TextOptions::word_wrap`]): each shown
//! line takes the rows `text_options::wrap_rows` gives it at the text's width, over what
//! the collapsed regions leave ([`Folds`]). Kept as each line's display width (whatever the
//! view's width), its rows and its first row:
//! - a new text (or tab width) is measured in one pass over it, nothing copied;
//! - after edits, only the lines they touched are read again;
//! - a new width (a resize, a zoom) or new folds read only the lines wider than a row.
//!
//! A row's line is found by a binary search, a line's row read. Off, the rows are the
//! folds' own and none of this is made.

use std::ops::Range;

use document::Document;

use crate::folding::Folds;

/// What the lines' widths hold for: the document and its tab width.
type WidthKey = (kernel::DocumentId, u32);

/// What the rows hold for besides the widths: the folds' generation, the columns a row
/// holds, the inlay hints shown (their list's identity: a new one counts the rows again).
type RowsKey = (u64, usize, usize);

/// The wrapped rows (see the module's doc).
#[derive(Debug, Default)]
pub(super) struct WrapRows {
    width_key: Option<WidthKey>,
    rows_key: Option<RowsKey>,
    /// The text's version and length the lines were measured for (another: its edits
    /// followed).
    version: Option<(u64, usize)>,
    /// Each document line's display columns.
    widths: Vec<u32>,
    /// Each document line's rows (0: hidden in a collapsed region).
    counts: Vec<u32>,
    /// Each document line's first row (a hidden line: the next shown one's).
    firsts: Vec<u32>,
    total: u32,
    /// The inlay hints shown (sorted by offset): a line's count as room in it.
    hints: std::sync::Arc<[super::Inlay]>,
    /// Scratch, reused: a line's text, its hints' room, its rows.
    line: String,
    room: Vec<(usize, usize)>,
    rows: Vec<Range<usize>>,
    /// How many lines were read last (the tests check an edit reads only its own).
    #[cfg(test)]
    measured: usize,
}

impl WrapRows {
    /// Made again for `doc`, `folds` and `columns` when one changed (see the module's doc).
    pub(super) fn refresh(
        &mut self,
        doc: &Document,
        folds: &Folds,
        columns: usize,
        hints: &std::sync::Arc<[super::Inlay]>,
    ) {
        let buffer = doc.buffer();
        let width_key = (doc.id(), doc.tab_width());
        let hints_id = std::sync::Arc::as_ptr(hints).cast::<()>() as usize;
        let rows_key = (folds.generation(), columns, hints_id);
        self.hints = std::sync::Arc::clone(hints);
        let version = (doc.version(), buffer.len());
        let same_widths = self.width_key == Some(width_key);
        let same_rows = self.rows_key == Some(rows_key);
        if same_widths && self.version == Some(version) && same_rows {
            return;
        }
        if !same_widths || self.version != Some(version) {
            let followed = same_widths
                && self
                    .version
                    .and_then(|(was, _)| doc.changes_since(was))
                    .is_some_and(|changes| self.follow(doc, folds, columns, &changes, same_rows));
            if !followed {
                self.measure_widths(doc);
            }
            self.width_key = Some(width_key);
            self.version = Some(version);
            if followed && same_rows {
                self.sum();
                return;
            }
        }
        self.count_all(doc, folds, columns);
        self.rows_key = Some(rows_key);
        self.sum();
    }

    /// Every line's width, in one pass over the text (nothing copied).
    fn measure_widths(&mut self, doc: &Document) {
        let tab = doc.tab_width().max(1) as usize;
        self.widths.clear();
        let mut columns = 0usize;
        for chunk in doc.buffer().chunks() {
            for (i, &b) in chunk.as_bytes().iter().enumerate() {
                match b {
                    b'\n' => {
                        self.widths.push(saturated(columns));
                        columns = 0;
                    }
                    b'\t' => columns += tab - columns % tab,
                    b'\r' => {}
                    0..0x80 => columns += 1,
                    // A continuation byte.
                    0x80..0xC0 => {}
                    _ => {
                        let c = chunk[i..].chars().next();
                        columns += c.map_or(1, ui_components::text_options::char_columns);
                    }
                }
            }
        }
        self.widths.push(saturated(columns));
    }

    /// One line's width read again.
    fn measure_line(&mut self, doc: &Document, line: usize) {
        doc.buffer().write_line(line, &mut self.line);
        let tab = doc.tab_width().max(1) as usize;
        let mut columns = 0usize;
        for c in self.line.chars() {
            columns += if c == '\t' {
                tab - columns % tab
            } else {
                ui_components::text_options::char_columns(c)
            };
        }
        #[cfg(test)]
        {
            self.measured += 1;
        }
        if let Some(width) = self.widths.get_mut(line) {
            *width = saturated(columns);
        }
    }

    /// Every line's rows, from the widths.
    fn count_all(&mut self, doc: &Document, folds: &Folds, columns: usize) {
        self.counts.clear();
        self.counts.resize(self.widths.len(), 1);
        self.count(doc, folds, columns, 0..self.widths.len());
    }

    /// Lines `range`'s rows at `columns`: hidden ones none, one no wider than a row one, a
    /// wider one read and wrapped.
    fn count(&mut self, doc: &Document, folds: &Folds, columns: usize, range: Range<usize>) {
        let (buffer, tab) = (doc.buffer(), doc.tab_width());
        let hidden = folds.hidden_ranges();
        let mut next_hidden = hidden.partition_point(|&(_, end)| (end as usize) < range.start);
        for line in range {
            while hidden.get(next_hidden).is_some_and(|&(_, end)| (end as usize) < line) {
                next_hidden += 1;
            }
            let is_hidden = hidden
                .get(next_hidden)
                .is_some_and(|&(start, end)| (start as usize..=end as usize).contains(&line));
            // Its hints' room (bytes in the line, columns), counted in its width.
            self.room.clear();
            let (start, end) = (buffer.line_start(line), buffer.line_end(line));
            let first = self.hints.partition_point(|h| h.offset < start);
            for hint in self.hints[first..].iter().take_while(|h| h.offset <= end) {
                self.room.push((hint.offset - start, hint.columns()));
            }
            let room: usize = self.room.iter().map(|(_, c)| c).sum();
            let wide = self.widths.get(line).is_some_and(|&w| w as usize + room > columns);
            let rows = if is_hidden {
                0
            } else if wide {
                buffer.write_line(line, &mut self.line);
                let hang = super::rows::hang_of(&self.line, tab, columns);
                ui_components::text_options::wrap_rows_with(
                    &self.line,
                    (columns, tab, hang),
                    &self.room,
                    &mut self.rows,
                );
                #[cfg(test)]
                {
                    self.measured += 1;
                }
                self.rows.len() as u32
            } else {
                1
            };
            if let Some(count) = self.counts.get_mut(line) {
                *count = rows;
            }
        }
    }

    /// The lines the edits replaced given room for the ones they wrote, those measured (and
    /// their rows counted when `rows_too`); `false` when that's most of the text (measured
    /// whole instead).
    fn follow(
        &mut self,
        doc: &Document,
        folds: &Folds,
        columns: usize,
        changes: &[document::TextChange],
        rows_too: bool,
    ) -> bool {
        let mut dirty: Vec<(usize, usize)> = Vec::with_capacity(changes.len());
        for c in changes {
            let (start, old_end, new_end) = (
                c.start_point.line.0 as usize,
                c.old_end_point.line.0 as usize,
                c.new_end_point.line.0 as usize,
            );
            if old_end < start || old_end >= self.widths.len() || new_end < start {
                return false;
            }
            // Its lines replaced by as many as it wrote (measured below).
            let wrote = new_end - start + 1;
            self.widths.splice(start..=old_end, std::iter::repeat_n(0, wrote));
            if rows_too {
                self.counts.splice(start..=old_end, std::iter::repeat_n(1, wrote));
            }
            // What earlier edits touched, where it is now.
            let map = |l: usize, inside: usize| {
                if l < start {
                    l
                } else if l > old_end {
                    l + new_end - old_end
                } else {
                    inside
                }
            };
            for d in &mut dirty {
                let from = map(d.0, start);
                *d = (from, map(d.1, new_end).max(from));
            }
            dirty.push((start, new_end));
        }
        let lines = doc.buffer().line_count();
        if self.widths.len() != lines || (rows_too && self.counts.len() != lines) {
            return false;
        }
        let touched: usize = dirty.iter().map(|(a, b)| b.saturating_sub(*a) + 1).sum();
        if touched > lines / 2 + 1 {
            return false;
        }
        for (a, b) in dirty {
            let range = a..(b + 1).min(lines);
            for line in range.clone() {
                self.measure_line(doc, line);
            }
            if rows_too {
                self.count(doc, folds, columns, range);
            }
        }
        true
    }

    /// Each line's first row, from the counts.
    fn sum(&mut self) {
        self.firsts.clear();
        self.firsts.reserve(self.counts.len());
        let mut row: u32 = 0;
        for &count in &self.counts {
            self.firsts.push(row);
            row = row.saturating_add(count);
        }
        self.total = row;
    }

    /// How many rows line `line` takes (0: hidden).
    pub(super) fn rows(&self, line: u32) -> u32 {
        self.counts.get(line as usize).copied().unwrap_or(1)
    }

    /// How many rows are shown.
    pub(super) fn total(&self) -> u32 {
        self.total
    }

    /// The first row of line `line` (a hidden one: its region's line's).
    pub(super) fn first_row(&self, folds: &Folds, line: u32) -> u32 {
        let shown = folds.shown_line(line) as usize;
        self.firsts.get(shown).copied().unwrap_or(self.total)
    }

    /// Row `row`'s line, and which of its rows it is (0: its first).
    pub(super) fn at(&self, row: u32) -> (u32, u32) {
        let row = row.min(self.total.saturating_sub(1));
        let line = self.firsts.partition_point(|&first| first <= row).saturating_sub(1);
        let first = self.firsts.get(line).copied().unwrap_or(0);
        (line as u32, row - first)
    }
}

/// A count of columns held in a `u32` (a line that wide is wide whatever it holds).
fn saturated(columns: usize) -> u32 {
    u32::try_from(columns).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "../../tests/unit/editor_view/wrap.rs"]
mod tests;
