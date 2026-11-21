//! The card explaining the code under the mouse, after the mouse rests on a word for a
//! moment (at once with ⌘ held): the declaring type, the signature (colored, one
//! parameter per line when there are several), the doc comment, notes ("since PHP 8.0",
//! "deprecated"), then the parameters with their descriptions and the source file. The
//! app computes the card once the delay has passed; its layout is computed on the first
//! paint and kept while it is shown. Taller than the room it has, it scrolls (the wheel,
//! its scrollbar).

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use code_intelligence::CompletionKind;
use kernel::text::one_line;
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, FontMetrics, ShapedLine, TextSystem};
use ui_components::EdgeScrollbar;
use ui_kit::{centered_baseline, draw_text};

use crate::anim::Tween;
use crate::ds::{self, Ctx};
use crate::palette::Palette;

/// How long the mouse rests on a word before the card appears.
const DELAY: Duration = Duration::from_millis(600);
/// The card's height at most (logical pixels; less when the editor has less room): a
/// longer explanation scrolls.
const MAX_H: f32 = 520.0;

/// A parameter: its type, its name and what it is for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CardParam {
    pub ty: Option<String>,
    pub name: String,
    pub doc: Option<String>,
}

/// What the card shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HoverCard {
    /// The declaring type (with its kind's badge) or a namespace.
    pub title: Option<(String, Option<CompletionKind>)>,
    /// Dimmed after the title (a namespace).
    pub subtitle: Option<String>,
    /// The code; `\n` separates its lines.
    pub code: String,
    pub doc: Option<String>,
    /// The notes (translated), each a warning or not.
    pub notes: Vec<(String, bool)>,
    pub params: Vec<CardParam>,
    pub source: Option<String>,
    /// The extension of its language's files (its code colored as that language's).
    pub extension: String,
    /// The translated row labels: "Parameters:", "Source:".
    pub labels: (String, String),
}

/// One thing to draw, relative to the card's top-left corner.
#[derive(Debug)]
enum Op {
    Text {
        x: f32,
        baseline: f32,
        line: Arc<ShapedLine>,
        color: Rgba,
    },
    Fill {
        rect: Rect,
        color: Rgba,
        radius: f32,
    },
    /// A badge (a note, a parameter's type or name): from `x`, centered on `mid`.
    Badge {
        x: f32,
        mid: f32,
        badge: CardBadge,
    },
}

#[derive(Debug)]
struct Layout {
    scale: f32,
    size: (f32, f32),
    ops: Vec<Op>,
}

#[derive(Debug)]
struct Shown {
    word: Range<usize>,
    at: Point,
    card: HoverCard,
    appear: Tween,
    layout: Option<Layout>,
    /// Where it was drawn (the mouse may move onto it).
    rect: Rect,
    /// How far its content is scrolled, and how far it can be (pixels).
    scroll: f32,
    max_scroll: f32,
    bar: EdgeScrollbar,
}

#[derive(Debug, Default)]
pub struct CodeHover {
    /// The word under the mouse, where, and since when.
    pending: Option<(Range<usize>, Point, Instant)>,
    shown: Option<Shown>,
}

impl CodeHover {
    /// The mouse is over `word` (document offsets), or over no word; `now`: no delay (⌘
    /// held). `true` if a redraw is needed.
    pub fn track(&mut self, word: Option<Range<usize>>, p: Point, now: bool) -> bool {
        if self.shown.as_ref().is_some_and(|s| Some(&s.word) == word.as_ref()) {
            return false;
        }
        // On the card, or on the way to it: it stays (its text can be read, selected later).
        if self.shown.as_ref().is_some_and(|s| reach(s).contains(p)) {
            self.pending = None;
            return false;
        }
        if self.pending.as_ref().map(|(w, ..)| w) == word.as_ref() && word.is_some() {
            if now && let Some((_, _, since)) = &mut self.pending {
                *since = Instant::now().checked_sub(DELAY).unwrap_or(*since);
                return true;
            }
            return false;
        }
        let hid = self.shown.take().is_some();
        let since = if now { Instant::now().checked_sub(DELAY) } else { Some(Instant::now()) };
        self.pending = word.zip(since).map(|(w, since)| (w, p, since));
        hid || (now && self.pending.is_some())
    }

    /// The word whose card is due (the delay passed): the app computes it and calls `show`.
    pub fn due(&self) -> Option<(Range<usize>, Point)> {
        let (word, at, since) = self.pending.as_ref()?;
        (since.elapsed() >= DELAY).then(|| (word.clone(), *at))
    }

    /// When the pending card is due (a redraw is scheduled for then).
    pub fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|(_, _, since)| *since + DELAY)
    }

    /// The card for the due word (`None`: nothing to say about it).
    pub fn show(&mut self, card: Option<HoverCard>) {
        let Some((word, at, _)) = self.pending.take() else { return };
        self.shown = card.map(|card| {
            let mut appear = Tween::new(0.0, Duration::from_millis(120));
            appear.retarget(1.0);
            Shown {
                word,
                at,
                card,
                appear,
                layout: None,
                rect: Rect::default(),
                scroll: 0.0,
                max_scroll: 0.0,
                bar: EdgeScrollbar::auto_hiding(),
            }
        });
    }

    /// Hides it (a key, a click, scrolling).
    pub fn hide(&mut self) -> bool {
        self.pending = None;
        self.shown.take().is_some()
    }

    pub fn is_shown(&self) -> bool {
        self.shown.is_some()
    }

    /// The mouse is on the card (clicks and scrolling there don't reach the editor).
    pub fn contains(&self, p: Point) -> bool {
        self.shown.as_ref().is_some_and(|s| s.rect.contains(p))
    }

    /// Where the card is drawn, while shown.
    pub fn bounds(&self) -> Option<Rect> {
        self.shown.as_ref().map(|s| s.rect)
    }

    pub fn is_animating(&self) -> bool {
        self.shown.as_ref().is_some_and(|s| s.appear.animating())
    }

    /// The wheel over the card (physical pixels, positive = down). `true` if it moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let Some(shown) = &mut self.shown else { return false };
        let before = shown.scroll;
        shown.scroll = (shown.scroll + dy).clamp(0.0, shown.max_scroll);
        shown.scroll != before
    }

    /// The mouse moved (anywhere: leaving it, its scrollbar narrows). `true`: redraw.
    pub fn scrollbar_hover(&mut self, p: Point) -> bool {
        self.shown.as_mut().is_some_and(|s| s.bar.hover(p))
    }

    /// A press on the card's scrollbar: `true` if it took it (a drag follows).
    pub fn scrollbar_press(&mut self, p: Point) -> bool {
        let Some(shown) = self.shown.as_mut().filter(|s| s.rect.contains(p)) else {
            return false;
        };
        let Some(offset) = shown.bar.press(p) else { return false };
        shown.scroll = offset.clamp(0.0, shown.max_scroll);
        true
    }

    pub fn scrollbar_held(&self) -> bool {
        self.shown.as_ref().is_some_and(|s| s.bar.is_dragging())
    }

    /// The held scrollbar's thumb dragged to `p`: `true` if the card's content moved.
    pub fn scrollbar_drag(&mut self, p: Point) -> bool {
        let Some(shown) = &mut self.shown else { return false };
        let Some(offset) = shown.bar.drag(p) else { return false };
        let before = shown.scroll;
        shown.scroll = offset.clamp(0.0, shown.max_scroll);
        shown.scroll != before
    }

    /// The mouse released: `true` if the scrollbar was held.
    pub fn scrollbar_release(&mut self) -> bool {
        self.shown.as_mut().is_some_and(|s| s.bar.release())
    }

    /// When its scrollbar next needs a frame (fading in or out); `None` at rest.
    pub fn frame_deadline(&self) -> Option<Instant> {
        self.shown.as_ref().and_then(|s| s.bar.next_frame())
    }

    /// Draws the card below the mouse (above it if there is no room), within `bounds`.
    pub fn paint(
        &mut self,
        bounds: Rect,
        scale: f32,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let Some(shown) = &mut self.shown else { return };
        if shown.layout.as_ref().is_none_or(|l| l.scale != scale) {
            let max_w = (bounds.size.width * 0.7).min(560.0 * scale);
            shown.layout = Some(layout(&shown.card, max_w, scale, text, palette));
        }
        let Some(layout) = &shown.layout else { return };
        let px = |v: f32| (v * scale).round();
        let (w, content_h) = layout.size;
        let at = shown.at;
        let x = at.x.min(bounds.max_x() - w - px(8.0)).max(bounds.min_x() + px(4.0));
        // Below the mouse, else above; when it fits on neither side, on the roomier one, as
        // high as it has room for (then it scrolls).
        let below = (bounds.max_y() - px(4.0) - (at.y + px(20.0))).max(0.0);
        let above = (at.y - px(12.0) - (bounds.min_y() + px(4.0))).max(0.0);
        let fits = |room: f32| content_h.min(px(MAX_H)) <= room;
        let on_below = fits(below) || (!fits(above) && below >= above);
        let h = content_h.min(px(MAX_H)).min(if on_below { below } else { above }).round();
        let y = if on_below { at.y + px(20.0) } else { at.y - px(12.0) - h };
        let card = Rect::new(x.round(), y.round(), w, h);
        shown.max_scroll = (content_h - h).max(0.0);
        shown.scroll = shown.scroll.min(shown.max_scroll);
        let alpha = shown.appear.value();
        let ctx = Ctx::new(palette, scale).with_opacity(alpha);
        let background = palette.hover_background;
        let under = scene.push_layer(None);
        ds::surface_with(under, &ctx, card, ds::Elevation::Medium, ds::radius::LG, background);
        shown.rect = card;
        // The content within the card, scrolled.
        let layer = scene.push_layer(Some(card));
        let (ox, oy) = (card.min_x(), card.min_y() - shown.scroll.round());
        for op in &layout.ops {
            match op {
                Op::Text { x, baseline, line, color } => {
                    draw_text(
                        layer,
                        line,
                        ox + x,
                        oy + baseline,
                        color.with_alpha(color.a * alpha),
                    );
                }
                Op::Fill { rect, color, radius } => {
                    let r = Rect::new(
                        ox + rect.min_x(),
                        oy + rect.min_y(),
                        rect.size.width,
                        rect.size.height,
                    );
                    layer.quad(Quad::filled(r, color.with_alpha(color.a * alpha)).rounded(*radius));
                }
                Op::Badge { x, mid, badge } => {
                    badge.badge().paint(layer, text, &ctx, ox + x, oy + mid);
                }
            }
        }
        if shown.max_scroll > 0.0 {
            let lane = Rect::new(
                card.min_x(),
                card.min_y() + px(6.0),
                card.size.width - px(2.0),
                card.size.height - px(12.0),
            );
            // The lane is the card less its corners: the content as much less, so the
            // offsets the bar gives back (a press, a drag) are the card's.
            let content = content_h - (h - lane.size.height);
            shown.bar.paint(scene, palette, scale, lane, content, shown.scroll);
        }
    }
}

/// The card plus the strip between it and the word: the mouse crossing it keeps the card.
fn reach(shown: &Shown) -> Rect {
    let r = shown.rect;
    let top = r.min_y().min(shown.at.y);
    let bottom = r.max_y().max(shown.at.y);
    Rect::new(r.min_x(), top, r.size.width, bottom - top)
}

fn code_font(palette: &Palette, scale: f32) -> Font {
    Font::new(palette.code_font.clone(), (palette.code_font_size * scale).round())
}

/// `text` in lines no wider than `first` (the first line) and `rest` (the others), broken
/// at spaces; a word wider than a whole line is broken where it reaches the edge.
fn flow(text: &str, first: f32, rest: f32, font: &Font, ts: &mut TextSystem) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let width = if lines.is_empty() { first } else { rest };
        let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if ts.shape_line(&candidate, font).width <= width {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        // On a line of its own; still too wide: cut into pieces that fit.
        let mut word = word;
        loop {
            let width = if lines.is_empty() { first } else { rest };
            let cut = fitting(word, width, font, ts);
            if cut >= word.len() {
                line = word.to_owned();
                break;
            }
            lines.push(word[..cut].to_owned());
            word = &word[cut..];
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// How much of `word` (bytes, at a character's end, at least one) fits in `width`.
fn fitting(word: &str, width: f32, font: &Font, ts: &mut TextSystem) -> usize {
    let ends: Vec<usize> = word.char_indices().map(|(i, c)| i + c.len_utf8()).collect();
    // The most characters that fit (a binary search over their ends: few shapings).
    let (mut lo, mut hi) = (1, ends.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if ts.shape_line(&word[..ends[mid - 1]], font).width <= width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    ends.get(lo.max(1) - 1).copied().unwrap_or(word.len())
}

/// A row of code: its pieces with their colors.
type CodeRow<'a> = Vec<(&'a str, Rgba)>;

/// A line of code in rows no wider than `width`, colored as its language's: a piece that
/// doesn't fit where the row has got to starts the next row; one wider than a whole row (a
/// long path in a string) is broken after its last `/`, `\` or space that fits, else where
/// it reaches the edge.
fn wrap_code<'a>(
    line: &'a str,
    extension: &str,
    width: f32,
    font: &Font,
    ts: &mut TextSystem,
    palette: &Palette,
) -> Vec<CodeRow<'a>> {
    let (mut rows, mut row, mut x): (Vec<CodeRow<'a>>, CodeRow<'a>, f32) =
        (Vec::new(), Vec::new(), 0.0);
    for (piece, color) in ui_components::code_tokens::runs(line, extension, palette) {
        let mut rest = piece;
        while !rest.is_empty() {
            let shaped = ts.shape_line(rest, font);
            if x + shaped.width <= width {
                x += shaped.width;
                row.push((rest, color));
                break;
            }
            if !row.is_empty() && shaped.width <= width {
                rows.push(std::mem::take(&mut row));
                x = 0.0;
                continue;
            }
            let cut = fitting(rest, width - x, font, ts);
            let soft = rest[..cut].rfind(['/', '\\', ' ']).map(|at| at + 1);
            let cut = soft.filter(|&at| at < rest.len()).unwrap_or(cut);
            if !row.is_empty() && x + ts.shape_line(&rest[..cut], font).width > width {
                rows.push(std::mem::take(&mut row));
                x = 0.0;
                continue;
            }
            row.push((&rest[..cut], color));
            rows.push(std::mem::take(&mut row));
            x = 0.0;
            rest = &rest[cut..];
        }
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

/// A path in lines no wider than `width`, broken after `/`.
fn flow_path(path: &str, width: f32, font: &Font, ts: &mut TextSystem) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for part in path.split_inclusive(['/', '\\']) {
        let candidate = format!("{line}{part}");
        if !line.is_empty() && ts.shape_line(&candidate, font).width > width {
            lines.push(std::mem::take(&mut line));
            line = part.to_owned();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Lays the card out at most `max_w` wide: what to draw, relative to its corner.
fn layout(
    card: &HoverCard,
    max_w: f32,
    scale: f32,
    ts: &mut TextSystem,
    palette: &Palette,
) -> Layout {
    let mut b = Builder::new(max_w, scale, ts, palette);
    b.title(card, ts);
    let code = b.code(card, ts);
    b.doc(card, ts);
    b.notes(card, ts);
    // Parameters and source, in two columns, under a separator.
    let separator = (!card.params.is_empty() || card.source.is_some()).then(|| b.columns(card, ts));
    b.finish(code, separator)
}

/// The card's layout while it is built, top to bottom: the fonts and their sizes, what to
/// draw so far, the next row's top and the widest content.
struct Builder<'a> {
    palette: &'a Palette,
    scale: f32,
    max_w: f32,
    pad: f32,
    /// The width inside the padding.
    inner: f32,
    code_font: Font,
    ui: Font,
    small: Font,
    chip_font: Font,
    um: FontMetrics,
    cm: FontMetrics,
    code_h: f32,
    ui_h: f32,
    ops: Vec<Op>,
    y: f32,
    widest: f32,
}

/// The code's panel: its op (sized at the end), its top and its bottom.
type CodePanel = (usize, f32, f32);

/// The parameters and source's columns: where the values start and how wide they are,
/// the chips' and the rows' heights.
#[derive(Clone, Copy)]
struct Columns {
    vx: f32,
    value_w: f32,
    chip_h: f32,
    row_h: f32,
}

impl<'a> Builder<'a> {
    fn new(max_w: f32, scale: f32, ts: &mut TextSystem, palette: &'a Palette) -> Self {
        let px = |v: f32| (v * scale).round();
        let pad = px(14.0);
        let code_font = code_font(palette, scale);
        let ui = Font::new(palette.ui_font.clone(), (palette.ui_font_size * scale).round());
        let small =
            Font::new(palette.ui_font.clone(), ((palette.ui_font_size - 1.0) * scale).round());
        let chip_font =
            Font::new(palette.code_font.clone(), ((palette.code_font_size - 1.0) * scale).round());
        let (cm, um) = (ts.metrics(&code_font), ts.metrics(&ui));
        let code_h = (cm.ascent + cm.descent + px(5.0)).round();
        let ui_h = (um.ascent + um.descent + px(5.0)).round();
        Self {
            palette,
            scale,
            max_w,
            pad,
            inner: max_w - 2.0 * pad,
            code_font,
            ui,
            small,
            chip_font,
            um,
            cm,
            code_h,
            ui_h,
            ops: Vec::new(),
            y: pad,
            widest: 0.0,
        }
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// The declaring type: its kind's badge, its name, the subtitle dimmed.
    fn title(&mut self, card: &HoverCard, ts: &mut TextSystem) {
        let Some((title, kind)) = &card.title else { return };
        let (palette, scale, pad, ui_h) = (self.palette, self.scale, self.pad, self.ui_h);
        let px = |v: f32| (v * scale).round();
        let y = self.y;
        let mut x = pad;
        let size = px(16.0);
        if let Some(kind) = kind {
            let (letter, color) = crate::completion_popup::badge(*kind, palette);
            let top = y + (ui_h - size) / 2.0;
            self.ops.push(Op::Fill {
                rect: Rect::new(x, top, size, size),
                color: color.with_alpha(0.22),
                radius: px(4.0),
            });
            let badge_font = Font::new(palette.code_font.clone(), (10.5 * scale).round())
                .with_weight(Font::SEMIBOLD);
            let shaped = ts.shape_line(letter, &badge_font);
            let bm = ts.metrics(&badge_font);
            self.ops.push(Op::Text {
                x: (x + (size - shaped.width) / 2.0).round(),
                baseline: centered_baseline(top, size, &bm),
                line: shaped,
                color,
            });
            x += size + px(8.0);
        }
        let font = self.ui.clone().with_weight(Font::SEMIBOLD);
        // Single-line texts (a declaration's name or type may be written over lines).
        let shaped = ts.shape_line(&one_line(title), &font);
        let baseline = centered_baseline(y, ui_h, &self.um);
        x += shaped.width;
        self.ops.push(Op::Text {
            x: x - shaped.width,
            baseline,
            line: shaped,
            color: palette.accent,
        });
        if let Some(subtitle) = &card.subtitle {
            let room = (pad + self.inner - x - px(10.0)).max(0.0);
            let shown = ds::ellipsize(ts, &one_line(subtitle), &self.small, room);
            let shaped = ts.shape_line(&shown, &self.small);
            x += px(10.0);
            self.ops.push(Op::Text {
                x,
                baseline,
                line: Arc::clone(&shaped),
                color: palette.text_muted,
            });
            x += shaped.width;
        }
        self.widest = self.widest.max(x - pad);
        self.y += ui_h + px(8.0);
    }

    /// The code, on a soft panel (its width is set once the card's is known).
    fn code(&mut self, card: &HoverCard, ts: &mut TextSystem) -> CodePanel {
        let code_top = self.y;
        let code_panel = self.ops.len();
        self.ops.push(Op::Fill {
            rect: Rect::default(),
            color: self.palette.text.with_alpha(0.05),
            radius: self.px(8.0),
        });
        self.y += self.px(8.0);
        let (x, room) = (self.pad + self.px(10.0), self.inner - self.px(20.0));
        for line in card.code.lines() {
            for row in wrap_code(line, &card.extension, room, &self.code_font, ts, self.palette) {
                let baseline = centered_baseline(self.y, self.code_h, &self.cm);
                let mut at = x;
                for (piece, color) in row {
                    let shaped = ts.shape_line(piece, &self.code_font);
                    let w = shaped.width;
                    self.ops.push(Op::Text { x: at, baseline, line: shaped, color });
                    at += w;
                }
                self.widest = self.widest.max(at - x);
                self.y += self.code_h;
            }
        }
        self.y += self.px(8.0);
        self.widest += self.px(20.0);
        (code_panel, code_top, self.y)
    }

    /// The doc comment's summary.
    fn doc(&mut self, card: &HoverCard, ts: &mut TextSystem) {
        let Some(doc) = card.doc.as_deref().filter(|d| !d.is_empty()) else { return };
        self.y += self.px(8.0);
        for paragraph in doc.lines() {
            for line in flow(paragraph, self.inner, self.inner, &self.ui, ts) {
                let shaped = ts.shape_line(&line, &self.ui);
                self.widest = self.widest.max(shaped.width);
                self.ops.push(Op::Text {
                    x: self.pad,
                    baseline: centered_baseline(self.y, self.ui_h, &self.um),
                    line: shaped,
                    color: self.palette.text,
                });
                self.y += self.ui_h;
            }
            self.y += self.px(4.0);
        }
    }

    /// The notes, as pills in a row.
    fn notes(&mut self, card: &HoverCard, ts: &mut TextSystem) {
        if card.notes.is_empty() {
            return;
        }
        let (palette, pad) = (self.palette, self.pad);
        self.y += self.px(8.0);
        let y = self.y;
        let pill_h = self.px(NOTE_H);
        let mut x = pad;
        for (note, warning) in &card.notes {
            let tone = if *warning { palette.warning } else { palette.accent };
            let badge = CardBadge::note(one_line(note).into_owned(), tone, self.small.clone());
            let w = badge.badge().width(ts, &Ctx::new(palette, self.scale));
            self.ops.push(Op::Badge { x, mid: y + pill_h / 2.0, badge });
            x += w + self.px(6.0);
        }
        self.widest = self.widest.max(x - pad);
        self.y += pill_h;
    }

    /// Parameters and source, in two columns: the labels, then the values. Returns where
    /// the separator above them goes.
    fn columns(&mut self, card: &HoverCard, ts: &mut TextSystem) -> f32 {
        let (pad, ui_h) = (self.pad, self.ui_h);
        self.y += self.px(12.0);
        let separator_y = self.y;
        self.y += self.px(12.0);
        let label_w = [&card.labels.0, &card.labels.1]
            .iter()
            .map(|l| ts.shape_line(l, &self.ui).width)
            .fold(0.0, f32::max);
        let vx = pad + label_w + self.px(12.0);
        let value_w = (self.inner - (vx - pad)).max(self.px(160.0));
        let chip_h = self.px(22.0);
        let cols = Columns { vx, value_w, chip_h, row_h: chip_h.max(ui_h) + self.px(4.0) };
        if !card.params.is_empty() {
            self.row_label(ts, &card.labels.0, cols);
            for param in &card.params {
                let x = self.param_chips(ts, param, &card.extension, cols);
                if let Some(doc) = param.doc.as_deref() {
                    self.param_doc(ts, doc, x, cols);
                }
                self.y += cols.row_h;
            }
            self.y += self.px(6.0);
        }
        if let Some(source) = &card.source {
            self.row_label(ts, &card.labels.1, cols);
            self.source(ts, source, cols);
        }
        separator_y
    }

    /// A row's label, in the first column.
    fn row_label(&mut self, ts: &mut TextSystem, text: &str, cols: Columns) {
        let shaped = ts.shape_line(text, &self.ui);
        self.ops.push(Op::Text {
            x: self.pad,
            baseline: centered_baseline(self.y, cols.row_h, &self.um),
            line: shaped,
            color: self.palette.text_muted,
        });
    }

    /// A parameter's `Type` and `$name` as chips. Returns where they end.
    fn param_chips(
        &mut self,
        ts: &mut TextSystem,
        param: &CardParam,
        extension: &str,
        cols: Columns,
    ) -> f32 {
        let (palette, y, Columns { vx, chip_h, row_h, .. }) = (self.palette, self.y, cols);
        let mut x = vx;
        // The code's colors: built-in types as keywords, names as variables.
        let (keyword, variable) =
            palette.syntax.map_or((palette.terminal_ansi[5], palette.terminal_ansi[1]), |c| {
                (c.keyword, c.variable)
            });
        let ty_color = match param.ty.as_deref() {
            Some(ty) if crate::code_tokens::is_keyword(extension, ty.trim_start_matches('?')) => {
                keyword
            }
            _ => palette.code_text,
        };
        let chips = [(param.ty.as_deref(), ty_color), (Some(param.name.as_str()), variable)];
        for (chip, color) in chips {
            let Some(chip) = chip else { continue };
            let badge = CardBadge {
                label: one_line(chip).into_owned(),
                color,
                fill: Some(palette.text.with_alpha(CODE_FILL)),
                font: self.chip_font.clone(),
                height: chip_h / self.scale,
                radius: Some(CODE_RADIUS),
                pad: CODE_PAD,
            };
            let w = badge.badge().width(ts, &Ctx::new(palette, self.scale));
            self.ops.push(Op::Badge { x, mid: y + row_h / 2.0, badge });
            x += w + self.px(4.0);
        }
        self.widest = self.widest.max(x - self.pad);
        x
    }

    /// A parameter's `– description`, beside its chips (ending at `x`), then under them.
    fn param_doc(&mut self, ts: &mut TextSystem, doc: &str, x: f32, cols: Columns) {
        let Columns { vx, value_w, row_h, .. } = cols;
        let first = (vx + value_w - x - self.px(4.0)).max(self.px(40.0));
        let lines = flow(&format!("– {doc}"), first, value_w, &self.ui, ts);
        for (i, line) in lines.iter().enumerate() {
            // Continuation lines hang under the description's start (unless the chips take
            // most of the row).
            let hang = if x - vx < value_w * 0.5 { x + self.px(2.0) } else { vx };
            let lx = if i == 0 { x + self.px(2.0) } else { hang };
            let shaped = ts.shape_line(line, &self.ui);
            self.widest = self.widest.max(lx + shaped.width - self.pad);
            self.ops.push(Op::Text {
                x: lx,
                baseline: centered_baseline(self.y, row_h, &self.um),
                line: shaped,
                color: self.palette.text,
            });
            if i + 1 < lines.len() {
                self.y += self.ui_h;
            }
        }
    }

    /// The source file's path, broken after `/`: the directories dimmed, the file name
    /// (the last line's end) plain.
    fn source(&mut self, ts: &mut TextSystem, source: &str, cols: Columns) {
        let (palette, vx) = (self.palette, cols.vx);
        let lines = flow_path(source, cols.value_w, &self.ui, ts);
        for (i, line) in lines.iter().enumerate() {
            let baseline = centered_baseline(self.y, cols.row_h, &self.um);
            let (dir, file) = match line.rfind(['/', '\\']) {
                _ if i + 1 < lines.len() => (line.as_str(), ""),
                Some(at) => line.split_at(at + 1),
                None => ("", line.as_str()),
            };
            let mut lx = vx;
            for (part, color) in [(dir, palette.text_muted), (file, palette.text)] {
                if part.is_empty() {
                    continue;
                }
                let shaped = ts.shape_line(part, &self.ui);
                let w = shaped.width;
                self.ops.push(Op::Text { x: lx, baseline, line: shaped, color });
                lx += w;
            }
            self.widest = self.widest.max(lx - self.pad);
            self.y += self.ui_h;
        }
        self.y += cols.row_h - self.ui_h;
    }

    /// The card's size, the code panel sized to it, the separator (when there are
    /// parameters or a source) across it.
    fn finish(mut self, code: CodePanel, separator: Option<f32>) -> Layout {
        let (scale, pad) = (self.scale, self.pad);
        let w = (self.widest + 2.0 * pad).min(self.max_w).round();
        let (index, top, bottom) = code;
        self.ops[index] = code_fill(pad, top, w, bottom, scale, self.palette);
        let Some(separator_y) = separator else {
            return Layout { scale, size: (w, (self.y + pad).round()), ops: self.ops };
        };
        self.ops.push(Op::Fill {
            rect: Rect::new(
                pad - self.px(4.0),
                separator_y,
                w - 2.0 * pad + self.px(8.0),
                (scale).round().max(1.0),
            ),
            color: self.palette.border,
            radius: 0.0,
        });
        Layout { scale, size: (w, (self.y + pad - self.px(6.0)).round()), ops: self.ops }
    }
}

/// The soft panel behind the code, across the card's inner width.
fn code_fill(pad: f32, top: f32, card_w: f32, bottom: f32, scale: f32, palette: &Palette) -> Op {
    let px = |v: f32| (v * scale).round();
    let inset = pad - px(4.0);
    Op::Fill {
        rect: Rect::new(inset, top, card_w - 2.0 * inset, bottom - top),
        color: palette.text.with_alpha(0.05),
        radius: px(8.0),
    }
}

#[cfg(test)]
#[path = "../tests/unit/code_hover.rs"]
mod tests;

/// A note's badge: its height and the room on each side of its text (logical pixels).
const NOTE_H: f32 = 20.0;
const NOTE_PAD: f32 = 8.0;
/// A parameter's code chip: its fill (the text's alpha), its corners and the room on each
/// side of its text (logical pixels).
const CODE_FILL: f32 = 0.06;
const CODE_RADIUS: f32 = 5.0;
const CODE_PAD: f32 = 6.0;

/// A badge of the card, kept with its layout: its text, its color (its fill made from it,
/// or its own), its font, its height (logical), its corners (`None`: a pill), its padding.
#[derive(Debug)]
struct CardBadge {
    label: String,
    color: Rgba,
    fill: Option<Rgba>,
    font: Font,
    height: f32,
    radius: Option<f32>,
    pad: f32,
}

impl CardBadge {
    /// A note in `color`: a pill tinted in it.
    fn note(label: String, color: Rgba, font: Font) -> Self {
        Self { label, color, fill: None, font, height: NOTE_H, radius: None, pad: NOTE_PAD }
    }

    /// The shared badge it is.
    fn badge(&self) -> ds::Badge<'_> {
        let mut badge = ds::Badge::new(&self.label)
            .color(self.color)
            .size(ds::BadgeSize::Custom(self.height))
            .pad(self.pad)
            .font(&self.font)
            .baseline(ds::BadgeBaseline::Metrics);
        if let Some(fill) = self.fill {
            badge = badge.fill(fill);
        }
        if let Some(radius) = self.radius {
            badge = badge.radius(radius);
        }
        badge
    }
}
