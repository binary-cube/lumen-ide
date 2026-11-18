//! Editing a declaration directly on the canvas: the selected card becomes a small editor
//! (cursor, typing, Enter with indentation, arrows). On Esc or a click outside, the new text
//! is sent to the app, which writes it to the file (in its tab, as an unsaved change).

use std::ops::RangeInclusive;
use std::path::PathBuf;

use document::{Document, Motion, commands, visual_width};
use kernel::Point;
use ui_components::{ViewerAction, ViewerKey};

/// Where the code is drawn (from the last paint), for clicks.
#[derive(Debug, Clone, Copy, Default)]
pub struct EditArea {
    pub x0: f32,
    pub top: f32,
    pub row_h: f32,
    pub cell: f32,
}

#[derive(Debug)]
pub struct CodeEdit {
    pub symbol: usize,
    pub doc: Document,
    path: PathBuf,
    lines: RangeInclusive<u32>,
    original: String,
    pub area: Option<EditArea>,
}

impl CodeEdit {
    pub fn new(symbol: usize, path: PathBuf, first_line: u32, code: &[String]) -> Self {
        let text = code.join("\n");
        let last = first_line + code.len().saturating_sub(1) as u32;
        Self {
            symbol,
            doc: Document::new(&text),
            path,
            lines: first_line..=last,
            original: text,
            area: None,
        }
    }

    pub fn line_count(&self) -> usize {
        self.doc.buffer().line_count()
    }

    pub fn insert(&mut self, text: &str) {
        commands::insert_text(&mut self.doc, text);
    }

    pub fn key(&mut self, key: ViewerKey) -> bool {
        let doc = &mut self.doc;
        match key {
            ViewerKey::Backspace => commands::backspace(doc),
            ViewerKey::Delete => commands::delete_forward(doc),
            ViewerKey::Enter => commands::newline(doc),
            ViewerKey::Tab => commands::insert_text(doc, "    "),
            ViewerKey::Left => commands::move_cursors(doc, Motion::Left, false),
            ViewerKey::Right => commands::move_cursors(doc, Motion::Right, false),
            ViewerKey::Up => commands::move_cursors(doc, Motion::Up, false),
            ViewerKey::Down => commands::move_cursors(doc, Motion::Down, false),
            ViewerKey::Home => commands::move_cursors(doc, Motion::LineStart, false),
            ViewerKey::End => commands::move_cursors(doc, Motion::LineEnd, false),
            ViewerKey::Escape | ViewerKey::PageUp | ViewerKey::PageDown => return false,
        }
        true
    }

    /// Cursor at point `p` (in the code area).
    pub fn click(&mut self, p: Point) {
        let Some(area) = self.area else { return };
        let buffer = self.doc.buffer();
        let line = (((p.y - area.top) / area.row_h).floor().max(0.0) as usize)
            .min(buffer.line_count().saturating_sub(1));
        let column = ((p.x - area.x0) / area.cell).round().max(0.0) as u32;
        let start = buffer.line_start(line);
        let text = buffer.slice(start..buffer.line_end(line));
        let mut offset = start + text.len();
        let mut width = 0;
        for (i, c) in text.char_indices() {
            if width >= column {
                offset = start + i;
                break;
            }
            width = visual_width(&text[..i + c.len_utf8()], 4);
        }
        commands::click(&mut self.doc, offset, false, false);
    }

    /// (row, visual column) of the cursor.
    pub fn caret(&self) -> (usize, u32) {
        let buffer = self.doc.buffer();
        let head = self.doc.selections().primary().head;
        let line = buffer.line_of(head);
        let prefix = buffer.slice(buffer.line_start(line)..head);
        (line, visual_width(&prefix, 4))
    }

    /// Editing finished: the new text, if anything changed.
    pub fn finish(self) -> Option<ViewerAction> {
        let text = self.doc.buffer().text();
        (text != self.original).then_some(ViewerAction::Replace {
            path: self.path,
            lines: self.lines,
            text,
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/edit.rs"]
mod tests;
