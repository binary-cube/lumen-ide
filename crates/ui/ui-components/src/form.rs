//! The parts of a settings form, for panels holding many settings (the theme studio, a
//! module's configuration): a setting's row (its label, a one-line description, its control
//! on the right, "↺" while it differs from its default), rows grouped in a card, a slider
//! on one line with its value said in words on its right ("Sparse", "Slow", "40 %"), a
//! foldable card (a list entry with its handle, icon, title, summary, switch and menu, its
//! options under it once opened, in foldable sections), a gallery of cards with a thumbnail,
//! and the navigation between a form's categories (a list down the side, tabs when narrow).
//!
//! Stateless, as the design system's components: each draws into a layer what it's given
//! and returns where its parts went (for the clicks); sizes in logical pixels scaled by the
//! context, colors from the palette.

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

use crate::ds::{self, Ctx, Tone, ellipsize, optical_baseline, radius};
use crate::painter::LayerPainter;
use crate::slider::{SliderArea, SliderSpec, SliderStyle, slider};

// ---------------------------------------------------------------------------------------
// A setting's row.

/// A row's inner padding, left and right (logical pixels).
pub const ROW_PAD: f32 = 14.0;
/// A row with only its label (logical pixels).
pub const ROW_H: f32 = 44.0;
/// A row with its label and its description.
pub const ROW_H_DETAIL: f32 = 58.0;
/// The room kept on the right for "↺" (whether shown or not, so controls line up).
pub const RESET_W: f32 = 30.0;
/// Narrower than this beside the control, the label goes above it (stacked).
pub const LABEL_MIN: f32 = 150.0;

/// What [`setting_row`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct RowSpec<'a> {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub label: &'a str,
    /// One line under the label, muted ("How many lights the garland has").
    pub detail: Option<&'a str>,
    /// The control's width (physical pixels) and height (logical: its own).
    pub control_w: f32,
    pub control_h: f32,
    /// An icon before its label (a module's), a name of the app's set.
    pub icon: Option<&'a str>,
    /// It differs from its default: "↺" shown and a mark at its left edge.
    pub modified: bool,
    pub reset_hovered: bool,
    /// How much it's highlighted (0..1: a search result jumped to fades out).
    pub highlight: f32,
}

/// Where a row's parts went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowArea {
    pub rect: Rect,
    /// Where its control goes (its width as asked, vertically its own height centered).
    pub control: Rect,
    /// "↺", when shown.
    pub reset: Option<Rect>,
    /// The label went above the control (not enough room beside it).
    pub stacked: bool,
}

/// Whether a row `width` wide with a control `control_w` wide puts its label above it.
pub fn stacks(ctx: &Ctx<'_>, width: f32, control_w: f32) -> bool {
    width - control_w - ctx.px(2.0 * ROW_PAD + RESET_W + 16.0) < ctx.px(LABEL_MIN)
}

/// How much a row's description adds, per line after its first (logical pixels).
pub const DETAIL_LINE: f32 = 17.0;

/// The room beside (or above) the control for a row's label and description.
/// The room an icon before the label takes (its size and the gap after it).
const ICON_ROOM: f32 = 18.0 + 10.0;

/// Where a row's text starts: after its padding, and its icon when it has one.
fn text_x(ctx: &Ctx<'_>, spec: &RowSpec<'_>) -> f32 {
    spec.x + ctx.px(ROW_PAD) + if spec.icon.is_some() { ctx.px(ICON_ROOM) } else { 0.0 }
}

fn label_room(ctx: &Ctx<'_>, spec: &RowSpec<'_>) -> f32 {
    let inner_x = text_x(ctx, spec);
    let reset_x = spec.x + spec.width - ctx.px(ROW_PAD) - ctx.px(RESET_W);
    let room = if stacks(ctx, spec.width, spec.control_w) {
        reset_x - inner_x - ctx.px(8.0)
    } else {
        reset_x - spec.control_w - ctx.px(16.0) - inner_x
    };
    room.max(0.0)
}

/// A row's description in lines: one, or two when it doesn't fit on one (cut short after).
fn detail_lines(text: &mut TextSystem, ctx: &Ctx<'_>, spec: &RowSpec<'_>) -> Vec<String> {
    match spec.detail {
        Some(detail) => ds::wrap(text, detail, &ctx.ui_font(-2.0), label_room(ctx, spec), 2),
        None => Vec::new(),
    }
}

/// How tall a row's text is (physical pixels): its label, its description's lines.
fn text_height(ctx: &Ctx<'_>, lines: usize) -> f32 {
    match lines {
        0 => ctx.px(ROW_H),
        n => ctx.px(ROW_H_DETAIL + DETAIL_LINE * (n - 1) as f32),
    }
}

/// How tall a row is (physical pixels).
pub fn row_height(text: &mut TextSystem, ctx: &Ctx<'_>, spec: &RowSpec<'_>) -> f32 {
    let lines = detail_lines(text, ctx, spec).len();
    height_of(ctx, spec, text_height(ctx, lines))
}

/// A row's height for its text's.
fn height_of(ctx: &Ctx<'_>, spec: &RowSpec<'_>, text_h: f32) -> f32 {
    if stacks(ctx, spec.width, spec.control_w) {
        text_h + ctx.px(spec.control_h + 10.0)
    } else {
        text_h.max(ctx.px(spec.control_h + 16.0))
    }
}

/// A setting's row: its label (and description, on two lines when long) on the left, room
/// for its control on the right (below the label when narrow), "↺" after it while modified.
/// Returns where they are.
pub fn setting_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    icons: &icons::Icons,
    spec: RowSpec<'_>,
) -> RowArea {
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let details = detail_lines(text, ctx, &spec);
    // The text block: centered beside the control, at the top when stacked.
    let text_h = text_height(ctx, details.len());
    let h = height_of(ctx, &spec, text_h);
    let rect = Rect::new(spec.x, spec.y, spec.width, h);
    if spec.highlight > 0.0 {
        layer.quad(
            Quad::filled(rect, ctx.fade(p.accent.with_alpha(0.12 * spec.highlight.min(1.0))))
                .rounded(px(radius::SM)),
        );
    }
    let stacked = stacks(ctx, spec.width, spec.control_w);
    let inner_x = spec.x + px(ROW_PAD);
    let reset_x = rect.max_x() - px(ROW_PAD) - px(RESET_W);
    let label_room = label_room(ctx, &spec);
    let label_font = ctx.ui_font(-0.5);
    let detail_font = ctx.ui_font(-2.0);
    let (label_top, label_h) = if details.is_empty() {
        (spec.y + (if stacked { text_h } else { h } - px(20.0)) / 2.0, px(20.0))
    } else if stacked {
        (spec.y + px(10.0), px(20.0))
    } else {
        // Beside a taller control, the text block stays centered.
        (spec.y + px(10.0) + ((h - text_h) / 2.0).max(0.0), px(20.0))
    };
    // Its icon, centered on its label's line; the text after it.
    let text_left = text_x(ctx, &spec);
    if let Some(id) = spec.icon.and_then(|name| icons.id(name)) {
        let side = px(18.0);
        let at = Rect::new(inner_x, (label_top + (label_h - side) / 2.0).round(), side, side);
        ui_kit::draw_icon(layer, icons, id, at, ctx.fade(p.text_muted));
    }
    let label = ellipsize(text, spec.label, &label_font, label_room);
    let shaped = text.shape_line(&label, &label_font);
    draw_text(
        layer,
        &shaped,
        text_left,
        optical_baseline(label_top.round(), label_h, &label_font),
        ctx.fade(p.text),
    );
    for (i, line) in details.iter().enumerate() {
        let shaped = text.shape_line(line, &detail_font);
        let top = label_top + label_h + px(1.0) + px(DETAIL_LINE) * i as f32;
        draw_text(
            layer,
            &shaped,
            text_left,
            optical_baseline(top.round(), px(18.0), &detail_font),
            ctx.fade(p.text_muted),
        );
    }
    let control_h = px(spec.control_h);
    let control = if stacked {
        Rect::new(inner_x, (spec.y + text_h).round(), spec.control_w, control_h)
    } else {
        Rect::new(
            (reset_x - px(8.0) - spec.control_w).round(),
            (spec.y + (h - control_h) / 2.0).round(),
            spec.control_w,
            control_h,
        )
    };
    let reset = spec.modified.then(|| {
        let side = px(ds::button_size::COMPACT);
        // Over a control spread across the row, at the end of its label's line.
        let mid =
            if stacked { label_top + label_h / 2.0 } else { control.min_y() + control_h / 2.0 };
        let r = Rect::new(
            (reset_x + (px(RESET_W) - side) / 2.0).round(),
            (mid - side / 2.0).round(),
            side,
            side,
        );
        reset_button(layer, text, ctx, icons, r, spec.reset_hovered);
        r
    });
    if spec.modified {
        // The mark of a changed setting: a short accent bar at the row's left edge.
        let bar_h = px(16.0);
        let mid = if stacked { label_top + label_h / 2.0 } else { spec.y + h / 2.0 };
        layer.quad(
            Quad::filled(
                Rect::new(spec.x + px(4.0), (mid - bar_h / 2.0).round(), px(3.0), bar_h),
                ctx.fade(p.accent),
            )
            .rounded(px(1.5)),
        );
    }
    RowArea { rect, control, reset, stacked }
}

/// "↺": puts a setting back as it was (its default, the theme it started from).
pub fn reset_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    icons: &icons::Icons,
    r: Rect,
    hovered: bool,
) {
    let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
        .icon("rotate-ccw")
        .rest_tone(Tone::Neutral)
        .hovered(hovered);
    ds::button(layer, text, icons, ctx, r, spec);
}

/// A group of rows: the card behind them (inserted under them once their height is known).
pub fn group_quad(ctx: &Ctx<'_>, rect: Rect) -> Quad {
    let p = ctx.palette;
    Quad::filled(rect, ctx.fade(p.text.with_alpha(0.022)))
        .rounded(ctx.px(radius::LG))
        .bordered(ctx.line(), ctx.fade(p.border.with_alpha(0.8)))
}

/// The line between two rows of a group, inset from its edges.
pub fn row_divider(layer: &mut Layer, ctx: &Ctx<'_>, x0: f32, x1: f32, y: f32) {
    let inset = ctx.px(ROW_PAD);
    layer.quad(Quad::filled(
        Rect::new(x0 + inset, y.round(), (x1 - x0 - 2.0 * inset).max(0.0), ctx.line()),
        ctx.fade(ctx.palette.border.with_alpha(0.55)),
    ));
}

// ---------------------------------------------------------------------------------------
// A slider on one line, its value said on its right.

/// The room on a [`value_slider`]'s right for its value (logical pixels).
pub const VALUE_W: f32 = 84.0;
/// A value slider's height (logical pixels): its thumb's.
pub const VALUE_SLIDER_H: f32 = 22.0;

/// What [`value_slider`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct ValueSliderSpec<'a> {
    pub x: f32,
    /// Its vertical middle.
    pub mid: f32,
    pub width: f32,
    /// How many stops it has, and the one it's at.
    pub stops: usize,
    pub current: usize,
    /// What the current value means ("Dense", "Slow", "40 %").
    pub value: &'a str,
    pub hovered: bool,
}

/// A slider on one line over `stops` values, its value said after it: the track, a tick
/// per stop (when there are few), the thumb, then the value in words. Returns its area
/// (its stops, for clicks and drags; the value's room takes them too).
pub fn value_slider(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: ValueSliderSpec<'_>,
) -> SliderArea {
    let p = ctx.palette;
    let value_w = ctx.px(VALUE_W);
    let track_w = (spec.width - value_w - ctx.px(10.0)).max(ctx.px(60.0));
    let mut style = SliderStyle::from_palette(p);
    style.labels = false;
    style.ticks = spec.stops <= 8;
    style.thumb_size = 14.0;
    style.max_width = f32::MAX;
    let labels = [""; 32];
    let count = spec.stops.clamp(1, labels.len());
    let thumb = ctx.px(style.thumb_size);
    let top = spec.mid - thumb / 2.0 - ctx.px(2.0);
    let mut area = slider(
        layer,
        text,
        ctx,
        SliderSpec {
            x: spec.x,
            top,
            width: track_w,
            labels: &labels[..count],
            current: spec.current.min(count - 1),
            hovered: spec.hovered,
            style: &style,
        },
    );
    let font = ctx.ui_font(-1.0);
    let value = ellipsize(text, spec.value, &font, value_w);
    let shaped = text.shape_line(&value, &font);
    let vx = (spec.x + spec.width - shaped.width).round();
    draw_text(
        layer,
        &shaped,
        vx,
        optical_baseline((spec.mid - ctx.px(10.0)).round(), ctx.px(20.0), &font),
        ctx.fade(p.text),
    );
    area.rect = Rect::new(
        spec.x,
        (spec.mid - ctx.px(VALUE_SLIDER_H) / 2.0).round(),
        track_w,
        ctx.px(VALUE_SLIDER_H),
    );
    area
}

/// The stop after a key: one step (`delta` ±1) or a big one (Shift: a quarter of the
/// stops, at least 2), stopping at the ends.
pub fn stepped(current: usize, stops: usize, delta: i32, big: bool) -> usize {
    if stops == 0 {
        return 0;
    }
    let step = if big { (stops / 4).max(2) as i64 } else { 1 };
    let next = current as i64 + i64::from(delta.signum()) * step;
    next.clamp(0, stops as i64 - 1) as usize
}

// ---------------------------------------------------------------------------------------
// A foldable card (an entry of a list, its options under it once opened).

/// A foldable card's header (logical pixels).
pub const FOLD_HEAD: f32 = 56.0;
/// A foldable section's header inside a card (logical pixels).
pub const FOLD_SECTION: f32 = 32.0;
/// The rule after a fold section's title: the border at this alpha.
const FOLD_RULE: f32 = 0.6;
/// A gallery card's tag: the room on each side of its label (logical pixels).
const CARD_BADGE_PAD: f32 = 6.0;
/// A side nav's row: the chosen one's accent alpha, the hovered one's text alpha; its
/// count's fill alpha and the room on each side of it (logical pixels).
const NAV_SELECTED: f32 = 0.12;
const NAV_HOVER: f32 = 0.05;
const NAV_COUNT_FILL: f32 = 0.14;
const NAV_COUNT_PAD: f32 = 5.0;

/// The parts of a foldable card's header the mouse may be over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldPart {
    Header,
    Handle,
    Toggle,
    Menu,
}

/// What [`fold_card_head`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct FoldCardSpec<'a> {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    /// The entry's icon (shown in a small tile tinted with `tint`).
    pub icon: &'a str,
    pub tint: Rgba,
    pub title: &'a str,
    /// What it's set to, in a few words ("Background · dense · slow").
    pub summary: &'a str,
    pub expanded: bool,
    /// Its switch (on: drawn; off: kept but not drawn, muted).
    pub enabled: bool,
    pub hovered: Option<FoldPart>,
}

/// Where a foldable card's header parts went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldCardArea {
    /// The whole header (a click opens / closes it).
    pub header: Rect,
    /// The handle it's dragged by (its order).
    pub handle: Rect,
    pub toggle: Rect,
    /// "⋯": its menu.
    pub menu: Rect,
}

/// A foldable card's header: the handle, the icon in its tile, the title and the summary
/// under it, the switch, "⋯" and the chevron (down when opened). The card's background is
/// [`fold_card_quad`], under the header and the options.
pub fn fold_card_head(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    icons: &icons::Icons,
    spec: FoldCardSpec<'_>,
) -> FoldCardArea {
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let h = px(FOLD_HEAD);
    let header = Rect::new(spec.x, spec.y, spec.width, h);
    let mid = spec.y + h / 2.0;
    let muted = !spec.enabled;
    // The handle: two columns of three dots.
    let handle = Rect::new(spec.x + px(6.0), (mid - px(14.0)).round(), px(18.0), px(28.0));
    let dot = px(2.5).max(2.0);
    let handle_color =
        if spec.hovered == Some(FoldPart::Handle) { p.text } else { p.text_muted.with_alpha(0.55) };
    for col in 0..2 {
        for row in 0..3 {
            let cx = handle.min_x() + px(6.0) + col as f32 * px(5.5);
            let cy = mid - px(5.5) + row as f32 * px(5.5);
            layer.quad(
                Quad::filled(
                    Rect::new((cx - dot / 2.0).round(), (cy - dot / 2.0).round(), dot, dot),
                    ctx.fade(handle_color),
                )
                .rounded(dot / 2.0),
            );
        }
    }
    // The icon in its tile.
    let tile = px(32.0);
    let tile_r = Rect::new(handle.max_x() + px(6.0), (mid - tile / 2.0).round(), tile, tile);
    let tint = if muted { p.text_muted } else { spec.tint };
    layer.quad(
        Quad::filled(tile_r, ctx.fade(tint.with_alpha(0.14)))
            .rounded(px(radius::MD))
            .bordered(ctx.line(), ctx.fade(tint.with_alpha(0.28))),
    );
    if let Some(id) = icons.id(spec.icon) {
        let side = px(16.0);
        let at = Rect::new(
            (tile_r.min_x() + (tile - side) / 2.0).round(),
            (tile_r.min_y() + (tile - side) / 2.0).round(),
            side,
            side,
        );
        ui_kit::draw_icon(layer, icons, id, at, ctx.fade(tint));
    }
    // On the right: the chevron, "⋯", the switch.
    let chevron = px(16.0);
    let chevron_r = Rect::new(
        header.max_x() - px(14.0) - chevron,
        (mid - chevron / 2.0).round(),
        chevron,
        chevron,
    );
    let chevron_icon = if spec.expanded { "chevron-up" } else { "chevron-down" };
    if let Some(id) = icons.id(chevron_icon) {
        let color = if spec.hovered == Some(FoldPart::Header) { p.text } else { p.text_muted };
        ui_kit::draw_icon(layer, icons, id, chevron_r, ctx.fade(color));
    }
    let menu_side = px(ds::button_size::MEDIUM);
    let menu = Rect::new(
        chevron_r.min_x() - px(8.0) - menu_side,
        (mid - menu_side / 2.0).round(),
        menu_side,
        menu_side,
    );
    let more = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
        .icon("more-horizontal")
        .rest_tone(Tone::Neutral)
        .hovered(spec.hovered == Some(FoldPart::Menu));
    ds::button(layer, text, icons, ctx, menu, more);
    let switch_x = menu.min_x() - px(10.0) - px(36.0);
    let toggle =
        ds::switch(layer, ctx, switch_x, mid, spec.enabled, spec.hovered == Some(FoldPart::Toggle));
    // The title and the summary under it.
    let tx = tile_r.max_x() + px(12.0);
    let room = (toggle.min_x() - px(12.0) - tx).max(0.0);
    let title_font = ctx.ui_font(0.0).with_weight(Font::SEMIBOLD);
    let small = ctx.ui_font(-2.0);
    let title = ellipsize(text, spec.title, &title_font, room);
    let shaped = text.shape_line(&title, &title_font);
    let has_summary = !spec.summary.is_empty();
    let title_top = if has_summary { mid - px(19.0) } else { mid - px(10.0) };
    draw_text(
        layer,
        &shaped,
        tx,
        optical_baseline(title_top.round(), px(20.0), &title_font),
        ctx.fade(if muted { p.text_muted } else { p.text }),
    );
    if has_summary {
        let summary = ellipsize(text, spec.summary, &small, room);
        let shaped = text.shape_line(&summary, &small);
        draw_text(
            layer,
            &shaped,
            tx,
            optical_baseline((mid + px(1.0)).round(), px(18.0), &small),
            ctx.fade(p.text_muted),
        );
    }
    FoldCardArea { header, handle, toggle, menu }
}

/// A foldable card's background (lifted under the mouse, ringed in the accent while it's
/// dragged).
pub fn fold_card_quad(ctx: &Ctx<'_>, rect: Rect, hovered: bool, dragging: bool) -> Quad {
    let p = ctx.palette;
    let (fill, border) = if dragging {
        (p.popup_background, p.accent.with_alpha(0.8))
    } else if hovered {
        (p.text.with_alpha(0.04), p.border)
    } else {
        (p.text.with_alpha(0.022), p.border.with_alpha(0.8))
    };
    Quad::filled(rect, ctx.fade(fill))
        .rounded(ctx.px(radius::LG))
        .bordered(if dragging { ctx.px(1.5) } else { ctx.line() }, ctx.fade(border))
}

/// A foldable section's header inside a card ("▾ Look"): the chevron, the title, and a
/// line to the end. Returns its area (a click opens / closes it).
pub fn fold_section(
    pen: &mut LayerPainter<'_>,
    (x, y, width): (f32, f32, f32),
    title: &str,
    expanded: bool,
    hovered: bool,
) -> Rect {
    let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let h = px(FOLD_SECTION);
    let r = Rect::new(x, y, width, h);
    let mid = y + h / 2.0;
    let side = px(12.0);
    let color = if hovered { p.text } else { p.text_muted };
    let at = Rect::new(x, (mid - side / 2.0).round(), side, side);
    crate::tree::chevron(layer, icons, at, expanded, ctx.fade(color));
    let font = ctx.ui_font(-1.5).with_weight(Font::SEMIBOLD);
    let shaped = text.shape_line(title, &font);
    let tx = x + side + px(8.0);
    draw_text(layer, &shaped, tx, optical_baseline(y, h, &font), ctx.fade(color));
    let line_x = (tx + shaped.width + px(10.0)).round();
    if r.max_x() > line_x {
        let line = p.border.with_alpha(FOLD_RULE);
        crate::ds::divider_in(layer, ctx, (line_x, r.max_x(), mid.round()), line);
    }
    r
}

// ---------------------------------------------------------------------------------------
// A gallery of cards.

/// Cells in rows: as many columns as fit cells at least `min_cell` wide, sharing the
/// width; nothing allocated (a cell's place is worked out when asked).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub x: f32,
    pub y: f32,
    pub columns: usize,
    pub cell_w: f32,
    pub cell_h: f32,
    pub gap: f32,
}

impl Grid {
    pub fn new(x: f32, y: f32, width: f32, min_cell: f32, cell_h: f32, gap: f32) -> Self {
        let columns = (((width + gap) / (min_cell + gap)).floor() as usize).max(1);
        let cell_w = ((width - gap * (columns - 1) as f32) / columns as f32).floor();
        Self { x, y, columns, cell_w, cell_h, gap }
    }

    /// Cell `i`.
    pub fn cell(&self, i: usize) -> Rect {
        let (row, col) = (i / self.columns, i % self.columns);
        Rect::new(
            (self.x + col as f32 * (self.cell_w + self.gap)).round(),
            (self.y + row as f32 * (self.cell_h + self.gap)).round(),
            self.cell_w,
            self.cell_h,
        )
    }

    /// How tall `count` cells are.
    pub fn height(&self, count: usize) -> f32 {
        if count == 0 {
            return 0.0;
        }
        let rows = count.div_ceil(self.columns);
        rows as f32 * self.cell_h + (rows - 1) as f32 * self.gap
    }
}

/// What [`gallery_card`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct GalleryCardSpec<'a> {
    pub rect: Rect,
    /// The thumbnail's height (logical pixels; its width the card's, inset).
    pub thumb_h: f32,
    pub title: &'a str,
    /// One line under the title (none: the title alone).
    pub detail: Option<&'a str>,
    /// A small tag after the title ("light", "medium").
    pub badge: Option<(&'a str, Tone)>,
    pub selected: bool,
    pub hovered: bool,
}

/// A gallery's card: its frame, room for its thumbnail at the top (returned: the caller
/// draws it), the title (and the tag), the line under it. Selected: ringed in the accent.
pub fn gallery_card(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: GalleryCardSpec<'_>,
) -> Rect {
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let r = spec.rect;
    let (fill, border, width) = if spec.selected {
        (p.accent.with_alpha(0.07), p.accent, px(1.5))
    } else if spec.hovered {
        (p.text.with_alpha(0.045), p.text_muted.with_alpha(0.5), ctx.line())
    } else {
        (p.text.with_alpha(0.02), p.border, ctx.line())
    };
    layer.quad(
        Quad::filled(r, ctx.fade(fill)).rounded(px(radius::LG)).bordered(width, ctx.fade(border)),
    );
    let inset = px(6.0);
    let thumb = Rect::new(
        r.min_x() + inset,
        r.min_y() + inset,
        r.size.width - 2.0 * inset,
        px(spec.thumb_h),
    );
    let pad = px(10.0);
    let title_font = ctx.ui_font(-1.0).with_weight(Font::SEMIBOLD);
    let small = ctx.ui_font(-2.5);
    let mut room = r.size.width - 2.0 * pad;
    let badge = spec.badge.map(|(label, tone)| {
        let badge = crate::ds::Badge::new(label)
            .color(tone.color(p))
            .size(crate::ds::BadgeSize::Small)
            .pad(CARD_BADGE_PAD)
            .font(&small);
        room -= badge.width(text, ctx) + px(6.0);
        badge
    });
    let title = ellipsize(text, spec.title, &title_font, room.max(0.0));
    let shaped = text.shape_line(&title, &title_font);
    let ty = thumb.max_y() + px(8.0);
    draw_text(
        layer,
        &shaped,
        r.min_x() + pad,
        optical_baseline(ty, px(18.0), &title_font),
        ctx.fade(if spec.selected { p.accent } else { p.text }),
    );
    if let Some(badge) = badge {
        badge.paint_right(layer, text, ctx, r.max_x() - pad, ty + px(18.0) / 2.0);
    }
    if let Some(detail) = spec.detail {
        let font = ctx.ui_font(-2.5);
        let line = ellipsize(text, detail, &font, r.size.width - 2.0 * pad);
        let shaped = text.shape_line(&line, &font);
        draw_text(
            layer,
            &shaped,
            r.min_x() + pad,
            optical_baseline(ty + px(18.0), px(16.0), &font),
            ctx.fade(p.text_muted),
        );
    }
    thumb
}

// ---------------------------------------------------------------------------------------
// The navigation between a form's categories.

/// A category: its icon, its name, a number after it (how many effects…) and a dot (it
/// has unsaved changes).
#[derive(Debug, Clone, Copy)]
pub struct NavItem<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    pub count: Option<usize>,
    pub dot: bool,
}

/// The categories of a [`nav_list`] or [`nav_tabs`]: the chosen one and the one under the
/// mouse.
#[derive(Debug, Clone, Copy)]
pub struct Nav<'a> {
    pub items: &'a [NavItem<'a>],
    pub selected: usize,
    pub hovered: Option<usize>,
}

impl<'a> Nav<'a> {
    pub fn new(items: &'a [NavItem<'a>], selected: usize, hovered: Option<usize>) -> Self {
        Self { items, selected, hovered }
    }
}

/// A category's row in [`nav_list`] (logical pixels).
pub const NAV_ROW: f32 = 36.0;
/// The tabs' height in [`nav_tabs`] (logical pixels).
pub const NAV_TAB: f32 = 34.0;

/// Where a [`nav_list`] goes: its left edge, top and width (physical pixels) and its rows'
/// height (logical: [`NAV_ROW`], or less for a long list).
#[derive(Debug, Clone, Copy)]
pub struct NavPlace {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub row_h: f32,
}

impl NavPlace {
    /// At `x`, `y`, `width` wide, its rows [`NAV_ROW`] high.
    pub fn new(x: f32, y: f32, width: f32) -> Self {
        Self { x, y, width, row_h: NAV_ROW }
    }
}

/// The categories down the side: a row each (its icon, its name, its number, its dot),
/// the chosen one on a tinted pill with its icon in the accent. Hands each row's area to
/// `each`; returns the y below the last.
pub fn nav_list(
    pen: &mut LayerPainter<'_>,
    place: NavPlace,
    nav: Nav<'_>,
    mut each: impl FnMut(usize, Rect),
) -> f32 {
    let NavPlace { x, y, width, row_h } = place;
    let Nav { items, selected, hovered } = nav;
    let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let h = px(row_h);
    let font = ctx.ui_font(-0.5);
    let strong = font.clone().with_weight(Font::SEMIBOLD);
    let small = ctx.ui_font(-2.5).with_weight(Font::SEMIBOLD);
    let mut top = y;
    for (i, item) in items.iter().enumerate() {
        let r = Rect::new(x, top, width, h);
        let on = i == selected;
        crate::ds::RowHighlight::new(on, hovered == Some(i))
            .selected_alpha(NAV_SELECTED)
            .hover(crate::ds::HoverFill::Faint(NAV_HOVER))
            .paint(layer, ctx, r);
        let mid = top + h / 2.0;
        let side = px(16.0);
        let ix = x + px(10.0);
        if let Some(id) = icons.id(item.icon) {
            let color = if on { p.accent } else { p.text_muted };
            ui_kit::draw_icon(
                layer,
                icons,
                id,
                Rect::new(ix, (mid - side / 2.0).round(), side, side),
                ctx.fade(color),
            );
        }
        let tx = ix + side + px(10.0);
        // On the right: the dot, then the number.
        let mut right = r.max_x() - px(10.0);
        if let Some(n) = item.count.filter(|n| *n > 0) {
            let label = n.to_string();
            let br = crate::ds::Badge::new(&label)
                .color(if on { p.accent } else { p.text_muted })
                .fill_alpha(NAV_COUNT_FILL)
                .size(crate::ds::BadgeSize::Medium)
                .pad(NAV_COUNT_PAD)
                .min_width(crate::ds::BadgeSize::Medium.height())
                .centered()
                .font(&small)
                .paint_right(layer, text, ctx, right, mid);
            right = br.min_x() - px(6.0);
        }
        if item.dot {
            let d = px(7.0);
            let dr = Rect::new(right - d, (mid - d / 2.0).round(), d, d);
            layer.quad(Quad::filled(dr, ctx.fade(p.accent)).rounded(d / 2.0));
            right = dr.min_x() - px(6.0);
        }
        let f = if on { &strong } else { &font };
        let label = ellipsize(text, item.label, f, (right - tx).max(0.0));
        let shaped = text.shape_line(&label, f);
        draw_text(
            layer,
            &shaped,
            tx,
            optical_baseline(top, h, f),
            ctx.fade(if on { p.text } else { p.text_muted.mix(p.text, 0.45) }),
        );
        each(i, r);
        top += h + px(2.0);
    }
    top
}

/// A group's caption over its categories in a [`nav_list`] ("EDITING"): small capitals,
/// muted, then a line on the same row from right after the name to the right edge, fading
/// out ([`crate::ds::divider_faded_right`]). The group holding the open page (`active`) has
/// its name and the start of its line in the accent, subtly. Returns the y below it.
pub fn nav_caption(
    pen: &mut LayerPainter<'_>,
    (x, y, width): (f32, f32, f32),
    caption: &str,
    active: bool,
) -> f32 {
    let (layer, text, ctx) = (&mut *pen.layer, &mut *pen.text, &pen.ds);
    let px = |v: f32| ctx.px(v);
    let p = ctx.palette;
    let font = ctx.ui_font(-3.0).with_weight(Font::BOLD);
    let caption = caption.to_uppercase();
    let label = ellipsize(text, &caption, &font, (width - px(20.0)).max(0.0));
    let shaped = text.shape_line(&label, &font);
    let h = px(NAV_CAPTION);
    let (text_color, line_color) = if active {
        (p.accent.with_alpha(0.9), p.accent.with_alpha(0.55))
    } else {
        (p.text_muted.with_alpha(0.9), p.border)
    };
    let tx = x + px(10.0);
    let baseline = optical_baseline(y + px(8.0), h - px(8.0), &font);
    draw_text(layer, &shaped, tx, baseline, ctx.fade(text_color));
    // The line through the middle of the capitals, after a small gap.
    let line_x = (tx + shaped.width + px(8.0)).round();
    let mid = (baseline - font.size * 0.36).round();
    crate::ds::divider_faded_right(layer, ctx, line_x, x + width, mid, line_color);
    y + h
}

/// A group caption's height in a [`nav_list`] (logical pixels).
pub const NAV_CAPTION: f32 = 24.0;

/// How wide a tab of [`nav_tabs`] is for `item`.
fn tab_width(text: &mut TextSystem, ctx: &Ctx<'_>, item: &NavItem<'_>, with_label: bool) -> f32 {
    let font = ctx.ui_font(-1.0).with_weight(Font::SEMIBOLD);
    let label =
        if with_label { text.shape_line(item.label, &font).width + ctx.px(8.0) } else { 0.0 };
    let extra = if item.dot { ctx.px(11.0) } else { 0.0 };
    (ctx.px(16.0) + label + extra + ctx.px(20.0)).round()
}

/// The categories as a row of tabs (a narrow form): each its icon and name (only the chosen
/// one named when they don't all fit), the chosen one underlined in the accent. Hands each
/// tab's area to `each`; returns the y below them.
pub fn nav_tabs(
    pen: &mut LayerPainter<'_>,
    (x, y, width): (f32, f32, f32),
    nav: Nav<'_>,
    mut each: impl FnMut(usize, Rect),
) -> f32 {
    let Nav { items, selected, hovered } = nav;
    let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let h = px(NAV_TAB);
    let full: f32 = items.iter().map(|i| tab_width(text, ctx, i, true)).sum();
    let named = |i: usize| full <= width || i == selected;
    let font = ctx.ui_font(-1.0).with_weight(Font::SEMIBOLD);
    let mut tx = x;
    for (i, item) in items.iter().enumerate() {
        let w = tab_width(text, ctx, item, named(i));
        let r = Rect::new(tx, y, w, h);
        let on = i == selected;
        crate::ds::RowHighlight::new(false, hovered == Some(i) && !on)
            .radius(radius::SM)
            .hover(crate::ds::HoverFill::Faint(NAV_HOVER))
            .paint(layer, ctx, r);
        let color = if on { p.text } else { p.text_muted };
        let mid = y + h / 2.0;
        let side = px(16.0);
        let ix = tx + px(10.0);
        if let Some(id) = icons.id(item.icon) {
            ui_kit::draw_icon(
                layer,
                icons,
                id,
                Rect::new(ix, (mid - side / 2.0).round(), side, side),
                ctx.fade(if on { p.accent } else { color }),
            );
        }
        let mut after = ix + side;
        if named(i) {
            let shaped = text.shape_line(item.label, &font);
            draw_text(
                layer,
                &shaped,
                after + px(8.0),
                optical_baseline(y, h, &font),
                ctx.fade(color),
            );
            after += px(8.0) + shaped.width;
        }
        if item.dot {
            let d = px(6.0);
            layer.quad(
                Quad::filled(
                    Rect::new(after + px(5.0), (mid - px(8.0)).round(), d, d),
                    ctx.fade(p.accent),
                )
                .rounded(d / 2.0),
            );
        }
        if on {
            layer.quad(
                Quad::filled(
                    Rect::new(tx + px(6.0), y + h - px(2.0), w - px(12.0), px(2.0)),
                    ctx.fade(p.accent),
                )
                .rounded(px(1.0)),
            );
        }
        each(i, r);
        tx += w;
    }
    crate::ds::divider(layer, ctx, x, x + width, y + h);
    y + h + ctx.line()
}

#[cfg(test)]
#[path = "../tests/unit/form.rs"]
mod tests;
