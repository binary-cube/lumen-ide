//! Selecting text in a column of the comparison (either side, the result), to copy a
//! difference: pressed and dragged, a word with two clicks, a line with three, a whole column
//! with ⌘A. Read only: what's selected is only copied. Positions are a row and a column in
//! the line as drawn (a tab as four spaces, the code font's advance a column); what's
//! copied is the file's own text (its tabs).

use super::*;

/// A place in a column: its row and its column in the line as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct TextPos {
    pub(crate) row: usize,
    pub(crate) col: usize,
}

/// What's selected: in which column (0 left, 1 the result, 2 right), from where to where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextSelection {
    pub(crate) which: usize,
    pub(crate) anchor: TextPos,
    pub(crate) head: TextPos,
    /// What a drag goes by: characters, or the word or line the clicks took (never cut).
    pub(crate) unit: Unit,
}

/// What a selection's drag goes by: after one click, characters; after two, whole words
/// (the first one kept whole); after three, whole lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Unit {
    #[default]
    Character,
    /// The word double-clicked, from where to where.
    Word(TextPos, TextPos),
    /// The line triple-clicked.
    Line(usize),
}

impl TextSelection {
    fn ordered(&self) -> (TextPos, TextPos) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// The columns a tab is drawn as.
const TAB_COLUMNS: usize = 4;

/// The columns `c` is drawn as (a tab as [`TAB_COLUMNS`]).
fn drawn_width(c: char) -> usize {
    if c == '\t' { TAB_COLUMNS } else { 1 }
}

/// How wide `line` is drawn, in columns (a tab as four).
fn drawn_len(line: &str) -> usize {
    line.chars().map(drawn_width).sum()
}

/// The byte where drawn column `col` starts in `line` (a column inside a tab: the tab's).
fn byte_at(line: &str, col: usize) -> usize {
    let mut at = 0;
    for (i, c) in line.char_indices() {
        let w = drawn_width(c);
        if at + w > col {
            return i;
        }
        at += w;
    }
    line.len()
}

/// The drawn columns of the word around `col` (letters, digits, `_`, `$`): walked once,
/// nothing gathered (asked at each move of a drag by words).
fn word_at(line: &str, col: usize) -> (usize, usize) {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut chars = line.chars();
    // The drawn column reached, and where the run of word characters up to it began.
    let (mut at, mut start) = (0, None);
    while let Some(c) = chars.next() {
        let w = drawn_width(c);
        start = if word(c) { start.or(Some(at)) } else { None };
        if at + w > col {
            // The character under the column: a word's, or none.
            let Some(start) = start else { return (col, col) };
            let rest: usize = chars.by_ref().take_while(|c| word(*c)).map(drawn_width).sum();
            return (start, at + w + rest);
        }
        at += w;
    }
    (col, col)
}

impl Compare {
    /// Row `row`'s line in column `which` (`None`: none there, the other side's lines).
    pub(crate) fn line_in(&self, row: usize, which: usize) -> Option<&str> {
        self.line_of(&self.rows, row, which)
    }

    /// The same, among `rows` (the paint holds them meanwhile).
    fn line_of<'a>(&'a self, rows: &'a [Row], row: usize, which: usize) -> Option<&'a str> {
        let row = rows.get(row)?;
        match which {
            0 => row.left.and_then(|i| self.left.get(i)).map(String::as_str),
            2 => row.right.and_then(|i| self.right.get(i)).map(String::as_str),
            _ => row.result.as_deref(),
        }
    }

    /// The place under `p` in column `which`.
    pub(crate) fn text_pos(&self, which: usize, p: Point) -> TextPos {
        let last = self.rows.len().saturating_sub(1);
        let (row, part) = self.row_at(p.y - self.list.min_y() + self.scroll);
        let row = row.min(last);
        let x = p.x - self.code_x[which] + self.across[which].offset;
        let col = (x / self.advance.max(1.0)).round().max(0.0) as usize;
        let line = self.line_in(row, which).unwrap_or_default();
        let len = drawn_len(line);
        // A wrapped line's row: its columns go on from where the row starts, up to its end.
        let parts = self.part_columns(line, which);
        let (from, to) = parts.get(part as usize).or(parts.last()).copied().unwrap_or((0, len));
        let end = if part as usize + 1 >= parts.len() { len } else { to.saturating_sub(1) };
        TextPos { row, col: (from + col).min(end).min(len) }
    }

    /// Whether `p` is on a column's code (past its line numbers): which one.
    pub(crate) fn code_at(&self, p: Point) -> Option<usize> {
        let which = self.column_at(p)?;
        (p.x >= self.code_x[which] && self.list.contains(p)).then_some(which)
    }

    /// A press on column `which`'s code, `clicks` in a row: the selection starts there (a
    /// word with two, the line with three).
    pub(crate) fn begin_select(&mut self, which: usize, p: Point, clicks: u32) {
        let at = self.text_pos(which, p);
        let line = self.line_in(at.row, which).unwrap_or_default();
        let (anchor, head, unit) = match clicks {
            2 => {
                let (a, b) = word_at(line, at.col);
                let (from, to) = (TextPos { row: at.row, col: a }, TextPos { row: at.row, col: b });
                (from, to, Unit::Word(from, to))
            }
            c if c >= 3 => {
                let to = TextPos { row: at.row, col: drawn_len(line) };
                (TextPos { row: at.row, col: 0 }, to, Unit::Line(at.row))
            }
            _ => (at, at, Unit::Character),
        };
        self.selection = Some(TextSelection { which, anchor, head, unit });
        self.selecting = true;
    }

    /// Where a drag to `at` takes a selection that goes by `unit`, from what the clicks took:
    /// characters reach `at`; words and lines cover the first one and the one at `at`, never
    /// cut (as the editor's).
    fn extended(&self, sel: TextSelection, at: TextPos) -> (TextPos, TextPos) {
        let line_end =
            |row: usize| TextPos { row, col: self.line_in(row, sel.which).map_or(0, drawn_len) };
        match sel.unit {
            Unit::Character => (sel.anchor, at),
            Unit::Word(from, to) => {
                let line = self.line_in(at.row, sel.which).unwrap_or_default();
                let (a, b) = word_at(line, at.col);
                let (start, end) =
                    (TextPos { row: at.row, col: a }, TextPos { row: at.row, col: b });
                if start < from { (to, start) } else { (from, end.max(to)) }
            }
            Unit::Line(row) if at.row < row => (line_end(row), TextPos { row: at.row, col: 0 }),
            Unit::Line(row) => (TextPos { row, col: 0 }, line_end(at.row)),
        }
    }

    /// The mouse dragged while selecting: the selection reaches it (in its column).
    pub(crate) fn drag_select(&mut self, p: Point) -> bool {
        let Some(mut sel) = self.selection.filter(|_| self.selecting) else { return false };
        let (anchor, head) = self.extended(sel, self.text_pos(sel.which, p));
        if (anchor, head) == (sel.anchor, sel.head) {
            return false;
        }
        (sel.anchor, sel.head) = (anchor, head);
        self.selection = Some(sel);
        true
    }

    /// Column `which` selected whole.
    pub(crate) fn select_column(&mut self, which: usize) {
        let last = self.rows.len().saturating_sub(1);
        let end = self.line_in(last, which).map_or(0, drawn_len);
        self.selection = Some(TextSelection {
            which,
            anchor: TextPos::default(),
            head: TextPos { row: last, col: end },
            unit: Unit::Character,
        });
    }

    /// The selected text, as in its file (the rows of the other side's lines left out).
    pub(crate) fn selection_text(&self) -> Option<String> {
        let sel = self.selection.filter(|s| !s.is_empty())?;
        let (from, to) = sel.ordered();
        let mut lines = Vec::new();
        for row in from.row..=to.row {
            let Some(line) = self.line_in(row, sel.which) else { continue };
            let start = if row == from.row { byte_at(line, from.col) } else { 0 };
            let end = if row == to.row { byte_at(line, to.col) } else { line.len() };
            lines.push(&line[start.min(end)..end]);
        }
        Some(lines.join("\n")).filter(|s| !s.is_empty())
    }

    /// Column `which`'s selection in its code layer (under the text), rows `first`.. shown.
    pub(crate) fn paint_selection(
        &self,
        layer: &mut Layer,
        ds: &Ctx<'_>,
        which: usize,
        grid: &Grid,
        rows: &[Row],
    ) {
        let Some(sel) = self.selection.filter(|s| s.which == which && !s.is_empty()) else {
            return;
        };
        let (from, to) = sel.ordered();
        let color = ds.palette.selection;
        let x0 = self.code_x[which] - self.across[which].offset;
        let shown = grid.first.max(from.row)..(grid.first + grid.visible).min(to.row + 1);
        for row in shown {
            let Some(line) = self.line_of(rows, row, which) else { continue };
            let len = drawn_len(line);
            let a = if row == from.row { from.col } else { 0 };
            // A line selected past its end: a column more (its line break).
            let b = if row == to.row { to.col } else { len + 1 };
            if b <= a {
                continue;
            }
            let top = (self.list.min_y() + self.row_top(row) - self.scroll).round();
            // Each of its rows its share (a wrapped line's), the last one past its end.
            let parts = self.part_columns(line, which);
            for (k, &(from, to)) in parts.iter().enumerate() {
                let to = if k + 1 == parts.len() { to + 1 } else { to };
                let (a, b) = (a.max(from), b.min(to));
                if b <= a {
                    continue;
                }
                let r = Rect::new(
                    (x0 + (a - from) as f32 * self.advance).round(),
                    top + k as f32 * self.row_h,
                    ((b - a) as f32 * self.advance).round(),
                    self.row_h,
                );
                layer.quad(Quad::filled(r, color));
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/select.rs"]
mod tests;
