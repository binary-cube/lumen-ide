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
}

impl TextSelection {
    fn ordered(&self) -> (TextPos, TextPos) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// How wide `line` is drawn, in columns (a tab as four).
fn drawn_len(line: &str) -> usize {
    line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// The byte where drawn column `col` starts in `line` (a column inside a tab: the tab's).
fn byte_at(line: &str, col: usize) -> usize {
    let mut at = 0;
    for (i, c) in line.char_indices() {
        let w = if c == '\t' { 4 } else { 1 };
        if at + w > col {
            return i;
        }
        at += w;
    }
    line.len()
}

/// The drawn columns of the word around `col` (letters, digits, `_`, `$`).
fn word_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let drawn =
        |i: usize| -> usize { chars[..i].iter().map(|c| if *c == '\t' { 4 } else { 1 }).sum() };
    // The character under the column.
    let mut at = 0;
    let mut i = 0;
    while i < chars.len() {
        let w = if chars[i] == '\t' { 4 } else { 1 };
        if at + w > col {
            break;
        }
        at += w;
        i += 1;
    }
    if i >= chars.len() || !word(chars[i]) {
        return (col, col);
    }
    let (mut a, mut b) = (i, i + 1);
    while a > 0 && word(chars[a - 1]) {
        a -= 1;
    }
    while b < chars.len() && word(chars[b]) {
        b += 1;
    }
    (drawn(a), drawn(b))
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
    fn text_pos(&self, which: usize, p: Point) -> TextPos {
        let last = self.rows.len().saturating_sub(1);
        let row = ((p.y - self.list.min_y() + self.scroll) / self.row_h.max(1.0)).floor();
        let row = (row.max(0.0) as usize).min(last);
        let x = p.x - self.code_x[which] + self.across[which].offset;
        let col = (x / self.advance.max(1.0)).round().max(0.0) as usize;
        let len = self.line_in(row, which).map_or(0, drawn_len);
        TextPos { row, col: col.min(len) }
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
        let (anchor, head) = match clicks {
            2 => {
                let (a, b) = word_at(line, at.col);
                (TextPos { row: at.row, col: a }, TextPos { row: at.row, col: b })
            }
            c if c >= 3 => {
                (TextPos { row: at.row, col: 0 }, TextPos { row: at.row, col: drawn_len(line) })
            }
            _ => (at, at),
        };
        self.selection = Some(TextSelection { which, anchor, head });
        self.selecting = true;
    }

    /// The mouse dragged while selecting: the selection reaches it (in its column).
    pub(crate) fn drag_select(&mut self, p: Point) -> bool {
        let Some(mut sel) = self.selection.filter(|_| self.selecting) else { return false };
        let head = self.text_pos(sel.which, p);
        if head == sel.head {
            return false;
        }
        sel.head = head;
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
            let y = (self.list.min_y() + row as f32 * self.row_h - self.scroll).round();
            let r = Rect::new(
                (x0 + a as f32 * self.advance).round(),
                y,
                ((b - a) as f32 * self.advance).round(),
                self.row_h,
            );
            layer.quad(Quad::filled(r, color));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/select.rs"]
mod tests;
