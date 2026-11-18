//! A small multi-line text field, for the grid's large value editor (a long text, JSON): a
//! caret moved by characters, words, lines and the text's ends, a selection extended
//! with ⇧, line breaks typed, the lines drawn scrolled to keep the caret in view.

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text_within;

/// The text and where the caret is (byte offsets on character boundaries).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextArea {
    text: String,
    cursor: usize,
    anchor: usize,
    /// The first line shown.
    pub top: usize,
}

impl TextArea {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_owned(), cursor: text.len(), anchor: text.len(), top: 0 }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// `typed` in place of the selection (line breaks kept).
    pub fn insert(&mut self, typed: &str) {
        let range = self.selection();
        let typed = typed.replace("\r\n", "\n").replace('\r', "\n");
        self.text.replace_range(range.clone(), &typed);
        self.cursor = range.start + typed.len();
        self.anchor = self.cursor;
    }

    fn prev(&self, at: usize) -> usize {
        self.text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next(&self, at: usize) -> usize {
        self.text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
    }

    /// The start of the word before `at` (letters, digits, `_`).
    fn word_back(&self, at: usize) -> usize {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut i = at;
        while i > 0 && !self.text[..i].chars().next_back().is_some_and(word) {
            i = self.prev(i);
        }
        while i > 0 && self.text[..i].chars().next_back().is_some_and(word) {
            i = self.prev(i);
        }
        i
    }

    fn word_forward(&self, at: usize) -> usize {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut i = at;
        while i < self.text.len() && !self.text[i..].chars().next().is_some_and(word) {
            i = self.next(i);
        }
        while i < self.text.len() && self.text[i..].chars().next().is_some_and(word) {
            i = self.next(i);
        }
        i
    }

    pub fn backspace(&mut self, word: bool) {
        if self.cursor == self.anchor {
            self.anchor = if word { self.word_back(self.cursor) } else { self.prev(self.cursor) };
        }
        self.insert("");
    }

    pub fn delete(&mut self) {
        if self.cursor == self.anchor {
            self.anchor = self.next(self.cursor);
        }
        self.insert("");
    }

    fn place(&mut self, at: usize, extend: bool) {
        self.cursor = at;
        if !extend {
            self.anchor = at;
        }
    }

    pub fn left(&mut self, word: bool, extend: bool) {
        let at = if word { self.word_back(self.cursor) } else { self.prev(self.cursor) };
        self.place(at, extend);
    }

    pub fn right(&mut self, word: bool, extend: bool) {
        let at = if word { self.word_forward(self.cursor) } else { self.next(self.cursor) };
        self.place(at, extend);
    }

    /// The line `at` is on: its start and end.
    fn line_of(&self, at: usize) -> (usize, usize) {
        let start = self.text[..at].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[at..].find('\n').map_or(self.text.len(), |i| at + i);
        (start, end)
    }

    /// The start of the line (`far`: of the text).
    pub fn home(&mut self, far: bool, extend: bool) {
        let at = if far { 0 } else { self.line_of(self.cursor).0 };
        self.place(at, extend);
    }

    pub fn end(&mut self, far: bool, extend: bool) {
        let at = if far { self.text.len() } else { self.line_of(self.cursor).1 };
        self.place(at, extend);
    }

    /// One line up or down, at the same column (in characters) when it's long enough.
    pub fn vertical(&mut self, down: bool, extend: bool) {
        let (start, end) = self.line_of(self.cursor);
        let column = self.text[start..self.cursor].chars().count();
        let target = if down {
            if end >= self.text.len() {
                self.place(self.text.len(), extend);
                return;
            }
            end + 1
        } else {
            if start == 0 {
                self.place(0, extend);
                return;
            }
            self.line_of(start - 1).0
        };
        let (line_start, line_end) = self.line_of(target);
        let at = self.text[line_start..line_end]
            .char_indices()
            .nth(column)
            .map_or(line_end, |(i, _)| line_start + i);
        self.place(at, extend);
    }

    /// The line and column (bytes) of `at`.
    pub fn position(&self, at: usize) -> (usize, usize) {
        let line = self.text[..at].matches('\n').count();
        (line, at - self.line_of(at).0)
    }

    /// Draws it in `area` (`font`, lines `line_h` apart), the selection and the caret, and
    /// scrolls to keep the caret in view.
    /// In `area`, with `font` in lines `line_h` tall.
    pub fn paint(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        area: Rect,
        (font, line_h): (&Font, f32),
        colors: (Rgba, Rgba, Rgba),
        caret_w: f32,
    ) {
        let (color, selection, caret) = colors;
        let visible = ((area.size.height / line_h.max(1.0)).floor() as usize).max(1);
        let (caret_line, _) = self.position(self.cursor);
        if caret_line < self.top {
            self.top = caret_line;
        } else if caret_line >= self.top + visible {
            self.top = caret_line + 1 - visible;
        }
        let metrics = text.metrics(font);
        let range = self.selection();
        let mut offset = 0;
        for (i, line) in self.text.split('\n').enumerate() {
            let start = offset;
            offset += line.len() + 1;
            if i < self.top || i >= self.top + visible {
                continue;
            }
            let top = area.min_y() + (i - self.top) as f32 * line_h;
            let baseline = ui_kit::centered_baseline(top, line_h, &metrics);
            let shaped = text.shape_line(line, font);
            // The selection on this line.
            let (a, b) = (range.start.max(start), range.end.min(start + line.len()));
            if a < b || (range.start < start && range.end > start + line.len()) {
                let x0 = area.min_x() + shaped.x_at(a.saturating_sub(start).min(line.len()));
                let x1 = area.min_x() + shaped.x_at(b.saturating_sub(start).min(line.len()));
                let x1 =
                    if range.end > start + line.len() { x1 + metrics.advance * 0.5 } else { x1 };
                layer.quad(Quad::filled(Rect::new(x0, top, (x1 - x0).max(1.0), line_h), selection));
            }
            draw_text_within(
                layer,
                &shaped,
                area.min_x(),
                baseline,
                color,
                (area.min_x(), area.max_x()),
            );
            if (start..=start + line.len()).contains(&self.cursor) {
                let x = area.min_x() + shaped.x_at(self.cursor - start);
                layer.quad(Quad::filled(
                    Rect::new(x.round(), top + 2.0, caret_w, line_h - 4.0),
                    caret,
                ));
            }
        }
    }

    /// The caret placed at `p` (inside `area`, as last painted with `line_h`).
    pub fn click(&mut self, text: &mut TextSystem, p: Point, area: Rect, font: &Font, line_h: f32) {
        let line = self.top + ((p.y - area.min_y()) / line_h.max(1.0)).floor().max(0.0) as usize;
        let mut offset = 0;
        for (i, l) in self.text.split('\n').enumerate() {
            if i == line {
                let shaped = text.shape_line(l, font);
                let at = shaped.byte_at(p.x - area.min_x(), l.len());
                self.place(offset + at, false);
                return;
            }
            offset += l.len() + 1;
        }
        self.place(self.text.len(), false);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/text_area.rs"]
mod tests;
