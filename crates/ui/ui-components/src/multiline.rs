//! A multi-line text field (a commit's message): its text wrapped at words to the field's
//! width, a caret moved by characters, words, visual lines and the text's ends, a
//! selection (⇧, a drag, a double click's word, ⌘A), line breaks typed, undo / redo, and its
//! own scroll with the auto-hiding scrollbar once it holds more than its tallest. It grows
//! between a least and a most number of lines ([`MultilineField::height`]).
//!
//! It's laid out when painted (the text system measures): the keys and the mouse use the
//! last layout — the lines and where each character sits on them.

use std::ops::Range;

use kernel::{Point, Rect};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

use crate::EdgeScrollbar;
use crate::ds::{self, Ctx, radius};

/// A state to go back to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    text: String,
    cursor: usize,
    anchor: usize,
}

/// The most undo steps kept.
const HISTORY: usize = 200;

#[derive(Debug, Default)]
pub struct MultilineField {
    text: String,
    cursor: usize,
    anchor: usize,
    /// Moving up and down keeps this x.
    goal: Option<f32>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The last edit was a character typed (they're one step together).
    typing: bool,
    /// The visual lines (byte ranges, without their break) and each character's x on its
    /// line (from the last layout), and what that layout was made for.
    lines: Vec<Range<usize>>,
    stops: Vec<Vec<(usize, f32)>>,
    laid_out: Option<(u64, u32)>,
    /// Scrolled (physical pixels), its scrollbar, where the text was drawn, a line's height.
    scroll: f32,
    bar: EdgeScrollbar,
    area: Rect,
    line_h: f32,
    /// The caret must come into view at the next paint.
    reveal: bool,
}

/// What the field shows besides its text.
#[derive(Debug, Clone, Copy)]
pub struct MultilineSpec<'a> {
    pub placeholder: &'a str,
    pub focused: bool,
    /// The first line's length past which a hint shows (`72`: a commit's subject); `None`:
    /// no hint.
    pub subject_limit: Option<usize>,
}

impl MultilineField {
    pub fn new(text: &str) -> Self {
        let mut field = Self { bar: EdgeScrollbar::auto_hiding(), ..Self::default() };
        field.set_text(text);
        field
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its whole text replaced (the history starts over).
    pub fn set_text(&mut self, text: &str) {
        self.text = text.replace("\r\n", "\n");
        self.cursor = self.text.len();
        self.anchor = self.cursor;
        self.undo.clear();
        self.redo.clear();
        self.laid_out = None;
        self.bar.set_auto_hiding(true);
    }

    pub fn selection(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
        self.typing = false;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { text: self.text.clone(), cursor: self.cursor, anchor: self.anchor }
    }

    /// Before an edit: the state kept for Undo (characters typed in a row: one step).
    fn remember(&mut self, typing: bool) {
        if !(typing && self.typing) {
            self.undo.push(self.snapshot());
            if self.undo.len() > HISTORY {
                self.undo.remove(0);
            }
        }
        self.typing = typing;
        self.redo.clear();
    }

    /// Typed or pasted text, replacing the selection (line breaks kept).
    pub fn insert(&mut self, typed: &str) {
        let clean: String =
            typed.replace("\r\n", "\n").chars().filter(|c| *c == '\n' || !c.is_control()).collect();
        let range = self.selection();
        if range.is_empty() && clean.is_empty() {
            return;
        }
        let one = range.is_empty() && clean.chars().count() == 1 && clean != "\n";
        self.remember(one);
        self.text.replace_range(range.clone(), &clean);
        self.cursor = range.start + clean.len();
        self.anchor = self.cursor;
        self.edited();
    }

    /// A line break (Enter).
    pub fn newline(&mut self) {
        self.insert("\n");
    }

    fn edited(&mut self) {
        self.goal = None;
        self.reveal = true;
        self.laid_out = None;
    }

    pub fn undo(&mut self) -> bool {
        let Some(before) = self.undo.pop() else { return false };
        self.redo.push(self.snapshot());
        (self.text, self.cursor, self.anchor) = (before.text, before.cursor, before.anchor);
        self.typing = false;
        self.edited();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(after) = self.redo.pop() else { return false };
        self.undo.push(self.snapshot());
        (self.text, self.cursor, self.anchor) = (after.text, after.cursor, after.anchor);
        self.typing = false;
        self.edited();
        true
    }

    fn prev_char(&self, at: usize) -> usize {
        self.text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_char(&self, at: usize) -> usize {
        self.text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
    }

    /// The start of the word before `at` (spaces skipped first).
    fn word_left(&self, at: usize) -> usize {
        let before = &self.text[..at];
        let trimmed = before.trim_end_matches(|c: char| !c.is_alphanumeric());
        trimmed
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric())
            .last()
            .map_or(trimmed.len().min(before.len()), |(i, _)| i)
    }

    /// The end of the word after `at`.
    fn word_right(&self, at: usize) -> usize {
        let after = &self.text[at..];
        let skip = after.len() - after.trim_start_matches(|c: char| !c.is_alphanumeric()).len();
        let rest = &after[skip..];
        let word =
            rest.char_indices().find(|(_, c)| !c.is_alphanumeric()).map_or(rest.len(), |(i, _)| i);
        at + skip + word
    }

    fn move_to(&mut self, at: usize, extend: bool) {
        self.cursor = at;
        if !extend {
            self.anchor = at;
        }
        self.typing = false;
        self.reveal = true;
    }

    pub fn left(&mut self, word: bool, extend: bool) {
        self.goal = None;
        let sel = self.selection();
        let at = if !extend && !sel.is_empty() {
            sel.start
        } else if word {
            self.word_left(self.cursor)
        } else {
            self.prev_char(self.cursor)
        };
        self.move_to(at, extend);
    }

    pub fn right(&mut self, word: bool, extend: bool) {
        self.goal = None;
        let sel = self.selection();
        let at = if !extend && !sel.is_empty() {
            sel.end
        } else if word {
            self.word_right(self.cursor)
        } else {
            self.next_char(self.cursor)
        };
        self.move_to(at, extend);
    }

    /// Backspace (a word with ⌥ / Ctrl), or the selection deleted.
    pub fn backspace(&mut self, word: bool) {
        if self.selection().is_empty() {
            let from = if word { self.word_left(self.cursor) } else { self.prev_char(self.cursor) };
            self.anchor = from;
        }
        self.insert("");
    }

    pub fn delete(&mut self) {
        if self.selection().is_empty() {
            self.anchor = self.next_char(self.cursor);
        }
        self.insert("");
    }

    /// The visual line holding `at` (from the last layout).
    fn line_of(&self, at: usize) -> usize {
        // Where a line wraps, its end is the next one's start: the caret goes there.
        (0..self.lines.len())
            .find(|&i| {
                let r = &self.lines[i];
                let wraps = self.lines.get(i + 1).is_some_and(|n| n.start == r.end);
                at >= r.start && (at < r.end || (at == r.end && !wraps))
            })
            .unwrap_or(self.lines.len().saturating_sub(1))
    }

    fn x_of(&self, line: usize, at: usize) -> f32 {
        self.stops
            .get(line)
            .and_then(|s| s.iter().find(|(i, _)| *i == at).or(s.last()))
            .map_or(0.0, |(_, x)| *x)
    }

    /// The byte on `line` nearest `x`.
    fn at_x(&self, line: usize, x: f32) -> usize {
        let Some(stops) = self.stops.get(line) else { return self.text.len() };
        stops
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(self.text.len(), |(i, _)| *i)
    }

    /// Home / End: of the visual line, or of the text (`far`: ⌘).
    pub fn home(&mut self, far: bool, extend: bool) {
        self.goal = None;
        let at = if far || self.lines.is_empty() {
            0
        } else {
            self.lines[self.line_of(self.cursor)].start
        };
        self.move_to(at, extend);
    }

    pub fn end(&mut self, far: bool, extend: bool) {
        self.goal = None;
        let at = if far || self.lines.is_empty() {
            self.text.len()
        } else {
            self.lines[self.line_of(self.cursor)].end
        };
        self.move_to(at, extend);
    }

    /// ↑ / ↓: the visual line above / below, at the same x (the text's ends past them).
    pub fn vertical(&mut self, down: bool, extend: bool) {
        if self.lines.is_empty() {
            return;
        }
        let line = self.line_of(self.cursor);
        let x = self.goal.unwrap_or_else(|| self.x_of(line, self.cursor));
        let at = match (down, line) {
            (false, 0) => 0,
            (true, l) if l + 1 >= self.lines.len() => self.text.len(),
            (false, l) => self.at_x(l - 1, x),
            (true, l) => self.at_x(l + 1, x),
        };
        self.move_to(at, extend);
        self.goal = Some(x);
    }

    /// The byte under `p` (from the last layout).
    fn at_point(&self, p: Point) -> usize {
        if self.lines.is_empty() {
            return self.text.len();
        }
        let row = ((p.y - self.area.min_y() + self.scroll) / self.line_h.max(1.0)).floor();
        let line = (row.max(0.0) as usize).min(self.lines.len() - 1);
        self.at_x(line, p.x - self.area.min_x())
    }

    /// The field contains `p` (its text area).
    pub fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
    }

    /// A press at `p`, `clicks` in a row: the caret there (⇧: the selection to there), two:
    /// its word, three: its line. Its scrollbar first. `false`: not on it.
    pub fn press(&mut self, p: Point, clicks: u32, extend: bool) -> bool {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return true;
        }
        if !self.area.contains(p) {
            return false;
        }
        let at = self.at_point(p);
        self.goal = None;
        self.typing = false;
        match clicks {
            0 | 1 => self.move_to(at, extend),
            2 => {
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
            _ => {
                let line = &self.lines[self.line_of(at)];
                self.anchor = line.start;
                self.cursor = line.end;
            }
        }
        true
    }

    /// The mouse dragged from a press in it: the selection follows (its scrollbar's thumb
    /// first). `true` if it took it.
    pub fn drag(&mut self, p: Point) -> bool {
        if let Some(offset) = self.bar.drag(p) {
            self.scroll = offset;
            return true;
        }
        self.cursor = self.at_point(p);
        self.reveal = true;
        true
    }

    pub fn release(&mut self) -> bool {
        self.bar.release()
    }

    /// The pointer at `p` (its scrollbar shows on its lane). `true` if it changed.
    pub fn hover(&mut self, p: Point) -> bool {
        self.bar.hover(p)
    }

    /// The wheel over it: scrolled (never past its ends). `true` if it moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        let content = self.lines.len() as f32 * self.line_h;
        self.scroll = (self.scroll + dy).clamp(0.0, (content - self.area.size.height).max(0.0));
        self.scroll != before
    }

    /// When its scrollbar next needs a frame (fading).
    pub fn next_frame(&self) -> Option<std::time::Instant> {
        self.bar.next_frame()
    }

    fn layout_key(&self, width: f32) -> (u64, u32) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.text.hash(&mut h);
        (h.finish(), width.to_bits())
    }

    /// Lays the text out at `width` (wrapped at words; a word longer than a line cut): its
    /// visual lines and where each character sits. Only when the text or the width changed.
    fn layout(&mut self, text: &mut TextSystem, font: &Font, width: f32) {
        let key = self.layout_key(width);
        if self.laid_out == Some(key) {
            return;
        }
        self.laid_out = Some(key);
        self.lines.clear();
        self.stops.clear();
        let full = self.text.clone();
        let mut start = 0;
        for paragraph in full.split('\n') {
            let end = start + paragraph.len();
            let shaped = text.shape_line(paragraph, font);
            let x_at = |i: usize| shaped.x_at(i - start);
            let mut line_start = start;
            let mut last_space: Option<usize> = None;
            let bounds: Vec<usize> = paragraph
                .char_indices()
                .map(|(i, _)| start + i)
                .chain(std::iter::once(end))
                .collect();
            let mut k = 0;
            while k < bounds.len() {
                let at = bounds[k];
                let over = x_at(at) - x_at(line_start) > width && at > line_start;
                if over {
                    // Break after the last space on the line, or before this character.
                    let cut = match last_space {
                        Some(s) if s > line_start => s,
                        // A word longer than the line: cut before this character (a
                        // character wider than the line: alone on its line).
                        _ if bounds[k - 1] > line_start => bounds[k - 1],
                        _ => at,
                    };
                    self.push_line(line_start..cut, &bounds, &x_at);
                    line_start = cut;
                    last_space = None;
                    k = bounds.iter().position(|&b| b >= cut).unwrap_or(k);
                    continue;
                }
                if at < end && full[at..].starts_with(' ') {
                    last_space = Some(at + 1);
                }
                k += 1;
            }
            self.push_line(line_start..end, &bounds, &x_at);
            start = end + 1;
        }
    }

    fn push_line(&mut self, range: Range<usize>, bounds: &[usize], x_at: &dyn Fn(usize) -> f32) {
        let x0 = x_at(range.start);
        let stops = bounds
            .iter()
            .filter(|&&b| b >= range.start && b <= range.end)
            .map(|&b| (b, x_at(b) - x0))
            .collect();
        self.lines.push(range);
        self.stops.push(stops);
    }

    /// How tall it is for `width` (the field's outer width): between `min` and `max` lines
    /// of its text, with its padding.
    pub fn height(
        &mut self,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        width: f32,
        (min, max): (usize, usize),
    ) -> f32 {
        let font = ds.ui_font(0.0);
        let pad = ds.px(10.0);
        self.layout(text, &font, (width - pad * 2.0 - ds.px(12.0)).max(1.0));
        let line_h = ds.px(20.0);
        let lines = self.lines.len().clamp(min, max);
        lines as f32 * line_h + pad * 2.0
    }

    /// The field in `rect`: its box, its text wrapped and scrolled (the caret kept in view),
    /// the selection, the caret, the placeholder while it's empty, the subject's hint.
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        rect: Rect,
        spec: MultilineSpec<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(0.0);
        let pad = px(10.0);
        let line_h = px(20.0);
        self.line_h = line_h;
        let border =
            if spec.focused { p.search_field_border_focused } else { p.search_field_border };
        let layer = scene.push_layer(None);
        layer.quad(
            Quad::filled(rect, p.search_field_background)
                .rounded(px(radius::MD))
                .bordered(ds.line(), border),
        );
        // The text leaves its scrollbar a lane on the right.
        let area = Rect::new(
            rect.min_x() + pad,
            rect.min_y() + pad,
            (rect.size.width - pad * 2.0 - px(12.0)).max(1.0),
            (rect.size.height - pad * 2.0).max(line_h),
        );
        self.area = area;
        self.layout(text, &font, area.size.width);
        let content = self.lines.len() as f32 * line_h;
        if std::mem::take(&mut self.reveal) {
            let line = self.line_of(self.cursor) as f32;
            let top = line * line_h;
            if top < self.scroll {
                self.scroll = top;
            } else if top + line_h > self.scroll + area.size.height {
                self.scroll = top + line_h - area.size.height;
            }
        }
        self.scroll = self.scroll.clamp(0.0, (content - area.size.height).max(0.0)).round();
        let layer = scene.push_layer(Some(Rect::new(
            area.min_x() - px(2.0),
            area.min_y(),
            area.size.width + px(4.0),
            area.size.height,
        )));
        if self.text.is_empty() {
            let shaped = text.shape_line(spec.placeholder, &font);
            draw_text(
                layer,
                &shaped,
                area.min_x(),
                ds::optical_baseline(area.min_y(), line_h, &font),
                p.text_muted,
            );
        }
        let sel = self.selection();
        let caret_line = self.line_of(self.cursor);
        for (i, range) in self.lines.iter().enumerate() {
            let top = area.min_y() + i as f32 * line_h - self.scroll;
            if top + line_h < area.min_y() || top > area.max_y() {
                continue;
            }
            // The selection on this line.
            let (a, b) = (sel.start.max(range.start), sel.end.min(range.end));
            if spec.focused
                && !sel.is_empty()
                && (a < b || (sel.start <= range.end && sel.end > range.end))
            {
                let xa = self.x_of(i, a);
                let mut xb = self.x_of(i, b);
                // A selected line break: a little past the text.
                if sel.end > range.end {
                    xb += px(6.0);
                }
                layer.quad(Quad::filled(
                    Rect::new(
                        area.min_x() + xa,
                        top + px(1.0),
                        (xb - xa).max(0.0),
                        line_h - px(2.0),
                    ),
                    p.selection,
                ));
            }
            let line = &self.text[range.clone()];
            let shaped = text.shape_line(line, &font);
            draw_text(
                layer,
                &shaped,
                area.min_x(),
                ds::optical_baseline(top, line_h, &font),
                p.text,
            );
            if spec.focused && i == caret_line {
                let x = area.min_x() + self.x_of(i, self.cursor);
                ui_kit::caret::paint(
                    layer,
                    ui_kit::caret::CaretPaint {
                        id: std::ptr::from_ref(self) as u64,
                        cell: Rect::new(
                            x.round(),
                            top + px(2.0),
                            px(1.5).max(1.0),
                            line_h - px(4.0),
                        ),
                        color: p.caret,
                        scale: 1.0,
                        shape: Some(theming::caret::CaretShape::Bar),
                        thickness: Some(px(1.5).max(1.0)),
                        steady: false,
                    },
                );
            }
        }
        // The subject's length, once it nears its limit.
        if let Some(limit) = spec.subject_limit {
            let subject = self.text.split('\n').next().unwrap_or_default().chars().count();
            if subject > limit * 2 / 3 {
                let small = ds.ui_font(-2.0);
                let label = format!("{subject}/{limit}");
                let shaped = text.shape_line(&label, &small);
                let color = if subject > limit { p.warning } else { p.text_muted.with_alpha(0.7) };
                let layer = scene.push_layer(Some(rect));
                draw_text(
                    layer,
                    &shaped,
                    rect.max_x() - pad - px(12.0) - shaped.width,
                    rect.min_y() + pad * 0.5 + small.size,
                    color,
                );
            }
        }
        self.bar.set_auto_hiding(true);
        let lane =
            Rect::new(rect.min_x(), area.min_y(), rect.size.width - px(2.0), area.size.height);
        self.bar.paint(scene, p, ds.scale, lane, content, self.scroll);
    }
}

#[cfg(test)]
#[path = "../tests/unit/multiline.rs"]
mod tests;
