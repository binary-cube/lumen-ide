//! Single-line text field (search, replace, later command palettes).

use std::ops::Range;

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};

use crate::{centered_baseline, draw_text_within};

#[derive(Debug, Default)]
pub struct TextInput {
    text: String,
    /// Byte offsets, always on character boundaries.
    cursor: usize,
    anchor: usize,
    /// A secret (a password): shown as dots, never copied.
    masked: bool,
    /// Was ever masked: its text is wiped from memory when replaced or dropped.
    secret: bool,
    /// Where the last paint put the field and each character boundary (for clicks).
    painted: std::cell::RefCell<Painted>,
    /// What ⌘Z / ⇧⌘Z go back and forth through (never kept for a secret).
    history: History,
}

/// A field's text and selection, as it was before an edit.
type Snapshot = (String, usize, usize);

/// The field's undo and redo (typing in a row is one step).
#[derive(Debug, Clone, Default)]
struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The last edit typed text ending here (the next character typed joins its step).
    typing_at: Option<usize>,
}

/// The steps a field remembers.
const HISTORY: usize = 100;

/// The field's area and its character boundaries' x, as last painted.
#[derive(Debug, Clone, Default)]
struct Painted {
    bounds: Rect,
    stops: Vec<(usize, f32)>,
}

/// The room a secret field's text gets up front (a long passphrase fits).
const SECRET_ROOM: usize = 256;

impl Clone for TextInput {
    /// A secret's copy keeps its room (typing into it never leaves copies behind).
    fn clone(&self) -> Self {
        let mut text = String::with_capacity(if self.secret { self.text.capacity() } else { 0 });
        text.push_str(&self.text);
        Self {
            text,
            cursor: self.cursor,
            anchor: self.anchor,
            masked: self.masked,
            secret: self.secret,
            painted: self.painted.clone(),
            history: self.history.clone(),
        }
    }
}

impl Drop for TextInput {
    fn drop(&mut self) {
        if self.secret {
            kernel::secret::wipe(&mut self.text);
        }
    }
}

impl PartialEq for TextInput {
    fn eq(&self, other: &Self) -> bool {
        (&self.text, self.cursor, self.anchor) == (&other.text, other.cursor, other.anchor)
    }
}

impl Eq for TextInput {}

impl TextInput {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            cursor: text.len(),
            anchor: text.len(),
            masked: false,
            secret: false,
            painted: std::cell::RefCell::default(),
            history: History::default(),
        }
    }

    /// A secret field (a password): its text shows as dots and can't be copied.
    pub fn set_masked(&mut self, masked: bool) {
        self.masked = masked;
        if masked && !self.secret {
            self.secret = true;
            self.history = History::default();
            // Room up front: growing while typed would leave copies of the secret behind.
            let mut text = String::with_capacity(self.text.len().max(SECRET_ROOM));
            text.push_str(&self.text);
            kernel::secret::wipe(&mut std::mem::replace(&mut self.text, text));
        }
    }

    /// Shown as dots now (a password field's eye toggles it).
    pub fn is_masked(&self) -> bool {
        self.masked
    }

    /// `p` is on the field (as last painted).
    pub fn contains(&self, p: kernel::Point) -> bool {
        self.painted.borrow().bounds.contains(p)
    }

    /// The cursor's area as last painted (the system places its input method's popups and
    /// the Caps Lock indicator there); `None` before the first paint.
    pub fn caret_area(&self) -> Option<Rect> {
        let painted = self.painted.borrow();
        let x = painted.stops.iter().find(|(i, _)| *i == self.cursor).map(|(_, x)| *x)?;
        let b = painted.bounds;
        Some(Rect::new(x, b.min_y() + b.size.height * 0.2, 1.0, b.size.height * 0.6))
    }

    /// A click at `x` (physical pixels): the cursor goes to the nearest character
    /// boundary, nothing selected (a click in a fully selected field doesn't keep it).
    pub fn click(&mut self, x: f32) {
        let offset = self.offset_at(x);
        self.cursor = offset;
        self.anchor = offset;
        self.history.typing_at = None;
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    pub fn selected_text(&self) -> &str {
        if self.masked {
            return "";
        }
        &self.text[self.selection()]
    }

    /// Replaces all the text and puts the cursor at the end, with all the text selected.
    pub fn set_text(&mut self, text: &str) {
        if text != self.text {
            self.remember(false);
        }
        if self.secret {
            kernel::secret::wipe(&mut self.text);
            self.text.push_str(text);
        } else {
            self.text = text.to_owned();
        }
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Inserts text (without `\n`), replacing the selection.
    pub fn insert(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        let range = self.selection();
        if range.is_empty() && clean.is_empty() {
            return;
        }
        // Characters typed one after the other are one step; anything else starts one.
        let typing = range.is_empty() && clean.chars().count() == 1;
        self.remember(typing);
        // A secret outgrowing its room moves to a larger one, the old wiped (growing in
        // place would leave a copy behind).
        let needed = self.text.len() - range.len() + clean.len();
        if self.secret && needed > self.text.capacity() {
            let mut larger = String::with_capacity(needed.max(self.text.capacity() * 2));
            larger.push_str(&self.text);
            kernel::secret::wipe(&mut std::mem::replace(&mut self.text, larger));
        }
        self.text.replace_range(range.clone(), &clean);
        self.cursor = range.start + clean.len();
        self.anchor = self.cursor;
        if typing && !self.secret {
            self.history.typing_at = Some(self.cursor);
        }
    }

    /// The text before an edit kept for undo (`typing`: joined to the step typed just
    /// before, when it goes on from there). A secret keeps none.
    fn remember(&mut self, typing: bool) {
        if self.secret {
            return;
        }
        let joined = typing && self.history.typing_at == Some(self.cursor);
        self.history.typing_at = None;
        self.history.redo.clear();
        if joined {
            return;
        }
        if self.history.undo.len() >= HISTORY {
            self.history.undo.remove(0);
        }
        self.history.undo.push((self.text.clone(), self.cursor, self.anchor));
    }

    /// Back one step (⌘Z). `false`: nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(before) = self.history.undo.pop() else { return false };
        let now = (std::mem::replace(&mut self.text, before.0), self.cursor, self.anchor);
        (self.cursor, self.anchor) = (before.1, before.2);
        self.history.redo.push(now);
        self.history.typing_at = None;
        true
    }

    /// Forward one step (⇧⌘Z). `false`: nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(after) = self.history.redo.pop() else { return false };
        let now = (std::mem::replace(&mut self.text, after.0), self.cursor, self.anchor);
        (self.cursor, self.anchor) = (after.1, after.2);
        self.history.undo.push(now);
        self.history.typing_at = None;
        true
    }

    /// The nearest character boundary to `x` (physical pixels), as last painted.
    fn offset_at(&self, x: f32) -> usize {
        let painted = self.painted.borrow();
        let offset = painted
            .stops
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(self.text.len(), |(i, _)| *i);
        offset.min(self.text.len())
    }

    /// A ⇧-click, or the mouse dragged with the button down, at `x`: the selection
    /// stretched to there from where it started.
    pub fn extend_to(&mut self, x: f32) {
        self.cursor = self.offset_at(x);
        self.history.typing_at = None;
    }

    /// A press at `x` with `clicks` in a row: one puts the cursor there, two select the
    /// word under it, three all the text.
    pub fn press(&mut self, x: f32, clicks: u32) {
        match clicks {
            0 | 1 => self.click(x),
            2 => {
                let at = self.offset_at(x);
                let word = |c: char| c.is_alphanumeric() || c == '_';
                let start = self.text[..at]
                    .char_indices()
                    .rev()
                    .take_while(|(_, c)| word(*c))
                    .last()
                    .map_or(at, |(i, _)| i);
                let end = self.text[at..]
                    .char_indices()
                    .find(|(_, c)| !word(*c))
                    .map_or(self.text.len(), |(i, _)| at + i);
                self.anchor = start;
                self.cursor = end;
            }
            _ => self.select_all(),
        }
        self.history.typing_at = None;
    }

    pub fn backspace(&mut self, word: bool) {
        if self.selection().is_empty() {
            self.anchor = if word { self.word_left() } else { self.prev_char() };
        }
        self.insert("");
    }

    pub fn delete(&mut self) {
        if self.selection().is_empty() {
            self.anchor = self.next_char();
        }
        self.insert("");
    }

    pub fn move_left(&mut self, word: bool, extend: bool) {
        let target = if !extend && !self.selection().is_empty() {
            self.selection().start
        } else if word {
            self.word_left()
        } else {
            self.prev_char()
        };
        self.move_to(target, extend);
    }

    pub fn move_right(&mut self, word: bool, extend: bool) {
        let target = if !extend && !self.selection().is_empty() {
            self.selection().end
        } else if word {
            self.word_right()
        } else {
            self.next_char()
        };
        self.move_to(target, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.move_to(self.text.len(), extend);
    }

    /// Selects the given range (in bytes, aligned to characters by the caller).
    pub fn select(&mut self, range: std::ops::Range<usize>) {
        self.anchor = range.start.min(self.text.len());
        self.cursor = range.end.min(self.text.len());
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    fn move_to(&mut self, offset: usize, extend: bool) {
        self.history.typing_at = None;
        self.cursor = offset;
        if !extend {
            self.anchor = offset;
        }
    }

    fn prev_char(&self) -> usize {
        self.text[..self.cursor].char_indices().last().map_or(0, |(i, _)| i)
    }

    fn next_char(&self) -> usize {
        self.text[self.cursor..].chars().next().map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    fn word_left(&self) -> usize {
        let before = self.text[..self.cursor].trim_end();
        before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or_else(|| before.char_indices().last().map_or(0, |(i, _)| i), |(i, _)| i)
    }

    fn word_right(&self) -> usize {
        let after = &self.text[self.cursor..];
        let skipped = after.len() - after.trim_start().len();
        let rest = &after[skipped..];
        let word = rest
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or_else(
                || rest.chars().next().map_or(0, char::len_utf8),
                |(i, c)| i + c.len_utf8(),
            );
        self.cursor + skipped + word
    }

    /// Draws the field in `bounds`. Long text scrolls so the cursor stays visible.
    /// What it is to a screen reader: a field named by its placeholder, its text as its
    /// value. A secret's (a password, even shown) has none: its text never leaves the field
    /// (no copy kept by the tree, by the system).
    fn described(
        &self,
        bounds: Rect,
        focused: bool,
        placeholder: &str,
    ) -> crate::access::Described {
        use crate::access::{Described, Role, TextLines};
        let field = Described::new(Role::TextField, placeholder, bounds).focused(focused);
        if self.secret || self.masked {
            return field;
        }
        let lines = TextLines {
            lines: vec![bounds],
            anchor: self.anchor,
            focus: self.cursor,
            caret: self.caret_area(),
        };
        field.value(self.text.as_str()).lines(lines)
    }

    pub fn paint(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        bounds: Rect,
        focused: bool,
        placeholder: &str,
        style: &TextInputStyle,
    ) {
        crate::access::node(|| self.described(bounds, focused, placeholder));
        let s = style;
        let border = if focused { s.border_focused } else { s.border };
        layer.quad(
            Quad::filled(bounds, s.background).rounded(s.radius).bordered(s.border_width, border),
        );

        let metrics = text.metrics(&s.font);
        let baseline = centered_baseline(bounds.min_y(), bounds.size.height, &metrics);
        let inner_width = (bounds.size.width - 2.0 * s.padding - s.lead - s.trail).max(0.0);
        // The text's room: inside the padding, clear of what's before (`lead`) and after it
        // (`trail`: a button inside the field).
        let inside =
            (bounds.min_x() + s.padding * 0.5 + s.lead, bounds.max_x() - s.padding * 0.5 - s.trail);
        if self.text.is_empty() {
            let shaped = text.shape_line(placeholder, &s.font);
            let x = bounds.min_x() + s.padding + s.lead;
            draw_text_within(layer, &shaped, x, baseline, s.placeholder, inside);
        }

        // A secret: a dot per character (the offsets follow).
        const DOT: char = '•';
        let shown: std::borrow::Cow<'_, str> = if self.masked {
            std::borrow::Cow::Owned(std::iter::repeat_n(DOT, self.text.chars().count()).collect())
        } else {
            std::borrow::Cow::Borrowed(&self.text)
        };
        let at = |i: usize| {
            if self.masked { self.text[..i].chars().count() * DOT.len_utf8() } else { i }
        };
        let shaped = text.shape_line(&shown, &s.font);
        let caret_x = shaped.x_at(at(self.cursor));
        // Scrolled to the caret while typing; from its start otherwise.
        let scroll = if focused { (caret_x - inner_width + s.caret_width).max(0.0) } else { 0.0 };
        let x0 = (bounds.min_x() + s.padding + s.lead - scroll).round();

        let sel = self.selection();
        if focused && !sel.is_empty() {
            // Within the field, as the text is: a long text scrolled to its caret starts far
            // to the field's left, and its selection with it.
            let a = (x0 + shaped.x_at(at(sel.start))).max(inside.0);
            let b = (x0 + shaped.x_at(at(sel.end))).min(inside.1);
            if b > a {
                let top = baseline - metrics.ascent - 2.0;
                let height = metrics.ascent + metrics.descent + 4.0;
                layer.quad(Quad::filled(Rect::new(a, top, b - a, height), s.selection));
            }
        }
        draw_text_within(layer, &shaped, x0, baseline, s.text, inside);
        {
            let mut painted = self.painted.borrow_mut();
            painted.bounds = bounds;
            painted.stops.clear();
            let offsets = self.text.char_indices().map(|(i, _)| i).chain([self.text.len()]);
            if shaped.glyphs.is_sorted_by_key(|g| g.start) {
                // In one pass, the glyphs walked with the characters (their starts in order):
                // not a search of every glyph for each character (a long cell: tens of ms).
                let mut g = 0;
                for (n, i) in offsets.enumerate() {
                    let shown = if self.masked { n * DOT.len_utf8() } else { i };
                    while shaped.glyphs.get(g).is_some_and(|gl| gl.start < shown) {
                        g += 1;
                    }
                    let x = shaped.glyphs.get(g).map_or(shaped.width, |gl| gl.x);
                    painted.stops.push((i, x0 + x));
                }
            } else {
                painted.stops.extend(offsets.map(|i| (i, x0 + shaped.x_at(at(i)))));
            }
        }
        if focused {
            // A field's caret is a bar (blinking, glowing, gliding as the look says).
            let top = baseline - metrics.ascent - 1.0;
            let height = metrics.ascent + metrics.descent + 2.0;
            crate::caret::paint(
                layer,
                crate::caret::CaretPaint {
                    id: std::ptr::from_ref(self) as u64,
                    cell: Rect::new((x0 + caret_x).round(), top, s.caret_width, height),
                    color: s.caret,
                    scale: 1.0,
                    shape: Some(theming::caret::CaretShape::Bar),
                    thickness: Some(s.caret_width),
                    steady: false,
                },
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextInputStyle {
    pub font: Font,
    pub background: Rgba,
    pub border: Rgba,
    pub border_focused: Rgba,
    pub text: Rgba,
    pub placeholder: Rgba,
    pub selection: Rgba,
    pub caret: Rgba,
    pub radius: f32,
    pub border_width: f32,
    pub padding: f32,
    pub caret_width: f32,
    /// More room before the text (an icon there, drawn by the field's owner).
    pub lead: f32,
    /// Room kept free after the text (a button inside the field, drawn by its owner: ×,
    /// Send): the text, its selection and the caret scroll within the rest.
    pub trail: f32,
}

#[cfg(test)]
#[path = "../tests/unit/text_input.rs"]
mod tests;
