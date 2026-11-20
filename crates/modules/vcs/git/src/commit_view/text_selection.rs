//! Selecting the code of the changes with the mouse, to copy it. Columns as shown (a tab is
//! four); the numbers beside it choose lines, the code is for this.

use super::*;

/// A column as many as a tab takes.
const TAB: usize = 4;

/// A place in the changes: a row of the pane, a column of its code.
type Place = (usize, usize);

/// Where the code was last painted.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CodeArea {
    pub(super) area: Rect,
    pub(super) left: f32,
    pub(super) row_h: f32,
    pub(super) column: f32,
    pub(super) scroll: f32,
    pub(super) sideways: f32,
}

#[derive(Debug, Default)]
pub(super) struct CodeSelection {
    at: CodeArea,
    /// Where it started and where it reaches.
    range: Option<(Place, Place)>,
    dragging: bool,
}

/// The columns `text` takes.
pub(super) fn columns(text: &str) -> usize {
    text.chars().map(|c| if c == '\t' { TAB } else { 1 }).sum()
}

/// The byte of `text` at column `column`.
fn byte_at(text: &str, column: usize) -> usize {
    let mut seen = 0;
    for (at, c) in text.char_indices() {
        if seen >= column {
            return at;
        }
        seen += if c == '\t' { TAB } else { 1 };
    }
    text.len()
}

impl CodeSelection {
    pub(super) fn place(&mut self, at: CodeArea) {
        self.at = at;
    }

    pub(super) fn clear(&mut self) {
        self.range = None;
        self.dragging = false;
    }

    pub(super) fn is_over(&self, p: Point) -> bool {
        let a = self.at;
        self.dragging || (a.area.contains(p) && p.x >= a.left && a.column > 0.0)
    }

    fn place_at(&self, p: Point) -> Option<Place> {
        let a = self.at;
        if a.row_h <= 0.0 || a.column <= 0.0 {
            return None;
        }
        let row = ((p.y - a.area.min_y() + a.scroll) / a.row_h).max(0.0) as usize;
        let column = ((p.x - a.left + a.sideways) / a.column).round().max(0.0) as usize;
        Some((row, column))
    }

    /// The highlight of row `row`, its code `width` columns wide, between `edges`.
    pub(super) fn band(&self, row: usize, width: usize, edges: (f32, f32)) -> Option<Rect> {
        let (from, to) = self.ordered()?;
        if row < from.0 || row > to.0 {
            return None;
        }
        let first = if row == from.0 { from.1.min(width) } else { 0 };
        // A row followed by more of it shows its line break too.
        let last = if row == to.0 { to.1.min(width) } else { width + 1 };
        if first >= last {
            return None;
        }
        let a = self.at;
        let x = |column: usize| a.left - a.sideways + column as f32 * a.column;
        let (start, end) = (x(first).max(edges.0), x(last).min(edges.1));
        let top = a.area.min_y() + row as f32 * a.row_h - a.scroll;
        (start < end).then(|| Rect::new(start, top, end - start, a.row_h))
    }

    fn ordered(&self) -> Option<(Place, Place)> {
        let (a, b) = self.range?;
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// Ended the drag: `true` if there was one.
    pub(super) fn release(&mut self) -> bool {
        if let Some((a, b)) = self.range
            && a == b
        {
            self.range = None;
        }
        std::mem::take(&mut self.dragging)
    }
}

/// The changes' code as a form's control: over it the cursor is the text's; a press on a
/// line of code starts a selection (held until let go), a drag reaches further.
pub(super) struct CodeText<'a> {
    pub(super) code: &'a mut CodeSelection,
    pub(super) diff: Option<&'a FileDiff>,
}

impl Control for CodeText<'_> {
    fn hit(&self, p: Point) -> bool {
        self.code.is_over(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> ui_components::control::Press {
        use ui_components::control::Press;
        let row = self.code.place_at(p).map(|(row, _)| row);
        let on_row = self.diff.is_some_and(|diff| {
            row.is_some_and(|row| matches!(diff.rows.get(row), Some(DiffRow::Line(..))))
        });
        self.code.clear();
        if !on_row || !self.code.is_over(p) {
            return Press::Ignored;
        }
        self.code.range = self.code.place_at(p).map(|at| (at, at));
        self.code.dragging = true;
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        if !self.code.dragging {
            return false;
        }
        if let (Some(reach), Some(range)) = (self.code.place_at(p), &mut self.code.range) {
            range.1 = reach;
        }
        true
    }

    fn release(&mut self) -> bool {
        self.code.release()
    }

    fn cursor(&self, _p: Point) -> Option<ui_components::ViewerCursor> {
        Some(ui_components::ViewerCursor::Text)
    }
}

impl CommitView {
    /// The code selected, its line breaks included; hunk headers aren't code.
    pub(super) fn selected_code(&self) -> Option<String> {
        let (from, to) = self.code.ordered()?;
        let diff = self.diff.as_ref()?;
        let mut out = Vec::new();
        for row in from.0..=to.0 {
            let Some(DiffRow::Line(h, l)) = diff.rows.get(row) else { continue };
            let line = diff.hunks[*h].lines[*l].text.trim_end_matches(['\n', '\r']);
            let start = if row == from.0 { byte_at(line, from.1) } else { 0 };
            let end = if row == to.0 { byte_at(line, to.1) } else { line.len() };
            out.push(line[start.min(end)..end].to_owned());
        }
        let text = out.join("\n");
        (!text.is_empty()).then_some(text)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/commit_view/text_selection.rs"]
mod tests;
