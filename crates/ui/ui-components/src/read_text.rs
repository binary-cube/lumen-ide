//! A text to read (release notes, a description): blocks — headings, paragraphs, points
//! of a list (indented by depth), gaps — wrapped at words to the area's width, scrolled
//! with the app's scrollbar (the wheel, its thumb dragged, a press on its track), and
//! selected as in an editor: a press and a drag (across blocks), ⇧ + press to extend, a
//! double click's word, a triple click's block, Select All; the selection copied as text
//! (a point of a list with its `•`, blocks on lines of their own).
//!
//! Laid out when painted (the text system measures), again only when the width, the scale
//! or the blocks change; the mouse uses the last layout.

use std::ops::Range;
use std::sync::Arc;

use kernel::{Point, Rect};
use render_api::{Quad, Scene};
use text_layout::{Font, ShapedLine, TextSystem};
use ui_kit::draw_text;

use crate::EdgeScrollbar;
use crate::ds::{self, Ctx};

/// A block's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Heading,
    Paragraph,
    /// A point of a list, `depth` 0 the outermost.
    Point {
        depth: u8,
    },
    /// Space between blocks (no text).
    Gap,
}

/// A block of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
}

impl Block {
    pub fn new(kind: BlockKind, text: impl Into<String>) -> Self {
        Self { kind, text: text.into() }
    }
}

/// A place in the text: a block and a byte in its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct TextPos {
    pub block: usize,
    pub byte: usize,
}

/// Logical sizes.
const BODY_LINE: f32 = 22.0;
const HEADING_LINE: f32 = 28.0;
const GAP: f32 = 10.0;
const INDENT: f32 = 18.0;
/// The bullet's place left of its point's text.
const BULLET_BACK: f32 = 12.0;
/// The room kept for the scrollbar right of the text.
const BAR_ROOM: f32 = 12.0;

/// One row on screen: a piece of a block's text.
#[derive(Debug, Clone)]
struct Row {
    block: usize,
    /// Its bytes in the block's text.
    range: Range<usize>,
    /// Its left edge and top, from the content's (physical, unscrolled).
    x: f32,
    y: f32,
    height: f32,
    heading: bool,
    /// The first row of a point (its bullet drawn).
    bullet: bool,
    shaped: Arc<ShapedLine>,
}

#[derive(Debug, Clone)]
struct Layout {
    width: f32,
    scale: f32,
    rows: Vec<Row>,
    height: f32,
}

#[derive(Debug, Default)]
pub struct ReadText {
    blocks: Vec<Block>,
    laid: Option<Layout>,
    /// Where it was last painted (the mouse is read against it).
    area: Rect,
    scroll: f32,
    bar: EdgeScrollbar,
    /// The selection: where it began and where it is (equal: none).
    anchor: TextPos,
    head: TextPos,
    /// A press made it (the drags extend it); by words or blocks after a double / triple
    /// click, from the press's word or block.
    selecting: Option<Unit>,
}

/// What a drag selects by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Char,
    Word(TextPos, TextPos),
    Block(usize),
}

impl ReadText {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self { blocks, ..Self::default() }
    }

    /// Other blocks: laid out again, unselected, at the top.
    pub fn set_blocks(&mut self, blocks: Vec<Block>) {
        if blocks != self.blocks {
            self.blocks = blocks;
            self.laid = None;
            self.scroll = 0.0;
            self.anchor = TextPos::default();
            self.head = TextPos::default();
        }
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Nothing to read.
    pub fn is_empty(&self) -> bool {
        self.blocks.iter().all(|b| b.text.trim().is_empty())
    }

    /// The content's height (physical), as last laid out.
    pub fn content_height(&self) -> f32 {
        self.laid.as_ref().map_or(0.0, |l| l.height)
    }

    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.area.size.height).max(0.0)
    }

    /// Scrolled by `dy` (physical): `true` if it moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let to = (self.scroll + dy).clamp(0.0, self.max_scroll());
        std::mem::replace(&mut self.scroll, to) != to
    }

    // ─── The layout ───

    fn fonts(ctx: &Ctx<'_>) -> (Font, Font) {
        (ctx.ui_font(0.0), ctx.ui_font(1.0).with_weight(Font::SEMIBOLD))
    }

    /// The blocks wrapped to `width` (physical), at `ctx`'s scale.
    fn layout(&self, text: &mut TextSystem, ctx: &Ctx<'_>, width: f32) -> Layout {
        let px = |v: f32| ctx.px(v);
        let (body, heading) = Self::fonts(ctx);
        let mut rows = Vec::new();
        let mut y = 0.0;
        for (i, block) in self.blocks.iter().enumerate() {
            let (font, line, indent) = match block.kind {
                BlockKind::Gap => {
                    y += px(GAP);
                    continue;
                }
                BlockKind::Heading => (&heading, px(HEADING_LINE), 0.0),
                BlockKind::Paragraph => (&body, px(BODY_LINE), 0.0),
                BlockKind::Point { depth } => {
                    (&body, px(BODY_LINE), px(INDENT) * (f32::from(depth) + 1.0))
                }
            };
            let room = (width - indent).max(px(40.0));
            for (n, range) in wrap(text, &block.text, font, room).into_iter().enumerate() {
                let shaped = text.shape_line(&block.text[range.clone()], font);
                rows.push(Row {
                    block: i,
                    range,
                    x: indent,
                    y,
                    height: line,
                    heading: block.kind == BlockKind::Heading,
                    bullet: n == 0 && matches!(block.kind, BlockKind::Point { .. }),
                    shaped,
                });
                y += line;
            }
        }
        Layout { width, scale: ctx.scale, rows, height: y }
    }

    // ─── Painting ───

    /// Drawn in `area`: the rows in view, the selection under them, the scrollbar.
    pub fn paint(&mut self, scene: &mut Scene, text: &mut TextSystem, ctx: &Ctx<'_>, area: Rect) {
        let px = |v: f32| ctx.px(v);
        self.area = area;
        let width = (area.size.width - px(BAR_ROOM)).max(px(40.0));
        let stale = self.laid.as_ref().is_none_or(|l| l.width != width || l.scale != ctx.scale);
        if stale {
            self.laid = Some(self.layout(text, ctx, width));
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        let Some(laid) = self.laid.take() else { return };
        let p = ctx.palette;
        let (body, heading) = Self::fonts(ctx);
        let selection = self.selection();
        let layer = scene.push_layer(Some(area));
        let top = area.min_y() - self.scroll;
        for row in &laid.rows {
            let y = top + row.y;
            if y + row.height < area.min_y() || y > area.max_y() {
                continue;
            }
            let x = area.min_x() + row.x;
            // The selected part of the row, under its text.
            if let Some(cut) = row_cut(row, &selection) {
                let (from, to) = (row.shaped.x_at(cut.start), row.shaped.x_at(cut.end));
                // A row selected to its end shows it (its line break or space copied too).
                let to = if cut.end == row.range.len() && selection.end > pos_end(row) {
                    to + px(4.0)
                } else {
                    to
                };
                if to > from {
                    let r = Rect::new(x + from, y + px(1.0), to - from, row.height - px(2.0));
                    layer.quad(Quad::filled(r, ctx.fade(p.selection)).rounded(px(2.0)));
                }
            }
            let font = if row.heading { &heading } else { &body };
            let base = ds::optical_baseline(y, row.height, font);
            if row.bullet {
                let dot = text.shape_line("•", font);
                draw_text(layer, &dot, x - px(BULLET_BACK), base, ctx.fade(p.accent));
            }
            draw_text(layer, &row.shaped, x, base, ctx.fade(p.text));
        }
        let content = laid.height;
        self.laid = Some(laid);
        self.bar.paint(scene, p, ctx.scale, area, content, self.scroll);
    }

    /// When the scrollbar next needs a frame (fading in or out).
    pub fn next_frame(&self) -> Option<std::time::Instant> {
        self.bar.next_frame()
    }

    // ─── The mouse ───

    /// Over its text (the I-beam) — not over its scrollbar.
    pub fn over_text(&self, p: Point) -> bool {
        self.area.contains(p) && !self.bar.contains(p)
    }

    /// The place under `p` (outside the rows: the nearest row's nearest end).
    fn pos_at(&self, p: Point) -> TextPos {
        let Some(laid) = &self.laid else { return TextPos::default() };
        let y = p.y - self.area.min_y() + self.scroll;
        let row = laid.rows.iter().find(|r| y < r.y + r.height).or(laid.rows.last());
        let Some(row) = row else { return TextPos::default() };
        if y < row.y && laid.rows.first().is_some_and(|f| std::ptr::eq(f, row)) {
            return TextPos { block: row.block, byte: row.range.start };
        }
        let x = p.x - self.area.min_x() - row.x;
        let at = row.shaped.byte_at(x, row.range.len()).min(row.range.len());
        TextPos { block: row.block, byte: row.range.start + at }
    }

    /// A press at `p`, `clicks` in a row; `extend` (⇧): the selection reaches it. On the
    /// scrollbar, its thumb taken or the text moved there. `true` if it was in it.
    pub fn press(&mut self, p: Point, clicks: u32, extend: bool) -> bool {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset.clamp(0.0, self.max_scroll());
            return true;
        }
        if !self.area.contains(p) {
            return false;
        }
        let at = self.pos_at(p);
        match clicks {
            2 => {
                let (from, to) = self.word_at(at);
                (self.anchor, self.head) = (from, to);
                self.selecting = Some(Unit::Word(from, to));
            }
            n if n >= 3 => {
                let len = self.blocks.get(at.block).map_or(0, |b| b.text.len());
                self.anchor = TextPos { block: at.block, byte: 0 };
                self.head = TextPos { block: at.block, byte: len };
                self.selecting = Some(Unit::Block(at.block));
            }
            _ => {
                if !extend {
                    self.anchor = at;
                }
                self.head = at;
                self.selecting = Some(Unit::Char);
            }
        }
        true
    }

    /// The mouse dragged to `p`: the thumb, or the selection (by its unit). `true` if it
    /// changed.
    pub fn drag(&mut self, p: Point) -> bool {
        if let Some(offset) = self.bar.drag(p) {
            self.scroll = offset.clamp(0.0, self.max_scroll());
            return true;
        }
        let Some(unit) = self.selecting else { return false };
        // Past an edge: the text scrolls along.
        if p.y < self.area.min_y() {
            self.scroll_by(p.y - self.area.min_y());
        } else if p.y > self.area.max_y() {
            self.scroll_by(p.y - self.area.max_y());
        }
        let at = self.pos_at(p);
        let before = (self.anchor, self.head);
        match unit {
            Unit::Char => self.head = at,
            Unit::Word(from, to) => {
                let (w_from, w_to) = self.word_at(at);
                (self.anchor, self.head) =
                    if at < from { (to, w_from) } else { (from, w_to.max(to)) };
            }
            Unit::Block(block) => {
                let len = |b: usize| self.blocks.get(b).map_or(0, |b| b.text.len());
                (self.anchor, self.head) = if at.block < block {
                    (TextPos { block, byte: len(block) }, TextPos { block: at.block, byte: 0 })
                } else {
                    (TextPos { block, byte: 0 }, TextPos { block: at.block, byte: len(at.block) })
                };
            }
        }
        before != (self.anchor, self.head)
    }

    /// The button released. `true` if something was held.
    pub fn release(&mut self) -> bool {
        self.selecting.take().is_some() | self.bar.release()
    }

    /// The mouse at `p` (its scrollbar widens under it). `true` if that changed.
    pub fn hover(&mut self, p: Point) -> bool {
        self.bar.hover(p)
    }

    /// The word at `at` (letters, digits and `_`; elsewhere the character there).
    fn word_at(&self, at: TextPos) -> (TextPos, TextPos) {
        let Some(text) = self.blocks.get(at.block).map(|b| b.text.as_str()) else {
            return (at, at);
        };
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let byte = at.byte.min(text.len());
        let start = text[..byte].char_indices().rev().take_while(|(_, c)| word(*c)).last();
        let start = start.map_or(byte, |(i, _)| i);
        let end = text[byte..]
            .char_indices()
            .find(|(_, c)| !word(*c))
            .map_or(text.len(), |(i, _)| byte + i);
        let end = if end == start {
            text[start..].chars().next().map_or(start, |c| start + c.len_utf8())
        } else {
            end
        };
        (TextPos { block: at.block, byte: start }, TextPos { block: at.block, byte: end })
    }

    // ─── The selection ───

    /// The selection, ordered.
    pub fn selection(&self) -> Range<TextPos> {
        if self.anchor <= self.head { self.anchor..self.head } else { self.head..self.anchor }
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }

    /// Everything selected. `true` if there's text.
    pub fn select_all(&mut self) -> bool {
        let last = self.blocks.len().saturating_sub(1);
        self.anchor = TextPos::default();
        self.head = TextPos { block: last, byte: self.blocks.last().map_or(0, |b| b.text.len()) };
        self.has_selection()
    }

    /// Nothing selected. `true` if something was.
    pub fn clear_selection(&mut self) -> bool {
        let had = self.has_selection();
        self.anchor = self.head;
        had
    }

    /// The selected text: each block on a line of its own (a point with its `•`, indented
    /// by depth), a gap as an empty line. `None`: nothing selected.
    pub fn selected_text(&self) -> Option<String> {
        if !self.has_selection() {
            return None;
        }
        let sel = self.selection();
        let mut out = String::new();
        for (i, block) in
            self.blocks.iter().enumerate().take(sel.end.block + 1).skip(sel.start.block)
        {
            let from = if i == sel.start.block { sel.start.byte } else { 0 };
            let to = if i == sel.end.block { sel.end.byte } else { block.text.len() };
            let part = block.text.get(from.min(to)..to).unwrap_or_default();
            if !out.is_empty() {
                out.push('\n');
            }
            if let BlockKind::Point { depth } = block.kind
                && from == 0
            {
                out.push_str(&"  ".repeat(usize::from(depth)));
                out.push_str("• ");
            }
            out.push_str(part);
        }
        Some(out)
    }
}

/// Where row `row` ends, as a place.
fn pos_end(row: &Row) -> TextPos {
    TextPos { block: row.block, byte: row.range.end }
}

/// The part of `row` (bytes in it) inside `selection`, if any.
fn row_cut(row: &Row, selection: &Range<TextPos>) -> Option<Range<usize>> {
    let start = TextPos { block: row.block, byte: row.range.start };
    let end = pos_end(row);
    if selection.end <= start || selection.start > end {
        return None;
    }
    let from = if selection.start > start { selection.start.byte - row.range.start } else { 0 };
    let to =
        if selection.end < end { selection.end.byte - row.range.start } else { row.range.len() };
    (to >= from).then_some(from..to)
}

/// `text` cut in rows of `width` at most, at spaces (a word longer than a row cut where
/// it overflows): each row's bytes (the space it broke at left out of both).
fn wrap(text: &mut TextSystem, value: &str, font: &Font, width: f32) -> Vec<Range<usize>> {
    if value.is_empty() {
        return std::iter::once(0..0).collect();
    }
    let shaped = text.shape_line(value, font);
    let mut rows = Vec::new();
    let mut start = 0;
    let mut space: Option<usize> = None;
    for (i, c) in value.char_indices() {
        let over = shaped.x_at(i + c.len_utf8()) - shaped.x_at(start) > width;
        if over && i > start {
            let cut = space.filter(|s| *s > start).unwrap_or(i);
            rows.push(start..cut);
            start = if space.is_some_and(|s| s == cut) { cut + 1 } else { cut };
            space = None;
        }
        if c == ' ' {
            space = Some(i);
        }
    }
    rows.push(start..value.len());
    rows
}

#[cfg(test)]
#[path = "../tests/unit/read_text.rs"]
mod tests;
