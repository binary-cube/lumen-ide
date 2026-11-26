//! Feedback: the keys' footer, the loader, the progress bar, the empty state, the tooltip.

use super::*;

/// A dialog footer: subtle background, top separator, keys on the left. Returns
/// [`footer`] at the bottom of a card with rounded corners (a view in the editor's card):
/// its bottom corners follow the card's (`radius`, logical).
pub fn footer_in_card(
    scene: &mut render_api::Scene,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    hints: &[KeyHint<'_>],
    radius: f32,
) {
    let r = ctx.px(radius).min(rect.size.height);
    let fill = ctx.fade(ctx.palette.text.with_alpha(super::BAND_TINT));
    // Square above the corners, rounded below them (no overlap: the fill is translucent).
    let layer = scene.push_layer(Some(rect));
    let top = Rect::new(rect.min_x(), rect.min_y(), rect.size.width, rect.size.height - r);
    layer.quad(Quad::filled(top, fill));
    let bottom = Rect::new(rect.min_x(), rect.max_y() - r, rect.size.width, r);
    let layer = scene.push_layer(Some(bottom));
    let round = Rect::new(rect.min_x(), rect.max_y() - 2.0 * r, rect.size.width, 2.0 * r);
    layer.quad(Quad::filled(round, fill).rounded(r));
    let layer = scene.push_layer(Some(rect));
    divider(layer, ctx, rect.min_x(), rect.max_x(), rect.min_y());
    // No actions on its right: the keys may take the whole width, the last ones left out
    // when it's narrow.
    let pad = ctx.px(space::LG + 2.0);
    let row = Rect::new(
        rect.min_x() + pad,
        rect.min_y() + ctx.line(),
        (rect.size.width - 2.0 * pad).max(0.0),
        rect.size.height - ctx.line(),
    );
    let style = ctx.palette.key_hints(ctx.scale);
    let hints = ui_kit::fitting_key_hints(text, hints, &style, row.size.width);
    paint_key_hints(layer, text, hints, row, &style);
}

/// the free right edge (for actions).
pub fn footer(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    hints: &[KeyHint<'_>],
) -> f32 {
    layer.quad(Quad::filled(rect, ctx.fade(ctx.palette.text.with_alpha(super::BAND_TINT))));
    divider(layer, ctx, rect.min_x(), rect.max_x(), rect.min_y());
    let row = Rect::new(
        rect.min_x() + ctx.px(space::LG + 2.0),
        rect.min_y() + ctx.line(),
        rect.size.width / 2.0,
        rect.size.height - ctx.line(),
    );
    paint_key_hints(layer, text, hints, row, &ctx.palette.key_hints(ctx.scale));
    rect.max_x() - ctx.px(space::LG + 2.0)
}

/// A modal's footer when its view draws it (`Viewer::draws_footer`): its height (logical
/// pixels), the keys on its left, its buttons on its right.
pub const MODAL_FOOTER_H: f32 = 60.0;

/// The bar of a modal's footer drawn by its view in `foot` (the bottom of the view's
/// area): its top line, its tint (its bottom corners following the window's). Returns the
/// layer its keys and buttons go in.
pub fn modal_footer_bar<'s>(
    scene: &'s mut render_api::Scene,
    ctx: &Ctx<'_>,
    foot: Rect,
) -> &'s mut Layer {
    let p = ctx.palette;
    let layer = scene.push_layer(Some(foot));
    layer.quad(Quad::filled(
        Rect::new(foot.min_x(), foot.min_y(), foot.size.width, ctx.line()),
        ctx.fade(p.card_border),
    ));
    let corner = ctx.px(radius::XL) - ctx.line();
    let tint = Rect::new(
        foot.min_x(),
        foot.min_y() - corner,
        foot.size.width,
        foot.size.height + corner - ctx.line(),
    );
    layer.quad(Quad::filled(tint, ctx.fade(p.text.with_alpha(super::BAND_TINT))).rounded(corner));
    layer
}

/// A button of a modal's footer.
#[derive(Debug, Clone, Copy, Default)]
pub struct FooterButton<'a> {
    pub label: &'a str,
    /// The main one: in the accent, with the key that presses it (`↵`).
    pub main: bool,
    /// What can't be undone: in the danger color.
    pub danger: bool,
    pub enabled: bool,
    pub hovered: bool,
    /// A `|` between it and the buttons on its right.
    pub separated: bool,
    /// The keys are on it: ringed.
    pub focused: bool,
    /// A main one pressed only by a click: no `↵` on it (what the user must choose to do:
    /// a push).
    pub keyless: bool,
}

/// The room around a footer's plain button's label, both sides together (logical pixels):
/// narrower than a main one's, which keeps the 96 of [`primary_width`].
const FOOTER_LABEL_ROOM: f32 = 30.0;

/// A modal footer's buttons, right to left from `right` (the first given the rightmost),
/// centered on `mid`: the main one with its key, the others bordered. Returns each one's
/// area (in the order given) and where the room left of them ends.
pub fn footer_buttons(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    right: f32,
    mid: f32,
    buttons: &[FooterButton<'_>],
) -> (Vec<Rect>, f32) {
    let px = |v: f32| ctx.px(v);
    let h = px(34.0);
    let font = button_font(ctx);
    let mut left = right;
    let mut areas = Vec::with_capacity(buttons.len());
    for b in buttons {
        // A group of its own: a short line between it and those on its right.
        if b.separated && !areas.is_empty() {
            let x = (left - px(6.0)).round();
            super::divider_vertical(
                layer,
                ctx,
                x,
                (mid - px(11.0)).round(),
                (mid + px(11.0)).round(),
            );
            left = x - px(14.0);
        }
        let rect = if b.danger && !b.main {
            let w = super::primary_width(text, ctx, &[b.label]);
            let rect = Rect::new(left - w, (mid - h / 2.0).round(), w, h);
            super::danger_button(layer, text, ctx, rect, b.label, b.enabled, b.hovered);
            rect
        } else if b.main && b.keyless {
            let w = super::primary_width(text, ctx, &[b.label]);
            let rect = Rect::new(left - w, (mid - h / 2.0).round(), w, h);
            if b.danger {
                super::danger_button(layer, text, ctx, rect, b.label, b.enabled, b.hovered);
            } else {
                super::primary_button(layer, text, ctx, rect, b.label, b.enabled, b.hovered);
            }
            rect
        } else if b.main {
            let spec = KeyButtonSpec {
                right: left,
                mid,
                label: b.label,
                key: "↵",
                enabled: b.enabled,
                hovered: b.hovered,
                danger: b.danger,
            };
            primary_button_with_key(layer, text, ctx, spec)
        } else {
            let w = (text.shape_line(b.label, &font).width + px(FOOTER_LABEL_ROOM)).round();
            let rect = Rect::new(left - w, (mid - h / 2.0).round(), w, h);
            super::secondary_button(layer, text, ctx, rect, b.label, b.enabled, b.hovered);
            rect
        };
        if b.focused {
            super::focus_ring(layer, ctx, rect, px(radius::MD));
        }
        areas.push(rect);
        left = rect.min_x() - px(if b.main { 10.0 } else { 8.0 });
    }
    (areas, left - px(4.0))
}

/// What a loading indicator shows: a spinner (dots turning), a text, or both; its colors
/// are the theme's unless given. While one is shown its view keeps asking for frames
/// (`Viewer::is_animating`), and stops when it's done.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loader<'a> {
    pub text: Option<&'a str>,
    pub spinner: bool,
    /// The spinner's diameter (logical pixels).
    pub size: f32,
    /// The spinner's color (default: the accent).
    pub color: Option<Rgba>,
    /// The text's color (default: muted).
    pub text_color: Option<Rgba>,
    /// How long it has turned (the caller's clock: a notification's age); `None`: the
    /// wall clock.
    pub elapsed: Option<std::time::Duration>,
}

impl Default for Loader<'_> {
    fn default() -> Self {
        Self { text: None, spinner: true, size: 16.0, color: None, text_color: None, elapsed: None }
    }
}

/// The spinner's dots, and one step of their turn.
const SPINNER_DOTS: usize = 8;
pub const SPINNER_STEP: std::time::Duration = std::time::Duration::from_millis(90);

impl<'a> Loader<'a> {
    /// A spinner and `text` beside it.
    pub fn with_text(text: &'a str) -> Self {
        Self { text: Some(text), ..Self::default() }
    }

    /// Turned by the caller's clock: `elapsed` since it started.
    pub fn elapsed(mut self, elapsed: std::time::Duration) -> Self {
        self.elapsed = Some(elapsed);
        self
    }

    /// The spinner's lit dot now (0..8).
    pub fn step(&self) -> usize {
        let elapsed = self.elapsed.unwrap_or_else(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
        });
        (elapsed.as_millis() / SPINNER_STEP.as_millis()) as usize % SPINNER_DOTS
    }

    /// Its width, to place it.
    pub fn width(&self, text: &mut TextSystem, ctx: &Ctx<'_>) -> f32 {
        let spinner = if self.spinner { ctx.px(self.size) } else { 0.0 };
        let gap = if self.spinner && self.text.is_some() { ctx.px(8.0) } else { 0.0 };
        let label = self.text.map_or(0.0, |t| text.shape_line(t, &ctx.ui_font(-0.5)).width);
        spinner + gap + label
    }
}

/// A loading indicator from `x`, vertically centered on `mid`. Returns its area.
pub fn loader(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    x: f32,
    mid: f32,
    loader: &Loader<'_>,
) -> Rect {
    let size = ctx.px(loader.size);
    // To a screen reader: something under way (a progress without a known end), named by
    // what it says; its text, painted under it, isn't read again. Its width is measured
    // only while a tree is made.
    let width = if ui_kit::access::recording() { loader.width(text, ctx) } else { size };
    ui_kit::access::group(
        || {
            use ui_kit::access::{Described, Role};
            let at = Rect::new(x, mid - size / 2.0, width, size);
            Described::new(Role::ProgressBar, loader.text.unwrap_or_default(), at)
        },
        || loader_parts(layer, text, ctx, (x, mid), loader),
    )
}

/// A [`loader`]'s spinner and text, painted: their area.
fn loader_parts(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    (x, mid): (f32, f32),
    loader: &Loader<'_>,
) -> Rect {
    let size = ctx.px(loader.size);
    let mut right = x;
    if loader.spinner {
        loader_spinner(layer, ctx, (x, mid), loader);
        right += size;
    }
    if let Some(label) = loader.text {
        if loader.spinner {
            right += ctx.px(8.0);
        }
        let font = ctx.ui_font(-0.5);
        let shaped = text.shape_line(label, &font);
        let color = loader.text_color.unwrap_or(ctx.palette.text_muted);
        draw_text(
            layer,
            &shaped,
            right.round(),
            optical_baseline(mid - size / 2.0, size, &font),
            ctx.fade(color),
        );
        right += shaped.width;
    }
    Rect::new(x, mid - size / 2.0, right - x, size)
}

/// Only a loader's spinner, from `x`, centered on `mid` (no text, no text system needed).
pub fn loader_spinner(layer: &mut Layer, ctx: &Ctx<'_>, (x, mid): (f32, f32), loader: &Loader<'_>) {
    // Eight dots on a circle, the brightest one turning (one step every 90 ms).
    const DOTS: usize = SPINNER_DOTS;
    let size = ctx.px(loader.size);
    let step = loader.step();
    let color = loader.color.unwrap_or(ctx.palette.accent);
    let (cx, cy) = (x + size / 2.0, mid);
    let radius = size * 0.38;
    let dot = (size * 0.2).max(2.0);
    for i in 0..DOTS {
        let angle = i as f32 / DOTS as f32 * std::f32::consts::TAU;
        let (dx, dy) = (angle.cos() * radius, angle.sin() * radius);
        let age = (step + DOTS - i) % DOTS;
        let alpha = 1.0 - age as f32 / DOTS as f32 * 0.85;
        let r = Rect::new(cx + dx - dot / 2.0, cy + dy - dot / 2.0, dot, dot);
        layer.quad(Quad::filled(r, ctx.fade(color.with_alpha(color.a * alpha))).rounded(dot / 2.0));
    }
}

/// The room between an empty state's title and its explanation (logical pixels).
const EMPTY_DETAIL_GAP: f32 = 6.0;

/// One line of an empty state's text block: its font, and its ascent and height (physical
/// pixels).
struct BlockLine {
    font: Font,
    ascent: f32,
    height: f32,
}

impl BlockLine {
    fn measure(text: &mut TextSystem, font: Font) -> Self {
        let m = text.metrics(&font);
        Self { ascent: m.ascent, height: m.ascent + m.descent, font }
    }
}

/// An empty state's text block, measured once: its title's line, its explanation's (when
/// it has one), and the room between them (physical pixels).
struct EmptyBlock {
    title: BlockLine,
    detail: Option<BlockLine>,
    gap: f32,
}

impl EmptyBlock {
    fn measure(text: &mut TextSystem, ctx: &Ctx<'_>, detail: bool) -> Self {
        Self {
            title: BlockLine::measure(text, ctx.ui_font(0.5)),
            detail: detail.then(|| BlockLine::measure(text, ctx.ui_font(-1.0))),
            gap: ctx.px(EMPTY_DETAIL_GAP),
        }
    }

    /// The height it takes: the title, and the explanation under it.
    fn height(&self) -> f32 {
        self.title.height + self.detail.as_ref().map_or(0.0, |d| d.height + self.gap)
    }

    /// Its texts drawn centered across `rect`, from `top`.
    fn draw(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        (rect, top): (Rect, f32),
        (title, detail): (&str, Option<&str>),
    ) {
        let shaped = text.shape_line(title, &self.title.font);
        draw_text(
            layer,
            &shaped,
            (rect.min_x() + (rect.size.width - shaped.width) / 2.0).round(),
            (top + self.title.ascent).round(),
            ctx.fade(ctx.palette.text_muted),
        );
        let (Some(detail), Some(line)) = (detail, &self.detail) else { return };
        let shaped = text.shape_line(detail, &line.font);
        let y = top + self.title.height + self.gap + line.ascent;
        draw_text(
            layer,
            &shaped,
            (rect.min_x() + (rect.size.width - shaped.width) / 2.0).round(),
            y.round(),
            ctx.fade(ctx.palette.text_muted.with_alpha(0.75)),
        );
    }
}

/// An empty state: a title (and optionally an explanation), centered in `rect`.
pub fn empty_state(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    title: &str,
    detail: Option<&str>,
) {
    let block = EmptyBlock::measure(text, ctx, detail.is_some());
    let top = rect.min_y() + (rect.size.height - block.height()) / 2.0;
    block.draw(layer, text, ctx, (rect, top), (title, detail));
}

/// The spinner of a [`loading_state`]: its diameter (logical pixels).
const LOADING_SPINNER: f32 = 22.0;
/// The room between a [`loading_state`]'s spinner and its title (logical pixels).
const LOADING_GAP: f32 = 14.0;

/// Something on its way (a page loading, an engine starting): a turning spinner above a
/// title (and optionally an explanation), centered in `rect`. While it's shown its view
/// says it's loading (`Viewer::is_loading`: frames at a loader's pace).
pub fn loading_state(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    title: &str,
    detail: Option<&str>,
) {
    let size = ctx.px(LOADING_SPINNER);
    let gap = ctx.px(LOADING_GAP);
    let block = EmptyBlock::measure(text, ctx, detail.is_some());
    // The title block moved down by half the spinner's room: the whole stays centered.
    let top = rect.min_y() + (size + gap + rect.size.height - block.height()) / 2.0;
    let spinner = Loader { size: LOADING_SPINNER, ..Loader::default() };
    let x = rect.min_x() + (rect.size.width - size) / 2.0;
    let mid = top - gap - size / 2.0;
    // To a screen reader: something under way, named by its title (as a loader is); the
    // title painted under it isn't read again, the explanation is.
    ui_kit::access::group(
        || ui_kit::access::Described::new(ui_kit::access::Role::ProgressBar, title, rect),
        || {
            loader_spinner(layer, ctx, (x, mid), &spinner);
            block.draw(layer, text, ctx, (rect, top), (title, detail));
        },
    );
}

/// The widest a tooltip's line gets (logical px) before it wraps between words.
pub const TOOLTIP_MAX_W: f32 = 440.0;

/// The most lines a tooltip shows (the last cut with "…").
const TOOLTIP_MAX_LINES: usize = 12;

/// A tooltip's lines: one per line of `label` (`\n` breaks one), each wrapped between words
/// when wider than `max`; none empty.
pub fn tooltip_lines(
    text: &mut TextSystem,
    font: &Font,
    label: &str,
    max: f32,
) -> Vec<std::sync::Arc<text_layout::ShapedLine>> {
    let mut out = Vec::new();
    for line in label.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
        if out.len() >= TOOLTIP_MAX_LINES {
            break;
        }
        let shaped = text.shape_line(line, font);
        if shaped.width <= max + 0.5 {
            out.push(shaped);
            continue;
        }
        let room = TOOLTIP_MAX_LINES - out.len();
        for part in wrap(text, line, font, max, room) {
            out.push(text.shape_line(&part, font));
        }
    }
    out
}

/// What a tooltip shows and where: the element it's about, its label and shortcut, the
/// area it stays in, and where the mouse is (its cursor never covers the arrow).
#[derive(Debug, Clone, Copy)]
pub struct TooltipSpec<'a> {
    pub anchor: Rect,
    pub label: &'a str,
    pub shortcut: Option<&'a str>,
    pub bounds: Rect,
    /// The mouse's point (physical), when the mouse is what shows it.
    pub pointer: Option<Point>,
}

/// Where a tooltip was drawn: its pill, and its arrow's box between the pill and the
/// element (`None`: no room for one) — what must show over a page of the web engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TooltipShape {
    pub pill: Rect,
    pub arrow: Option<Rect>,
}

/// The mouse's cursor as drawn by the system from its point (logical pixels): it reaches
/// this far right and down. A tooltip's arrow and pill keep clear of it.
const CURSOR_W: f32 = 14.0;
const CURSOR_H: f32 = 22.0;
/// The room kept between the cursor and a tooltip's arrow (logical pixels).
const CURSOR_CLEAR: f32 = 3.0;

/// A tooltip: the label (and optionally the shortcut, in a key) in a floating pill,
/// below the `anchor` element (above if it doesn't fit), with an arrow pointing at it; stays
/// within `bounds`. The mouse's cursor never hides the arrow: under it, the arrow moves to
/// the cursor's left (on the element still), and a pill below goes down past the cursor's
/// body (above the element when that wouldn't fit). A label of several lines (`\n`)
/// shows them one under the other, the pill as wide as the widest and as tall as all of
/// them; a line too wide wraps. Returns the pill's rect.
pub fn tooltip(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: &TooltipSpec<'_>,
) -> TooltipShape {
    let TooltipSpec { anchor, label, shortcut, bounds, pointer } = *spec;
    let font = ctx.ui_font(-1.0);
    let key_font = ctx.ui_font(-2.5);
    let key = shortcut.map(|k| text.shape_line(k, &key_font));
    let (pad, row_h, line_h) = (ctx.px(10.0), ctx.px(28.0), ctx.px(18.0));
    let key_w = key.as_ref().map_or(0.0, |k| k.width + ctx.px(10.0) + ctx.px(8.0));
    let room = (bounds.size.width - 2.0 * pad - key_w - ctx.px(8.0)).max(ctx.px(80.0));
    let lines = tooltip_lines(text, &font, label, ctx.px(TOOLTIP_MAX_W).min(room));
    let text_w = lines.iter().map(|l| l.width).fold(0.0_f32, f32::max);
    let h = row_h + line_h * lines.len().saturating_sub(1) as f32;
    let w = (text_w + key_w + 2.0 * pad).round();
    let gap = ctx.px(7.0);
    // The cursor over the element: a pill below starts past its body (the cursor hangs
    // down from its point).
    let cursor = pointer.filter(|p| {
        p.x >= anchor.min_x() - ctx.px(CURSOR_W)
            && p.x <= anchor.max_x()
            && p.y >= anchor.min_y()
            && p.y <= anchor.max_y()
    });
    let under = cursor.map_or(anchor.max_y() + gap, |p| {
        (anchor.max_y() + gap).max(p.y + ctx.px(CURSOR_H) + ctx.px(CURSOR_CLEAR))
    });
    let below = under + h <= bounds.max_y();
    let y = if below { under } else { anchor.min_y() - gap - h };
    let y = y.max(bounds.min_y());
    let x = (anchor.min_x() + anchor.size.width / 2.0 - w / 2.0).clamp(
        bounds.min_x() + ctx.px(4.0),
        (bounds.max_x() - w - ctx.px(4.0)).max(bounds.min_x()),
    );
    let rect = Rect::new(x.round(), y.round(), w, h);
    surface(layer, ctx, rect, Elevation::Low, radius::SM + 1.0);
    // The arrow, centered on the element: a triangle of one-pixel rows, with its edge in the
    // border color (continuing the card outline) and no card border under it, so the
    // arrow and the body form a single shape.
    let arrow = ctx.px(6.0);
    let mut ax = anchor.min_x() + anchor.size.width / 2.0;
    // Below, the cursor's body over the arrow: the arrow left of it, on the element still.
    if below
        && let Some(p) = cursor
        && ax + arrow >= p.x - ctx.px(CURSOR_CLEAR)
        && ax - arrow <= p.x + ctx.px(CURSOR_W)
    {
        let left_of_cursor = p.x - ctx.px(CURSOR_CLEAR) - arrow;
        // On the element still, unless it's only a sliver at the mouse (a wide item's).
        ax = if anchor.size.width > 2.0 * arrow {
            left_of_cursor.max(anchor.min_x() + arrow)
        } else {
            left_of_cursor
        };
    }
    let ax = ax.clamp(rect.min_x() + ctx.px(radius::MD), rect.max_x() - ctx.px(radius::MD)).round();
    let line = ctx.line();
    let fill = ctx.fade(ctx.palette.popup_background);
    let border = ctx.fade(ctx.palette.popup_border);
    let mut i = 0.0;
    while i < arrow {
        let half = arrow - i;
        let row_y = if below { rect.min_y() - i - line } else { rect.max_y() + i };
        layer.quad(Quad::filled(
            Rect::new(ax - half - line, row_y, 2.0 * (half + line), line),
            border,
        ));
        layer.quad(Quad::filled(
            Rect::new(ax - half + line * 0.5, row_y, 2.0 * half - line, line),
            fill,
        ));
        i += line;
    }
    // The opening in the card border, under the arrow's base.
    let base_y = if below { rect.min_y() } else { rect.max_y() - line };
    layer.quad(Quad::filled(
        Rect::new(ax - arrow + line, base_y, 2.0 * arrow - 2.0 * line, line),
        fill,
    ));
    let first = optical_baseline(rect.min_y(), row_h, &font);
    for (i, line) in lines.iter().enumerate() {
        let baseline = (first + line_h * i as f32).round();
        draw_text(layer, line, rect.min_x() + pad, baseline, ctx.fade(ctx.palette.text));
    }
    if let Some(key) = key {
        let kh = ctx.px(18.0);
        let kr = Rect::new(
            rect.max_x() - pad - key.width - ctx.px(10.0),
            rect.min_y() + (row_h - kh) / 2.0,
            key.width + ctx.px(10.0),
            kh,
        );
        let back = ctx.palette.text.with_alpha(KEY_BACK);
        let look = inline_key_look(ctx, back, ctx.palette.text_muted);
        let at = (kr.min_x() + ctx.px(5.0), optical_baseline(kr.min_y(), kh, &key_font));
        ui_kit::paint_keycap(layer, kr, &key, at, &look);
    }
    // The arrow's box: between the pill and the element, where it was drawn.
    let (top, bottom) =
        if below { (anchor.max_y(), rect.min_y()) } else { (rect.max_y(), anchor.min_y()) };
    let arrow_box = (bottom > top).then(|| Rect::new(ax - arrow, top, 2.0 * arrow, bottom - top));
    TooltipShape { pill: rect, arrow: arrow_box }
}
