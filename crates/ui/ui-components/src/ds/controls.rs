//! Buttons: primary (busy, with a key), danger, secondary, a toolbar's; their font, their
//! width, their press.

use super::*;

/// The height of a view's buttons (and of the field a button stands next to): the wizard's.
pub const BUTTON_H: f32 = 34.0;

/// The weight of every button's label (the app's dialogs', its views', its modals').
pub const BUTTON_WEIGHT: u16 = 500;

/// The font of every button's label: the app's dialogs' (one definition, so every button
/// reads the same).
pub fn button_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(-0.5).with_weight(BUTTON_WEIGHT)
}

/// How wide a button is: as its widest label (`labels`: what it may show, so it doesn't
/// jump when the label changes), with room around it; never narrower than 96.
pub fn primary_width(text: &mut TextSystem, ctx: &Ctx<'_>, labels: &[&str]) -> f32 {
    let font = button_font(ctx);
    let widest = labels.iter().map(|l| text.shape_line(l, &font).width).fold(0.0, f32::max);
    (widest + ctx.px(40.0)).max(ctx.px(96.0)).round()
}

/// How something pressable shows now: whether it can be pressed, whether the mouse is on it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Press {
    pub enabled: bool,
    pub hovered: bool,
}

impl Press {
    pub fn new(enabled: bool, hovered: bool) -> Self {
        Self { enabled, hovered }
    }
}

/// The main button of a view (Commit, Refactor): accent filled, its label centered;
/// muted when disabled.
pub fn primary_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    label: &str,
    enabled: bool,
    hovered: bool,
) {
    filled_button(layer, text, ctx, rect, (label, ctx.palette.accent), enabled, hovered);
}

/// [`primary_button`] on its way to running its action, or back: `busy` (0..=1, eased by its
/// caller) is how far — at 1 a spinner before its label (both centered), the button
/// [`BUSY_EXTRA`] wider than its label alone needs and never lit by the mouse; between, the
/// spinner grows and fades in as the room for it opens, and the mouse's light fades out.
pub fn busy_primary_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    label: &str,
    busy: f32,
    hovered: bool,
) {
    let busy = busy.clamp(0.0, 1.0);
    let base = ctx.palette.accent;
    let fill = if hovered {
        base.mix(Rgba::WHITE, super::button::FILLED_HOVER_MIX * (1.0 - busy))
    } else {
        base
    };
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(ctx.px(radius::MD)));
    busy_label(layer, text, ctx, rect, (label, Rgba::WHITE), busy);
}

/// A busy button's spinner and label in `color`, centered together in `rect`: `busy`
/// (0..=1) is how far the spinner has grown and faded in, and how much room it has opened
/// before the label (see [`busy_primary_button`]).
pub(super) fn busy_label(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    (label, color): (&str, Rgba),
    busy: f32,
) {
    let busy = busy.clamp(0.0, 1.0);
    let font = button_font(ctx);
    let shaped = text.shape_line(label, &font);
    let room = ctx.px(BUSY_EXTRA) * busy;
    let x = (rect.min_x() + (rect.size.width - room - shaped.width) / 2.0).round();
    // Below that it would be a speck: the label alone.
    if busy > super::button::SPINNER_FROM {
        let spinner = Loader {
            size: BUSY_SPINNER * busy,
            color: Some(color.with_alpha(color.a * busy)),
            ..Loader::default()
        };
        loader(layer, text, ctx, x, rect.center().y, &spinner);
    }
    draw_text(
        layer,
        &shaped,
        (x + room).round(),
        optical_baseline(rect.min_y(), rect.size.height, &font),
        ctx.fade(color),
    );
}

/// How long a button takes to make room for its spinner, or to give it back.
pub const BUSY_TRANSITION: std::time::Duration = std::time::Duration::from_millis(180);

/// A busy button's spinner, and the gap between it and the label (logical pixels).
const BUSY_SPINNER: f32 = 12.0;
const BUSY_GAP: f32 = 6.0;

/// How much wider a busy button is than its label alone needs (its spinner and the gap).
pub const BUSY_EXTRA: f32 = BUSY_SPINNER + BUSY_GAP;

/// A button whose action only takes away (Drop, Discard): [`primary_button`] in the danger
/// color.
pub fn danger_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    label: &str,
    enabled: bool,
    hovered: bool,
) {
    filled_button(layer, text, ctx, rect, (label, ctx.palette.danger), enabled, hovered);
}

/// A button to a screen reader while the guard lives (its paint): named by its label, greyed
/// out while it's off, its key said with it (its texts drawn in it are its own).
fn told_button(
    label: &str,
    rect: Rect,
    enabled: bool,
    key: Option<&str>,
) -> Option<ui_kit::access::Opened> {
    // Drawn inside a button told already (the shared `ds::button` paints through these).
    if ui_kit::access::inside(ui_kit::access::Role::Button) {
        return None;
    }
    ui_kit::access::open(|| {
        use ui_kit::access::{Described, Role};
        let node = Described::new(Role::Button, label, rect).disabled(!enabled);
        match key {
            Some(key) => node.shortcut(key),
            None => node,
        }
    })
}

/// A button filled with `base`, its label centered.
fn filled_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    (label, base): (&str, Rgba),
    enabled: bool,
    hovered: bool,
) {
    let _told = told_button(label, rect, enabled, None);
    let p = ctx.palette;
    let fill = if !enabled {
        p.text.with_alpha(0.08)
    } else if hovered {
        base.mix(Rgba::WHITE, super::button::FILLED_HOVER_MIX)
    } else {
        base
    };
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(ctx.px(radius::MD)));
    centered_label(layer, text, ctx, rect, label, if enabled { Rgba::WHITE } else { p.text_muted });
}

/// The height of a view header's buttons (the Git window's Commit / Refresh / Fetch / Pull /
/// Push): smaller than a dialog's [`BUTTON_H`].
pub const TOOLBAR_BUTTON_H: f32 = 30.0;
/// Its icon's side, where the icon starts, and the room between it and the label (logical
/// pixels).
const TOOLBAR_ICON: f32 = 14.0;
const TOOLBAR_ICON_X: f32 = 11.0;
const TOOLBAR_ICON_GAP: f32 = 8.0;

/// The font of a view header's buttons.
pub fn toolbar_button_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(-0.5)
}

/// What a view header's button shows.
#[derive(Debug, Clone, Copy)]
pub struct ToolbarButton<'a> {
    pub label: &'a str,
    pub icon: &'a str,
    /// The header's main action, or one asking for attention (Push with commits to send):
    /// tinted in the accent.
    pub accent: bool,
    pub hovered: bool,
}

/// How wide [`toolbar_button`] is for `label` (its icon, the label, room around them).
pub fn toolbar_button_width(text: &mut TextSystem, ctx: &Ctx<'_>, label: &str) -> f32 {
    let font = toolbar_button_font(ctx);
    (text.shape_line(label, &font).width + ctx.px(TOOLBAR_ICON) + ctx.px(30.0)).round()
}

/// A button in a view's header row (beside its title, as the Git window's): bordered,
/// compact ([`TOOLBAR_BUTTON_H`] high), its icon then its label; the accent one tinted, not
/// filled (a dialog's main button is [`primary_button`]). `rect` from
/// [`toolbar_button_width`].
pub fn toolbar_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    button: ToolbarButton<'_>,
) {
    paint_toolbar_button(layer, text, icons, ctx, rect, button, false);
}

/// [`toolbar_button`] while its action runs: a spinner in its icon's place, never lit by
/// the mouse (its view asks for frames while it turns: `Viewer::is_loading`).
pub fn busy_toolbar_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    button: ToolbarButton<'_>,
) {
    let button = ToolbarButton { hovered: false, ..button };
    paint_toolbar_button(layer, text, icons, ctx, rect, button, true);
}

fn paint_toolbar_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    button: ToolbarButton<'_>,
    busy: bool,
) {
    let _told = told_button(button.label, rect, true, None);
    let p = ctx.palette;
    let (fill, border, color) = match (button.accent, button.hovered) {
        (true, false) => (p.accent.with_alpha(0.16), p.accent.with_alpha(0.5), p.accent),
        (true, true) => (p.accent.with_alpha(0.26), p.accent, p.accent),
        (false, true) => (p.text.with_alpha(0.09), p.card_border, p.text),
        (false, false) => (Rgba::TRANSPARENT, p.card_border, p.text.with_alpha(0.86)),
    };
    layer.quad(
        Quad::filled(rect, ctx.fade(fill))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(border)),
    );
    let icon_size = ctx.px(TOOLBAR_ICON);
    // An icon alone (its label is its tooltip: a row too narrow) is centered.
    let icon_x = if button.label.is_empty() {
        (rect.min_x() + (rect.size.width - icon_size) / 2.0).round()
    } else {
        rect.min_x() + ctx.px(TOOLBAR_ICON_X)
    };
    if busy {
        let spinner = Loader { size: TOOLBAR_ICON, color: Some(color), ..Loader::default() };
        loader(layer, text, ctx, icon_x, rect.center().y, &spinner);
    } else if let Some(id) = icons.id(button.icon) {
        let ir = Rect::new(
            icon_x,
            (rect.min_y() + (rect.size.height - icon_size) / 2.0).round(),
            icon_size,
            icon_size,
        );
        ui_kit::draw_icon(layer, icons, id, ir, ctx.fade(color));
    }
    if button.label.is_empty() {
        return;
    }
    let font = toolbar_button_font(ctx);
    let shaped = text.shape_line(button.label, &font);
    draw_text(
        layer,
        &shaped,
        (rect.min_x() + ctx.px(TOOLBAR_ICON_X + TOOLBAR_ICON_GAP) + icon_size).round(),
        optical_baseline(rect.min_y(), rect.size.height, &font),
        ctx.fade(color),
    );
}

/// A button beside the main one (Cancel, Apply, Test Connection, Browse…): bordered,
/// transparent, tinted under the mouse, as rounded as [`primary_button`]; its label muted
/// when disabled. Sized by [`primary_width`].
pub fn secondary_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    label: &str,
    enabled: bool,
    hovered: bool,
) {
    let _told = told_button(label, rect, enabled, None);
    let p = ctx.palette;
    let fill = if enabled && hovered { p.text.with_alpha(0.08) } else { Rgba::TRANSPARENT };
    layer.quad(
        Quad::filled(rect, ctx.fade(fill))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(p.border)),
    );
    centered_label(layer, text, ctx, rect, label, if enabled { p.text } else { p.text_muted });
}

/// A button's label, centered in `rect`.
pub(super) fn centered_label(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    label: &str,
    color: Rgba,
) {
    let font = button_font(ctx);
    let shaped = text.shape_line(label, &font);
    draw_text(
        layer,
        &shaped,
        (rect.min_x() + (rect.size.width - shaped.width) / 2.0).round(),
        optical_baseline(rect.min_y(), rect.size.height, &font),
        ctx.fade(color),
    );
}

/// What [`primary_button_with_key`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct KeyButtonSpec<'a> {
    pub right: f32,
    pub mid: f32,
    pub label: &'a str,
    pub key: &'a str,
    pub enabled: bool,
    pub hovered: bool,
    /// What can't be undone: in the danger color instead of the accent.
    pub danger: bool,
}

/// [`primary_button`] with its key after the label (`↵`), in a small tile: the button and
/// the key that presses it, read together. Its width fits the label; returns its area,
/// ending at `right`.
pub fn primary_button_with_key(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: KeyButtonSpec<'_>,
) -> Rect {
    let KeyButtonSpec { right, mid, label, key, enabled, hovered, danger } = spec;
    let p = ctx.palette;
    let base = if danger { p.danger } else { p.accent };
    let font = button_font(ctx);
    let key_font = ctx.ui_font(-2.5);
    let shaped = text.shape_line(label, &font);
    let key_shaped = text.shape_line(key, &key_font);
    let key_w = (key_shaped.width + ctx.px(8.0)).max(ctx.px(18.0));
    let h = ctx.px(BUTTON_H);
    let w = (shaped.width + key_w + ctx.px(38.0)).round();
    let rect = Rect::new((right - w).round(), (mid - h / 2.0).round(), w, h);
    let _told = told_button(label, rect, enabled, Some(key));
    let fill = if !enabled {
        p.text.with_alpha(0.08)
    } else if hovered {
        base.mix(Rgba::WHITE, super::button::FILLED_HOVER_MIX)
    } else {
        base
    };
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(ctx.px(radius::MD)));
    let color = if enabled { Rgba::WHITE } else { p.text_muted };
    let x = rect.min_x() + ctx.px(15.0);
    draw_text(layer, &shaped, x, optical_baseline(rect.min_y(), h, &font), ctx.fade(color));
    let kh = ctx.px(18.0);
    let tile =
        Rect::new((x + shaped.width + ctx.px(8.0)).round(), (mid - kh / 2.0).round(), key_w, kh);
    layer.quad(Quad::filled(tile, ctx.fade(color.with_alpha(0.2))).rounded(ctx.px(5.0)));
    draw_text(
        layer,
        &key_shaped,
        (tile.min_x() + (key_w - key_shaped.width) / 2.0).round(),
        optical_baseline(tile.min_y(), kh, &key_font),
        ctx.fade(color),
    );
    rect
}
