//! The code editor view: gutter (numbers + folding), colored text, cursors,
//! selections, search matches, collapsed regions.
//!
//! Each tab has its own `EditorView` (scroll, layout cache). Shaping is done only
//! for visible rows; long lines are truncated at the right edge. Displayed rows
//! differ from document lines when there are collapsed regions ([`Folds`]).

use std::fmt::Write as _;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use document::Document;
use kernel::{Point, Rect, Rgba};
use render_api::{GlyphInstance, Quad, Scene};
use text_layout::{Font, FontFamily, FontMetrics, ShapedLine, TextSystem};
use ui_kit::{centered_baseline, draw_text};

use crate::Painter;
use crate::anim::Tween;
use crate::folding::Folds;
use crate::palette::Palette;
use crate::splitter::Orientation;
use ui_components::scrollbar::{ScrollMetrics, Scrollbar, ScrollbarStyle};

mod access;
mod decorations;
mod errors;
mod gutter;
mod highlights;
mod hits;
mod text;

pub use decorations::*;

use display::*;
mod display;
mod rows;
mod scroll;
mod wrap;

pub use rows::RowMove;
use rows::Seg;
use wrap::WrapRows;

/// The columns a caret's position is shaped in steps of (see `EditorView::x_in_line`).
const X_STEP: usize = 1024;

/// A visible line longer than this (bytes) has only its drawn part highlighted
/// ([`EditorView::long_line_windows`]).
const LONG_LINE: usize = 16 * 1024;

/// A byte's face: plain, or these bits.
const BOLD: u8 = 1;
const ITALIC: u8 = 2;

/// What the strip's mark lines were computed for: the document version, the lists'
/// generation ([`Decorations::marks_generation`]) and, per list, its length and its first
/// and last offsets.
type MarkKey = (u64, u64, [(usize, usize, usize); 4]);

/// What the widest line depends on: the document (its id, version and length), its tab width
/// and the folds' hidden lines (their generation). Not the font: it's counted in columns.
type WidthKey = (kernel::DocumentId, u64, usize, u32, u64);

fn mark_key(version: u64, generation: u64, lists: [&[Range<usize>]; 4]) -> MarkKey {
    let sum = |l: &[Range<usize>]| {
        (l.len(), l.first().map_or(0, |r| r.start), l.last().map_or(0, |r| r.start))
    };
    (version, generation, lists.map(sum))
}

/// Dimensions computed for the current scale factor, in physical pixels.
#[derive(Debug, Clone, Copy)]
struct Layout {
    line_height: f32,
    advance: f32,
    ascent: f32,
    descent: f32,
    gutter_width: f32,
    /// Width of the annotation column (git blame) at the gutter's left; 0 when hidden.
    annotation: f32,
    /// Distance from the editor's left edge to the start of the text.
    text_left: f32,
    caret_width: f32,
}

/// What one frame of [`EditorView::paint`] draws from, computed once at its start: the
/// document and its decorations, the layout, the fonts, where the text and the rows are.
struct Frame<'a> {
    doc: &'a Document,
    deco: Decorations<'a>,
    focused: bool,
    layout: Layout,
    font: Font,
    /// The collapsed regions' labels.
    ui_font: Font,
    scale: f32,
    bounds: Rect,
    text_area: Rect,
    /// Where the text starts (scrolled), in whole pixels.
    x0: f32,
    /// How many displayed characters a row can show (the rest is not shaped).
    max_columns: usize,
    first_row: u32,
    scroll_y: f32,
}

impl Frame<'_> {
    /// The top of visible row `i`. Whole pixels: with a fractional scroll (trackpad
    /// momentum) text, selections and the gutter would round differently and shift by a
    /// pixel between frames (jitter).
    fn row_top(&self, i: usize) -> f32 {
        (self.bounds.min_y() + (self.first_row as usize + i) as f32 * self.layout.line_height
            - self.scroll_y)
            .round()
    }

    fn row_baseline(&self, i: usize) -> f32 {
        baseline(&self.layout, self.row_top(i))
    }
}

/// A visible row being drawn: its document line, its top and its text's baseline.
#[derive(Clone, Copy)]
struct Row {
    line: u32,
    top: f32,
    baseline: f32,
}

/// Where the vertical strip's marks go: its track, the span the thumb moves along, the
/// document's line count, how wide the strip is (0 = narrow, 1 = wide), the scale.
#[derive(Clone, Copy)]
struct Strip {
    track: Rect,
    span: (f32, f32),
    lines: f32,
    wide: f32,
    scale: f32,
}

impl Strip {
    /// The top of a mark `h` tall for `line` (its proportional position).
    fn y_of(&self, line: u32, h: f32) -> f32 {
        let line = line as f32;
        (self.span.0 + line / self.lines * (self.span.1 - self.span.0 - h)).round()
    }
}

#[derive(Debug)]
pub struct EditorView {
    /// Text size relative to the theme (0 = uninitialized, i.e. 100%).
    zoom: f32,
    /// Where the last link was drawn (⌘ + hover), for the badge above it.
    link_rect: Option<Rect>,
    font_family: FontFamily,
    bounds: Rect,
    scale: f32,
    scroll_x: f32,
    /// Vertical scroll in pixels, over displayed rows (not document lines).
    scroll_y: f32,
    max_scroll_x: f32,
    layout: Option<Layout>,
    /// Code font size in physical pixels (computed together with the layout).
    font_size_px: f32,
    /// Digit count of the last line for which the gutter was computed.
    gutter_digits: usize,
    /// Characters reserved for annotations (git blame) in the gutter; 0 = none.
    annotation_chars: usize,
    /// The annotation rows (from the last paint), with their line.
    annotation_rects: Vec<(Rect, u32)>,
    /// The statements' marks drawn (a module's: clickable), by their first line.
    statement_rects: Vec<(Rect, u32)>,
    preedit: Option<String>,
    /// Column of the vertical guide (e.g. `max_line_length` from `.editorconfig`).
    ruler: Option<u32>,
    /// Click areas of the collapsed regions (the `⋯` label), from the last paint.
    pills: Vec<(Rect, u32)>,
    /// The gutter color swatches (from the last paint), with their line.
    swatch_rects: Vec<(Rect, u32)>,
    /// The gutter's change bars (from the last paint), with the change's index.
    vcs_rects: Vec<(Rect, usize)>,
    /// The line whose folding arrow is under the mouse.
    hovered_marker: Option<u32>,
    /// The error mark under the mouse (the error's offset), on the scroll strip.
    hovered_error: Option<(usize, f32)>,
    /// Occurrence marks on the scroll strip: (y, offset).
    occurrence_marks: Vec<(f32, usize)>,
    /// A mark the lane's thumb was pressed on (its offset, where it was pressed): taken on
    /// letting go if the thumb wasn't moved.
    press_mark: Option<(usize, f32)>,
    /// Widening of the scroll strip on hover (0 = narrow, 1 = wide).
    bar_wide: Tween,
    /// Appearance of the preview for the error under the mouse (its offset, 0 → 1).
    preview: Option<(usize, Tween)>,
    /// The scrollbars (right and bottom), hidden at rest: shown by a scroll, kept while the
    /// mouse is on one ([`AutoHide`]); the marks on the right strip (errors, changes) stay.
    vbar: Scrollbar,
    vauto: ui_components::AutoHide,
    hauto: ui_components::AutoHide,
    /// Its bars' state of use (see `scroll::with_bars`).
    bars: ui_components::control::Controls,
    /// Error marks on the vertical strip: (the mark's y, the error's offset).
    error_marks: Vec<(f32, usize)>,
    /// The lines of the warnings, errors and occurrences marked on the strip, for the
    /// document version and the lists they come from (not looked up every frame).
    mark_lines: kernel::Memo<MarkKey, [Vec<u32>; 4]>,
    hbar: Scrollbar,
    // Buffers reused across frames (no allocations in steady state).
    line: String,
    display: String,
    number: String,
    /// Cursor lines (sorted), computed once per frame for the gutter.
    cursor_lines: Vec<usize>,
    rects: Vec<(Rect, Rgba)>,
    /// The visible rows' indentation (display columns; `None`: a blank line), for the
    /// indent guides — reused between frames.
    indents: Vec<Option<usize>>,
    lines: Vec<u32>,
    starts: Vec<usize>,
    buckets: Vec<Vec<usize>>,
    colors: Vec<Rgba>,
    /// Each byte's face in the row (`BOLD` / `ITALIC`), beside its color.
    faces: Vec<u8>,
    disp_to_src: Vec<u32>,
    /// The bold / italic runs' scratch (`restyle_glyphs`), reused between rows and frames.
    restyle: Restyle,
    /// The widest line's display columns over the whole document (not just the visible
    /// rows), for the horizontal scroll range: worked out once per text / tab / folds.
    longest_line: kernel::Memo<WidthKey, (usize, u32)>,
    /// The inlay hints (sorted by offset), as last given ([`Self::set_inlays`]).
    inlays: Arc<[Inlay]>,
    /// The inlays of the line in `line` (its bytes, their columns), reused.
    line_inlays: Vec<(usize, usize)>,
    /// Where the bulb was drawn (the last paint).
    bulb_rect: Option<Rect>,
    /// Where each lens part was drawn (the last paint): its lens's index in the
    /// decorations' and its own.
    lens_rects: Vec<(Rect, usize, usize)>,
    /// The font advance the scroll was last clamped for (a zoom changes the width in pixels).
    width_advance: f32,
    /// Lines wrap at the text's width ([`ui_components::TextOptions::word_wrap`]).
    wrap: bool,
    /// The columns a wrapped row holds (from the last frame's width; 0: none yet).
    wrap_columns: usize,
    /// The wrapped rows, while lines wrap.
    wrap_rows: WrapRows,
    /// The loaded line's parts as wrapped, and what they were made for (the document, its
    /// version, the line, the columns, the tab width): a line wrapped once, whatever asks for
    /// its rows.
    wrap_parts: Vec<Range<usize>>,
    wrap_parts_for: Option<(kernel::DocumentId, u64, usize, usize, u32)>,
    /// The hints those parts were wrapped with (kept: their list is the key, never an
    /// address another list could take).
    wrap_parts_hints: Option<Arc<[Inlay]>>,
    /// The loaded line's later rows' indentation (columns), wrapped with its parts.
    wrap_hang: usize,
    /// The line in `line`: its document, version and number (what its parts are for).
    loaded: Option<(kernel::DocumentId, u64, usize)>,
    /// The visible rows' parts of their lines (beside `lines`).
    segs: Vec<Seg>,
    /// Where the row drawn now starts in its line's display (its glyphs' bytes are from
    /// there): 0 but for a wrapped line's later rows.
    row_base: usize,
    /// How far right the row drawn now starts (pixels): a wrapped line's later rows, under
    /// its text; 0 otherwise.
    row_shift: f32,
    /// How many times [`Self::longest_line`] was worked out (the tests check it's not
    /// per frame).
    #[cfg(test)]
    width_scans: u32,
}

/// The scratch of [`EditorView::restyle_glyphs`]: the runs (face, first glyph, end), which
/// plain glyphs stay, the restyled glyphs.
#[derive(Debug, Default)]
struct Restyle {
    runs: Vec<(u8, usize, usize)>,
    keep: Vec<bool>,
    styled: Vec<GlyphInstance>,
}

impl EditorView {
    pub fn new(font_family: FontFamily) -> Self {
        Self {
            link_rect: None,
            font_family,
            bounds: Rect::default(),
            scale: 1.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            max_scroll_x: 0.0,
            layout: None,
            font_size_px: 13.0,
            gutter_digits: 0,
            annotation_chars: 0,
            annotation_rects: Vec::new(),
            statement_rects: Vec::new(),
            preedit: None,
            ruler: None,
            pills: Vec::new(),
            zoom: 1.0,
            swatch_rects: Vec::new(),
            vcs_rects: Vec::new(),
            hovered_marker: None,
            hovered_error: None,
            occurrence_marks: Vec::new(),
            press_mark: None,
            bar_wide: Tween::new(0.0, Duration::from_millis(160)),
            preview: None,
            vbar: Scrollbar::new(ScrollbarStyle::default(), Orientation::Vertical),
            vauto: ui_components::AutoHide::new(true),
            hauto: ui_components::AutoHide::new(true),
            bars: ui_components::control::Controls::default(),
            error_marks: Vec::new(),
            mark_lines: kernel::Memo::default(),
            hbar: Scrollbar::new(ScrollbarStyle::default(), Orientation::Horizontal),
            line: String::new(),
            display: String::new(),
            number: String::new(),
            cursor_lines: Vec::new(),
            rects: Vec::new(),
            indents: Vec::new(),
            lines: Vec::new(),
            starts: Vec::new(),
            buckets: Vec::new(),
            colors: Vec::new(),
            faces: Vec::new(),
            disp_to_src: Vec::new(),
            restyle: Restyle::default(),
            longest_line: kernel::Memo::default(),
            inlays: Arc::from(Vec::new()),
            line_inlays: Vec::new(),
            bulb_rect: None,
            lens_rects: Vec::new(),
            width_advance: 0.0,
            wrap: false,
            wrap_columns: 0,
            wrap_rows: WrapRows::default(),
            wrap_parts: Vec::new(),
            wrap_parts_for: None,
            wrap_parts_hints: None,
            wrap_hang: 0,
            loaded: None,
            segs: Vec::new(),
            row_base: 0,
            row_shift: 0.0,
            #[cfg(test)]
            width_scans: 0,
        }
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The inlay hints shown from now on (sorted by offset; the same list again costs
    /// nothing).
    pub fn set_inlays(&mut self, inlays: Arc<[Inlay]>) {
        if !Arc::ptr_eq(&self.inlays, &inlays) {
            self.inlays = inlays;
        }
    }

    /// The inlay hints shown.
    pub fn inlays(&self) -> &Arc<[Inlay]> {
        &self.inlays
    }

    /// The bulb is under `p` (the last paint's).
    pub fn bulb_at(&self, p: Point) -> bool {
        self.bulb_rect.is_some_and(|r| r.contains(p))
    }

    /// The lens part under `p` (its lens's index in the decorations' lenses, its own),
    /// from the last paint.
    pub fn lens_at(&self, p: Point) -> Option<(usize, usize)> {
        self.lens_rects.iter().find(|(r, ..)| r.contains(p)).map(|(_, i, part)| (*i, *part))
    }

    /// Line `line` of `doc` into `self.line`, its inlays into `self.line_inlays`.
    fn load_line(&mut self, doc: &Document, line: usize) {
        let buffer = doc.buffer();
        buffer.write_line(line, &mut self.line);
        self.loaded = Some((doc.id(), doc.version(), line));
        self.line_inlays.clear();
        if self.inlays.is_empty() {
            return;
        }
        let start = buffer.line_start(line);
        let end = start + self.line.len();
        let first = self.inlays.partition_point(|i| i.offset < start);
        for inlay in self.inlays[first..].iter().take_while(|i| i.offset <= end) {
            self.line_inlays.push((inlay.offset - start, inlay.columns()));
        }
    }

    pub fn set_font_family(&mut self, family: FontFamily) {
        if self.font_family != family {
            self.font_family = family;
            self.layout = None;
        }
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON {
            let rows = self.layout.map(|l| self.scroll_y / l.line_height);
            self.scale = scale;
            self.layout = None;
            if let Some(rows) = rows {
                self.scroll_y = (rows * self.line_height_estimate(&Palette::default())).round();
            }
        }
        self.bounds = bounds;
    }

    pub fn set_ruler(&mut self, column: Option<u32>) {
        self.ruler = column;
    }

    /// Text being composed via IME, shown at the cursor (not yet in the document).
    pub fn set_preedit(&mut self, text: Option<String>) {
        self.preedit = text.filter(|t| !t.is_empty());
    }

    pub fn line_height(&self) -> f32 {
        self.layout.map_or(16.0 * self.scale, |l| l.line_height)
    }

    /// How many whole rows fit in the viewport (for PageUp / PageDown).
    pub fn page_lines(&self) -> u32 {
        ((self.bounds.size.height / self.line_height()).floor() as u32).saturating_sub(1).max(1)
    }

    /// The byte range of the visible rows (for syntax highlighting).
    pub fn visible_bytes(&self, doc: &Document, folds: &Folds) -> Range<usize> {
        let buffer = doc.buffer();
        let lh = self.line_height();
        let first_row = (self.scroll_y / lh).floor().max(0.0) as u32;
        let rows = (self.bounds.size.height / lh).ceil() as u32 + 1;
        let first = self.line_of_row(folds, first_row) as usize;
        let last = (self.line_of_row(folds, first_row + rows) as usize)
            .min(buffer.line_count().saturating_sub(1));
        buffer.line_start(first)..buffer.line_end(last)
    }

    /// For visible rows too long to highlight whole (a minified file's): each visible line's
    /// part that can be drawn (rows are drawn from their start to the view's right edge, in
    /// steps of [`X_STEP`] columns so a small scroll keeps them), the short ones whole and
    /// merged. `None`: no such row, the rows are highlighted whole ([`Self::visible_bytes`]).
    pub fn long_line_windows(&self, doc: &Document, folds: &Folds) -> Option<Vec<Range<usize>>> {
        let layout = self.layout?;
        let buffer = doc.buffer();
        let lh = layout.line_height;
        let first_row = (self.scroll_y / lh).floor().max(0.0) as u32;
        let rows = (self.bounds.size.height / lh).ceil() as u32 + 1;
        let first = self.line_of_row(folds, first_row) as usize;
        let last = (self.line_of_row(folds, first_row + rows) as usize)
            .min(buffer.line_count().saturating_sub(1));
        let columns = ((self.scroll_x + self.bounds.size.width) / layout.advance.max(1.0)).ceil()
            as usize
            + 2;
        // A column is at most 4 bytes of the source (tabs and hints take columns, not bytes).
        let cap = columns.div_ceil(X_STEP) * X_STEP * 4;
        let long = cap.max(LONG_LINE);
        if !(first..=last).any(|line| buffer.line_len(line) > long) {
            return None;
        }
        let mut windows: Vec<Range<usize>> = Vec::new();
        for line in first..=last {
            let start = buffer.line_start(line);
            let end = buffer.line_end(line);
            let end = if end - start > cap { buffer.floor_char_boundary(start + cap) } else { end };
            match windows.last_mut() {
                Some(w) if start <= w.end + 1 => w.end = end,
                _ => windows.push(start..end),
            }
        }
        Some(windows)
    }

    /// Its scrollbars held shown: the mouse on one since it showed, or one dragged.
    fn bars_held(&self) -> (bool, bool) {
        (
            self.vauto.held(self.vbar.is_hovered(), self.vbar.is_dragging()),
            self.hauto.held(self.hbar.is_hovered(), self.hbar.is_dragging()),
        )
    }

    /// When its scrollbars next need a frame (fading in or out, a hold ending); `None`: at
    /// rest — no frames.
    /// Where it's scrolled to (x, y): what's shown moved if it changed.
    pub fn scroll_position(&self) -> (f32, f32) {
        (self.scroll_x, self.scroll_y)
    }

    pub fn scrollbar_deadline(&self) -> Option<std::time::Instant> {
        let now = std::time::Instant::now();
        let (v, h) = self.bars_held();
        [self.vauto.next_frame_at(now, v), self.hauto.next_frame_at(now, h)]
            .into_iter()
            .flatten()
            .min()
    }

    /// A transition is in progress (scroll strip, preview): frames must be requested.
    pub fn is_animating(&self) -> bool {
        self.bar_wide.animating() || self.preview.is_some_and(|(_, t)| t.animating())
    }

    /// The badge above the link (⌘ + hover): the key and the target, e.g. `⌘ click` →
    /// `User.php:12`. Drawn after [`EditorView::paint`], over the text.
    pub fn paint_link_badge(
        &self,
        key: &str,
        label: &str,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let Some(link) = self.link_rect else { return };
        let s = self.scale;
        let px = |v: f32| (v * s).round();
        let style = palette.key_hints(s);
        let width = ui_kit::key_hints_width(text, &[(&[key], label)], &style);
        let (w, h) = (width + px(20.0), px(26.0));
        let b = self.bounds;
        let x = link.min_x().min(b.max_x() - w - px(6.0)).max(b.min_x() + px(4.0));
        // Above the word; if it doesn't fit (first line), below.
        let mut y = link.min_y() - h - px(4.0);
        if y < b.min_y() {
            y = link.max_y() + px(4.0);
        }
        let badge = Rect::new(x.round(), y.round(), w.round(), h);
        let layer = scene.push_layer(None);
        let ctx = crate::ds::Ctx::new(palette, s);
        crate::ds::surface(layer, &ctx, badge, crate::ds::Elevation::Low, h / 2.0 / s);
        layer.quad(
            Quad::filled(badge, palette.popup_background)
                .rounded(h / 2.0)
                .bordered(s.round().max(1.0), palette.accent.with_alpha(0.45)),
        );
        let row = Rect::new(badge.min_x() + px(10.0), badge.min_y(), badge.size.width, h);
        let style = ui_kit::KeyHintsStyle { text: palette.accent, ..style };
        ui_kit::paint_key_hints(layer, text, &[(&[key], label)], row, &style);
    }

    pub fn paint(
        &mut self,
        doc: &Document,
        deco: Decorations<'_>,
        focused: bool,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let f = self.begin_frame(doc, deco, focused, text, palette);
        self.paint_current_lines(&f, scene, palette);
        self.bucket_colors(&f);
        // Backgrounds in the text area: selections, then matches (translucent, on top).
        let highlights = scene.push_layer(Some(f.text_area));
        let widest = self.paint_highlights(&f, text, highlights, palette);
        self.paint_indent_guides(&f, highlights, palette);
        // The document's widest line, or a visible row shaped wider (a ligature, a fallback
        // font's glyph).
        let widest =
            widest.max(self.longest_line.value().map_or(0, |w| w.0) as f32 * f.layout.advance);
        // Wrapped, every row fits: nothing to scroll sideways.
        self.max_scroll_x = if self.wraps() {
            0.0
        } else {
            (widest - f.text_area.size.width + f.layout.advance * 8.0).max(0.0)
        };
        self.paint_ruler(&f, highlights, palette);
        // The code and its line numbers aren't read as loose texts (a token at a time): the
        // editor is told as a text area of its own.
        if ui_kit::access::recording() {
            let node = self.described(&f, deco.folds, text, palette);
            ui_kit::access::node(|| node);
        }
        ui_kit::access::silent(|| {
            self.paint_gutter(&f, text, scene, palette);
            self.paint_text(&f, text, scene, palette);
        });
        // The cursors and the IME text being composed.
        if focused {
            self.paint_carets(&f, text, scene, palette);
            self.paint_preedit(&f, text, scene, palette);
        }
        let (vtrack, wide) = self.paint_scrollbars(&f, scene, palette);
        self.paint_strip_marks(&f, vtrack, wide, scene, palette);
    }

    /// The frame's geometry; fills `self.lines` with the visible rows' document lines.
    fn begin_frame<'a>(
        &mut self,
        doc: &'a Document,
        deco: Decorations<'a>,
        focused: bool,
        text: &mut TextSystem,
        palette: &Palette,
    ) -> Frame<'a> {
        let layout = self.layout(text, doc, palette);
        let font = self.font(palette);
        let ui_font =
            Font::new(palette.ui_font.clone(), (palette.ui_font_size * 0.92 * self.scale).round());
        let b = self.bounds;
        let folds = deco.folds;
        // The lane on the right is kept: no text under it.
        let lane = scroll::lane_width(self.scale);
        let text_area = Rect::new(
            b.min_x() + layout.gutter_width,
            b.min_y(),
            (b.size.width - layout.gutter_width - lane).max(0.0),
            b.size.height,
        );
        // Lines wrap at the text's width (a column kept for the wrap's mark): no
        // horizontal scroll then.
        self.wrap_columns = if self.wrap {
            ((text_area.size.width / layout.advance.max(1.0)).floor() as usize).saturating_sub(2)
        } else {
            0
        };
        self.refresh_rows(doc, folds);
        if self.wraps() {
            self.scroll_x = 0.0;
            self.max_scroll_x = 0.0;
        }
        // The horizontal range comes from the whole document: its widest line,
        // cached, so the bar doesn't come and go as a long line scrolls in and out of view.
        // When that width changes (the long line deleted, a region folded), the scroll is
        // brought back within it before anything is placed.
        let doc_width =
            if self.wraps() { None } else { self.document_width(doc, folds, layout.advance) };
        if let Some(width) = doc_width {
            let max = (width - text_area.size.width + layout.advance * 8.0).max(0.0);
            self.max_scroll_x = max;
            self.scroll_x = self.scroll_x.min(max).round();
        }
        let x0 = (b.min_x() + layout.text_left - self.scroll_x).round();
        let max_columns =
            ((self.scroll_x + text_area.size.width) / layout.advance).ceil() as usize + 2;

        // Visible rows → document lines (and the part of each a wrapped row shows).
        let first_row = (self.scroll_y / layout.line_height).floor().max(0.0) as u32;
        let row_count = (b.size.height / layout.line_height).ceil() as u32 + 1;
        self.fill_rows(doc, folds, first_row, row_count);
        Frame {
            doc,
            deco,
            focused,
            layout,
            font,
            ui_font,
            scale: self.scale,
            bounds: b,
            text_area,
            x0,
            max_columns,
            first_row,
            scroll_y: self.scroll_y,
        }
    }

    /// The document's widest line in pixels (its columns, cached per text, tab width and
    /// folds, times the font's advance); `Some` only when it changed (the text, the tab width,
    /// the folds or the font), so the scroll is clamped then, not on every frame.
    fn document_width(&mut self, doc: &Document, folds: &Folds, advance: f32) -> Option<f32> {
        let buffer = doc.buffer();
        let key = (doc.id(), doc.version(), buffer.len(), doc.tab_width(), folds.generation());
        let same_font = self.width_advance == advance;
        self.width_advance = advance;
        let columns = match self.longest_line.value() {
            Some(&(columns, _)) if self.longest_line.key() == Some(&key) => {
                if same_font {
                    return None;
                }
                columns
            }
            _ => {
                // After edits (the same document, tab width and folds): the changed lines
                // measured, the rest as it was — typing doesn't count the whole file.
                let edited = match (self.longest_line.key(), self.longest_line.value()) {
                    (Some(&(id, version, _, tab, generation)), Some(&widest))
                        if (id, tab, generation)
                            == (doc.id(), doc.tab_width(), folds.generation()) =>
                    {
                        doc.changes_since(version).and_then(|changes| {
                            widest_after(buffer, &changes, widest, tab, folds.hidden_ranges())
                        })
                    }
                    _ => None,
                };
                let widest = edited.unwrap_or_else(|| {
                    #[cfg(test)]
                    {
                        self.width_scans += 1;
                    }
                    widest_line(buffer.chunks(), doc.tab_width(), folds.hidden_ranges())
                });
                self.longest_line.set(key, widest).0
            }
        };
        Some(columns as f32 * advance)
    }

    fn font(&self, palette: &Palette) -> Font {
        Font::new(
            self.font_family.clone(),
            (palette.code_font_size * self.zoom() * self.scale).round(),
        )
    }

    fn line_height_estimate(&self, palette: &Palette) -> f32 {
        (palette.code_font_size * self.zoom() * self.scale).round() * palette.code_line_height
    }

    fn zoom(&self) -> f32 {
        if self.zoom > 0.0 { self.zoom } else { 1.0 }
    }

    /// Text size relative to the theme's (1 = 100%). The top row stays the same.
    pub fn set_zoom(&mut self, zoom: f32) {
        if (zoom - self.zoom()).abs() < f32::EPSILON {
            return;
        }
        let rows = self.layout.map(|l| self.scroll_y / l.line_height);
        self.zoom = zoom;
        self.layout = None;
        if let Some(rows) = rows {
            // Row height grows with the zoom (an estimate until the next layout).
            self.scroll_y = (rows * self.line_height_estimate(&Palette::default())).round();
        }
    }

    /// The x position (relative to the start of the text) of column `col` in the line held in
    /// `self.line`. Shaped up to a step of [`X_STEP`] columns past it (from the shaping cache:
    /// the whole line for an ordinary one, not a new prefix per column), so a caret on a
    /// minified file's one long line doesn't shape it whole on every frame.
    /// The x of byte `col` of the line in `self.line`, from the start of its row `seg`.
    fn x_in_row(&mut self, text: &mut TextSystem, doc: &Document, seg: Seg, col: usize) -> f32 {
        let x = self.x_in_line(text, doc, col);
        if seg.from == 0 { x } else { x - self.x_in_line(text, doc, seg.from) }
    }

    fn x_in_line(&mut self, text: &mut TextSystem, doc: &Document, col: usize) -> f32 {
        let tab = doc.tab_width();
        let disp = source_to_display_with(&self.line, col, tab, &self.line_inlays);
        if disp == 0 {
            return 0.0;
        }
        // A display byte's columns are at most its offset: this many hold it, and a few more
        // (a ligature across it shaped whole).
        let columns = ((disp + 8) / X_STEP + 1) * X_STEP;
        expand_tabs_with(&self.line, columns, tab, &self.line_inlays, &mut self.display);
        let font = Font::new(self.font_family.clone(), self.font_size_px);
        text.shape_line(&self.display, &font).x_at(disp)
    }

    fn layout(&mut self, text: &mut TextSystem, doc: &Document, palette: &Palette) -> Layout {
        let digits = doc.buffer().line_count().to_string().len().max(3);
        if let (Some(layout), true) = (self.layout, digits == self.gutter_digits) {
            return layout;
        }
        let font = self.font(palette);
        self.font_size_px = font.size;
        let m = text.metrics(&font);
        // The annotations (blame), line numbers, then the folding arrow strip.
        let annotation = if self.annotation_chars > 0 {
            ((self.annotation_chars as f32 + 2.0) * m.advance).round()
        } else {
            0.0
        };
        let gutter_width = ((digits as f32 + 5.0) * m.advance).round() + annotation;
        let layout = Layout {
            line_height: (font.size * palette.code_line_height).round(),
            advance: m.advance,
            ascent: m.ascent,
            descent: m.descent,
            gutter_width,
            annotation,
            text_left: gutter_width + (m.advance * 0.5).round(),
            caret_width: (2.0 * self.scale).round(),
        };
        self.layout = Some(layout);
        self.gutter_digits = digits;
        layout
    }
}

fn baseline(layout: &Layout, top: f32) -> f32 {
    (top + (layout.line_height - (layout.ascent + layout.descent)) / 2.0 + layout.ascent).round()
}

#[cfg(test)]
#[path = "../../tests/unit/editor_view.rs"]
mod tests;
