//! Controls: switch, checkbox, buttons (primary, secondary, danger, with a key), choices, segmented, stepper, radio
//! cards, the search field and its clear button.

use super::*;

/// A toggle (on / off), vertically centered on `mid`. Returns its area.
pub fn switch(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, mid: f32, on: bool, hovered: bool) -> Rect {
    switch_sized(layer, ctx, (x, mid), (ctx.px(36.0), ctx.px(20.0)), on, hovered, true)
}

/// A toggle that may be off limits: `enabled` false, it's dimmed and ignores the mouse.
pub fn switch_enabled(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    (x, mid): (f32, f32),
    on: bool,
    hovered: bool,
    enabled: bool,
) -> Rect {
    let sized = (ctx.px(36.0), ctx.px(20.0));
    switch_sized(layer, ctx, (x, mid), sized, on, hovered && enabled, enabled)
}

/// The width of a [`small_switch`].
pub const SMALL_SWITCH_W: f32 = 26.0;

/// What a switch off limits keeps of its colors.
pub const DISABLED_ALPHA: f32 = 0.45;

/// The switch made small, for a menu's row: only says how a setting stands.
pub fn small_switch(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, mid: f32, on: bool) -> Rect {
    switch_sized(layer, ctx, (x, mid), (ctx.px(SMALL_SWITCH_W), ctx.px(15.0)), on, false, true)
}

fn switch_sized(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    (x, mid): (f32, f32),
    (w, h): (f32, f32),
    on: bool,
    hovered: bool,
    enabled: bool,
) -> Rect {
    let dim =
        |c: Rgba| if enabled { ctx.fade(c) } else { ctx.fade(c.with_alpha(c.a * DISABLED_ALPHA)) };
    let rect = Rect::new(x, (mid - h / 2.0).round(), w, h);
    ui_kit::access::node(|| {
        use ui_kit::access::{Described, Role};
        Described::new(Role::Switch, "", rect).checked(on).disabled(!enabled)
    });
    let track = if on {
        ctx.palette.accent
    } else {
        ctx.palette.text.with_alpha(if hovered { 0.22 } else { 0.16 })
    };
    layer.quad(Quad::filled(rect, dim(track)).rounded(h / 2.0));
    let knob = h - ctx.px(4.0);
    let kx = if on { rect.max_x() - ctx.px(2.0) - knob } else { rect.min_x() + ctx.px(2.0) };
    let kr = Rect::new(kx, rect.min_y() + ctx.px(2.0), knob, knob);
    layer.quad(Quad::filled(kr, dim(Rgba::WHITE)).rounded(knob / 2.0));
    rect
}

/// A checkbox's state; `Mixed` for a group partly chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Off,
    On,
    Mixed,
}

impl Check {
    /// `On` if all are, `Off` if none, `Mixed` otherwise.
    pub fn of(chosen: usize, total: usize) -> Self {
        match chosen {
            0 => Self::Off,
            n if n >= total => Self::On,
            _ => Self::Mixed,
        }
    }
}

/// A checkbox, vertically centered on `mid`. Returns its area.
pub fn checkbox(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    x: f32,
    mid: f32,
    state: Check,
    enabled: bool,
) -> Rect {
    Checkbox::new(state).enabled(enabled).paint(layer, text, ctx, x, mid)
}

/// A checkbox's side by default (logical pixels) and its ✓'s font at that side (relative to
/// the UI's).
pub const CHECKBOX: f32 = 16.0;
const CHECKBOX_TICK_FONT: f32 = -2.0;
/// An empty box's outline at rest and under the mouse (alpha of the muted text).
const CHECKBOX_BORDER: f32 = 0.6;
const CHECKBOX_BORDER_HOVER: f32 = 0.95;

/// A checkbox, as configured: its state, enabled, its side, lit under the mouse. Its
/// defaults are [`checkbox`]'s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Checkbox {
    pub state: Check,
    pub enabled: bool,
    /// Its side (logical pixels).
    pub size: f32,
    /// Under the mouse: an empty box's outline brighter.
    pub hovered: bool,
}

impl Checkbox {
    pub fn new(state: Check) -> Self {
        Self { state, enabled: true, size: CHECKBOX, hovered: false }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Its side (logical pixels).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Drawn from `x`, centered on `mid`. Returns its area.
    pub fn paint(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        x: f32,
        mid: f32,
    ) -> Rect {
        let size = ctx.px(self.size);
        // Its corners and marks scale with it; at the usual side, the usual ones.
        let k = self.size / CHECKBOX;
        let px = |v: f32| ctx.px(v * k);
        let r = Rect::new(x.round(), (mid - size / 2.0).round(), size, size);
        ui_kit::access::node(|| {
            use ui_kit::access::{Described, Role};
            let mut node = Described::new(Role::CheckBox, "", r)
                .checked(self.state == Check::On)
                .disabled(!self.enabled);
            node.state.mixed = self.state == Check::Mixed;
            node
        });
        if self.state == Check::Off {
            let border =
                if self.hovered && self.enabled { CHECKBOX_BORDER_HOVER } else { CHECKBOX_BORDER };
            layer.quad(Quad::filled(r, Rgba::TRANSPARENT).rounded(px(4.0)).bordered(
                ctx.line().max(1.0),
                ctx.fade(ctx.palette.text_muted.with_alpha(border)),
            ));
            return r;
        }
        let fill = if self.enabled { ctx.palette.accent } else { ctx.palette.text_muted };
        layer.quad(Quad::filled(r, ctx.fade(fill)).rounded(px(4.0)));
        if self.state == Check::Mixed {
            let bar =
                Rect::new(r.min_x() + px(4.0), (mid - px(1.0)).round(), size - px(8.0), px(2.0));
            layer.quad(Quad::filled(bar, ctx.fade(Rgba::WHITE)).rounded(px(1.0)));
            return r;
        }
        let font = ctx.ui_font(CHECKBOX_TICK_FONT);
        let font = if k == 1.0 { font } else { Font::new(font.family, (font.size * k).round()) };
        let font = font.with_weight(Font::BOLD);
        let tick = text.shape_line("✓", &font);
        draw_text(
            layer,
            &tick,
            (r.min_x() + (size - tick.width) / 2.0).round(),
            optical_baseline(r.min_y(), size, &font),
            ctx.fade(Rgba::WHITE),
        );
        r
    }
}

/// A radio's side by default and its dot's (logical pixels).
pub const RADIO: f32 = 18.0;
const RADIO_DOT: f32 = 7.0;
/// Its outline's width when not chosen (logical pixels).
const RADIO_BORDER: f32 = 1.5;
/// Its outline under the mouse (alpha of the muted text).
const RADIO_BORDER_HOVER: f32 = 0.6;

/// A radio, as configured: chosen or not, under the mouse, its side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radio {
    pub selected: bool,
    pub hovered: bool,
    /// Its side (logical pixels).
    pub size: f32,
}

impl Radio {
    pub fn new(selected: bool) -> Self {
        Self { selected, hovered: false, size: RADIO }
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Drawn with its top left at `at` (physical pixels): chosen, an accent disc with a white
    /// dot; else a ring (brighter under the mouse). Returns its area.
    pub fn paint(&self, layer: &mut Layer, ctx: &Ctx<'_>, at: Point) -> Rect {
        let p = ctx.palette;
        let d = ctx.px(self.size);
        let radio = Rect::new(at.x, at.y, d, d);
        ui_kit::access::node(|| {
            use ui_kit::access::{Described, Role};
            Described::new(Role::Radio, "", radio).checked(self.selected)
        });
        if self.selected {
            layer.quad(Quad::filled(radio, ctx.fade(p.accent)).rounded(d / 2.0));
            let dot = ctx.px(RADIO_DOT * self.size / RADIO);
            let inner = Rect::new(
                radio.min_x() + (d - dot) / 2.0,
                radio.min_y() + (d - dot) / 2.0,
                dot,
                dot,
            );
            layer.quad(Quad::filled(inner, ctx.fade(Rgba::WHITE)).rounded(dot / 2.0));
        } else {
            let border =
                if self.hovered { p.text_muted.with_alpha(RADIO_BORDER_HOVER) } else { p.border };
            layer.quad(
                Quad::filled(radio, Rgba::TRANSPARENT)
                    .rounded(d / 2.0)
                    .bordered(ctx.px(RADIO_BORDER), ctx.fade(border)),
            );
        }
        radio
    }
}

/// A bare radio from `x`, centered on `mid`. Returns its area.
pub fn radio(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, mid: f32, spec: Radio) -> Rect {
    let d = ctx.px(spec.size);
    spec.paint(layer, ctx, Point::new(x.round(), (mid - d / 2.0).round()))
}

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
    let fill = if hovered { base.mix(Rgba::WHITE, 0.12 * (1.0 - busy)) } else { base };
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
    if busy > 0.05 {
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
    let p = ctx.palette;
    let fill = if !enabled {
        p.text.with_alpha(0.08)
    } else if hovered {
        base.mix(Rgba::WHITE, 0.12)
    } else {
        base
    };
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(ctx.px(radius::MD)));
    centered_label(layer, text, ctx, rect, label, if enabled { Rgba::WHITE } else { p.text_muted });
}

/// The height of a view header's buttons (the Git window's Commit / Refresh / Fetch / Pull /
/// Push): smaller than a dialog's [`BUTTON_H`].
pub const TOOLBAR_BUTTON_H: f32 = 30.0;

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
    (text.shape_line(label, &font).width + ctx.px(14.0) + ctx.px(30.0)).round()
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
    let icon_size = ctx.px(14.0);
    if busy {
        let spinner = Loader { size: 14.0, color: Some(color), ..Loader::default() };
        loader(layer, text, ctx, rect.min_x() + ctx.px(11.0), rect.center().y, &spinner);
    } else if let Some(id) = icons.id(button.icon) {
        let ir = Rect::new(
            rect.min_x() + ctx.px(11.0),
            (rect.min_y() + (rect.size.height - icon_size) / 2.0).round(),
            icon_size,
            icon_size,
        );
        ui_kit::draw_icon(layer, icons, id, ir, ctx.fade(color));
    }
    let font = toolbar_button_font(ctx);
    let shaped = text.shape_line(button.label, &font);
    draw_text(
        layer,
        &shaped,
        (rect.min_x() + ctx.px(19.0) + icon_size).round(),
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

/// What [`choice`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct ChoiceSpec<'a> {
    pub x: f32,
    pub mid: f32,
    pub label: &'a str,
    pub selected: bool,
    pub hovered: bool,
}

/// A [`choice`] pill's width for `label` (to lay pills out before drawing them).
pub fn choice_width(text: &mut TextSystem, ctx: &Ctx<'_>, label: &str) -> f32 {
    (text.shape_line(label, &ctx.ui_font(0.0)).width + ctx.px(28.0)).round()
}

/// An option in a choice group (pill), vertically centered on `mid`; the selected one
/// is in the accent color. Returns its area.
pub fn choice(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: ChoiceSpec<'_>,
) -> Rect {
    paint_choice(layer, text, ctx, spec, None)
}

/// A choice pill's leading dot, and the room after it (logical pixels).
const CHOICE_DOT: f32 = 7.0;
const CHOICE_DOT_GAP: f32 = 6.0;

/// [`choice_width`] of a [`choice_with_dot`] pill: its dot's room counted.
pub fn choice_width_with_dot(text: &mut TextSystem, ctx: &Ctx<'_>, label: &str) -> f32 {
    choice_width(text, ctx, label) + ctx.px(CHOICE_DOT + CHOICE_DOT_GAP)
}

/// [`choice`] with a color dot before its label (a severity, a status).
pub fn choice_with_dot(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: ChoiceSpec<'_>,
    dot: Rgba,
) -> Rect {
    paint_choice(layer, text, ctx, spec, Some(dot))
}

fn paint_choice(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: ChoiceSpec<'_>,
    dot: Option<Rgba>,
) -> Rect {
    let ChoiceSpec { x, mid, label, selected, hovered } = spec;
    let font = ctx.ui_font(0.0);
    let shaped = text.shape_line(label, &font);
    let h = ctx.px(32.0);
    let lead = dot.map_or(0.0, |_| ctx.px(CHOICE_DOT + CHOICE_DOT_GAP));
    let rect =
        Rect::new(x, (mid - h / 2.0).round(), (shaped.width + lead + ctx.px(28.0)).round(), h);
    let p = ctx.palette;
    let (fill, border, color) = if selected {
        (p.accent.with_alpha(0.10), p.accent.with_alpha(0.7), p.accent)
    } else if hovered {
        (p.text.with_alpha(0.05), p.border, p.text)
    } else {
        (Rgba::TRANSPARENT, p.border, p.text)
    };
    layer.quad(
        Quad::filled(rect, ctx.fade(fill))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(border)),
    );
    if let Some(dot) = dot {
        let d = ctx.px(CHOICE_DOT);
        let at = Rect::new(rect.min_x() + ctx.px(14.0), (mid - d / 2.0).round(), d, d);
        layer.quad(Quad::filled(at, ctx.fade(dot)).rounded(d / 2.0));
    }
    draw_text(
        layer,
        &shaped,
        rect.min_x() + ctx.px(14.0) + lead,
        optical_baseline(rect.min_y(), h, &font),
        ctx.fade(color),
    );
    rect
}

/// What [`check_row`] draws: its label and state, and, for a list that differs, its
/// corners, its hover's fill, its ✓'s size, the chosen one untinted.
#[derive(Debug, Clone, Copy)]
pub struct CheckRowSpec<'a> {
    pub label: &'a str,
    /// The one in force: tinted in the accent, a ✓ before its label.
    pub checked: bool,
    pub hovered: bool,
    /// The row under the keys: lit as under the mouse.
    pub active: bool,
    /// The highlight's corners (logical pixels).
    pub radius: f32,
    /// The hover's fill; `None`: the text, faint.
    pub hover_fill: Option<Rgba>,
    /// The ✓'s side (logical pixels).
    pub icon: f32,
    /// The chosen one tinted in the accent (else only its ✓ and label say it; the hover
    /// shows over it).
    pub tinted: bool,
    /// The chosen one's label in the accent (else in the text's color: only its ✓ says it).
    pub label_tinted: bool,
    /// The room kept at the row's right (logical pixels: a note drawn there), the label
    /// ellipsized before it; `None`: the label drawn whole.
    pub trailing: Option<f32>,
}

impl<'a> CheckRowSpec<'a> {
    /// The usual row: `checked` tinted, `hovered` lit.
    pub fn new(label: &'a str, checked: bool, hovered: bool) -> Self {
        Self {
            label,
            checked,
            hovered,
            active: false,
            radius: radius::MD,
            hover_fill: None,
            icon: CHECK_ICON,
            tinted: true,
            label_tinted: true,
            trailing: None,
        }
    }

    /// Under the keys.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// The hover's fill (a token: `hover_background`).
    pub fn hover_fill(mut self, fill: Rgba) -> Self {
        self.hover_fill = Some(fill);
        self
    }

    /// The ✓'s side (logical pixels).
    pub fn icon(mut self, side: f32) -> Self {
        self.icon = side;
        self
    }

    /// The chosen one not tinted.
    pub fn untinted(mut self) -> Self {
        self.tinted = false;
        self
    }

    /// The chosen one's label in the text's color (a menu's: only its ✓ says it).
    pub fn plain_label(mut self) -> Self {
        self.label_tinted = false;
        self
    }

    /// `trailing` logical pixels kept at the row's right, the label ellipsized before them.
    pub fn trailing(mut self, trailing: f32) -> Self {
        self.trailing = Some(trailing.max(0.0));
        self
    }
}

/// A check row's ✓, the room before it and its label's start (logical pixels), and its
/// fills' alphas (the chosen one's accent, the hover's text).
const CHECK_ICON: f32 = 14.0;
const CHECK_INSET: f32 = 8.0;
const CHECK_LABEL_X: f32 = 30.0;
const CHECK_TINT: f32 = 0.14;
const CHECK_HOVER: f32 = 0.06;

/// One of a list's choices as a row in `rect` (a calendar's presets): its label from the
/// left after a ✓'s gutter, the chosen one tinted in the accent with its ✓, the one under
/// the mouse lit.
pub fn check_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: CheckRowSpec<'_>,
) {
    // A choice to a screen reader: named by its label, checked when it's the one chosen.
    let described = || {
        use ui_kit::access::{Described, Role};
        Described::new(Role::ListItem, spec.label, rect)
            .selected(spec.checked)
            .checked(spec.checked)
    };
    ui_kit::access::group(described, || paint_check_row(layer, text, icons, ctx, rect, spec));
}

fn paint_check_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: CheckRowSpec<'_>,
) {
    let p = ctx.palette;
    let h = rect.size.height;
    let r = ctx.px(spec.radius);
    if spec.checked && spec.tinted {
        layer.quad(Quad::filled(rect, ctx.fade(p.accent.with_alpha(CHECK_TINT))).rounded(r));
    } else if spec.hovered || spec.active {
        let fill = spec.hover_fill.unwrap_or(p.text.with_alpha(CHECK_HOVER));
        layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(r));
    }
    let s = ctx.px(spec.icon);
    if spec.checked
        && let Some(id) = icons.id("check")
    {
        let at = Rect::new(
            rect.min_x() + ctx.px(CHECK_INSET),
            (rect.min_y() + (h - s) / 2.0).round(),
            s,
            s,
        );
        ui_kit::draw_icon(layer, icons, id, at, ctx.fade(p.accent));
    }
    let font = ctx.ui_font(-0.5);
    let x = rect.min_x() + ctx.px(CHECK_LABEL_X);
    let base = optical_baseline(rect.min_y(), h, &font);
    let color = ctx.fade(if spec.checked && spec.label_tinted { p.accent } else { p.text });
    match spec.trailing {
        Some(trailing) => {
            let room = (rect.max_x() - x - ctx.px(trailing)).max(0.0);
            super::draw_ellipsized(layer, text, spec.label, &font, (x, base), room, color);
        }
        None => draw_text(layer, &text.shape_line(spec.label, &font), x, base, color),
    }
}

/// What [`segmented`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct SegmentedSpec<'a> {
    pub right: f32,
    pub mid: f32,
    pub labels: &'a [&'a str],
    pub selected: usize,
    pub hovered: Option<usize>,
}

/// A segmented control (one of a few, side by side in a track: "All · Dark · Light"),
/// ending at `right`, centered on `mid`; the `selected` segment is a raised card. Returns
/// each segment's area (for clicks).
pub fn segmented(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: SegmentedSpec<'_>,
) -> Vec<Rect> {
    let mut out = Vec::with_capacity(spec.labels.len());
    segmented_with(layer, text, ctx, spec, |_, r| out.push(r));
    out
}

/// [`segmented`], each segment's area handed to `each` (its index, its area) instead of
/// collected: nothing allocated (a view drawing it every frame).
pub fn segmented_with(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: SegmentedSpec<'_>,
    each: impl FnMut(usize, Rect),
) {
    paint_segmented(layer, text, None, ctx, spec, SegmentedLook::default(), each);
}

/// The default height of a [`segmented`] control (logical pixels).
pub const SEGMENTED_H: f32 = 32.0;

/// Where a [`segmented_styled`] control stands.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum SegmentPlace {
    /// Ending at its spec's `right`, each segment as wide as its label.
    #[default]
    Right,
    /// Starting at `x`, each segment as wide as its label.
    Left(f32),
    /// Across `width` from `x`, its segments equal, their labels cut with `…` to fit.
    Fill { x: f32, width: f32 },
}

/// How a [`segmented_styled`] control looks beyond its [`SegmentedSpec`]; the default is
/// [`segmented`]'s.
#[derive(Debug, Clone, Copy)]
pub struct SegmentedLook<'a> {
    /// Its height (logical pixels).
    pub height: f32,
    pub place: SegmentPlace,
    /// The segments that can't be chosen now: greyed, never lit, not handed to `each`.
    pub disabled: &'a [usize],
    /// Each segment's icon, before its label (`""`: none); empty: no icons.
    pub icons: &'a [&'a str],
}

impl Default for SegmentedLook<'_> {
    fn default() -> Self {
        Self { height: SEGMENTED_H, place: SegmentPlace::Right, disabled: &[], icons: &[] }
    }
}

/// [`segmented_with`] as `look` says: its height, where it stands (from the left, or
/// filling a width), segments disabled, icons. Only the segments that can be pressed are
/// handed to `each`.
pub fn segmented_styled(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    (spec, look): (SegmentedSpec<'_>, SegmentedLook<'_>),
    each: impl FnMut(usize, Rect),
) {
    paint_segmented(layer, text, Some(icons), ctx, spec, look, each);
}

/// The segments' room inside the track, and the room around a label (logical pixels).
const SEGMENT_INSET: f32 = 3.0;
const SEGMENT_PAD: f32 = 14.0;
/// A segment's icon, and the room after it (logical pixels).
const SEGMENT_ICON: f32 = 14.0;
const SEGMENT_ICON_GAP: f32 = 6.0;

fn paint_segmented(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: Option<&icons::Icons>,
    ctx: &Ctx<'_>,
    spec: SegmentedSpec<'_>,
    look: SegmentedLook<'_>,
    mut each: impl FnMut(usize, Rect),
) {
    let SegmentedSpec { right, mid, labels, selected, hovered } = spec;
    let p = ctx.palette;
    let font = ctx.ui_font(-0.5);
    let (h, inset, pad) = (ctx.px(look.height), ctx.px(SEGMENT_INSET), ctx.px(SEGMENT_PAD));
    let icon_of = |i: usize| {
        let name = look.icons.get(i).copied().filter(|n| !n.is_empty())?;
        icons?.id(name)
    };
    let icon_w =
        |i: usize| icon_of(i).map_or(0.0, |_| ctx.px(SEGMENT_ICON) + ctx.px(SEGMENT_ICON_GAP));
    let natural = |text: &mut TextSystem, i: usize, l: &str| {
        (text.shape_line(l, &font).width + icon_w(i) + 2.0 * pad).round()
    };
    let (track, fill_w) = match look.place {
        SegmentPlace::Fill { x, width } => {
            let cell = ((width - 2.0 * inset) / labels.len().max(1) as f32).floor();
            (Rect::new(x.round(), (mid - h / 2.0).round(), width, h), Some(cell))
        }
        SegmentPlace::Right | SegmentPlace::Left(_) => {
            let total = labels.iter().enumerate().map(|(i, l)| natural(text, i, l)).sum::<f32>()
                + 2.0 * inset;
            let x = match look.place {
                SegmentPlace::Left(x) => x,
                _ => right - total,
            };
            (Rect::new(x.round(), (mid - h / 2.0).round(), total, h), None)
        }
    };
    layer.quad(
        Quad::filled(track, ctx.fade(p.text.with_alpha(0.05)))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(p.border)),
    );
    let mut x = track.min_x() + inset;
    for (i, label) in labels.iter().enumerate() {
        let w = match fill_w {
            Some(cell) => cell,
            None => natural(text, i, label),
        };
        let seg = Rect::new(x, track.min_y() + inset, w, h - 2.0 * inset);
        let on = i == selected;
        let off = look.disabled.contains(&i);
        if on {
            layer.quad(
                Quad::filled(seg, ctx.fade(p.popup_background))
                    .rounded(ctx.px(radius::MD) - inset)
                    .bordered(ctx.line(), ctx.fade(p.card_border)),
            );
        } else if hovered == Some(i) && !off {
            layer.quad(
                Quad::filled(seg, ctx.fade(p.text.with_alpha(0.04)))
                    .rounded(ctx.px(radius::MD) - inset),
            );
        }
        let color = match (off, on) {
            (true, _) => p.text_muted.with_alpha(0.5),
            (false, true) => p.text,
            (false, false) => p.text_muted,
        };
        let room = (w - 2.0 * pad - icon_w(i)).max(0.0);
        let shaped = match fill_w {
            Some(_) if text.shape_line(label, &font).width > room => {
                let cut = ellipsize(text, label, &font, room);
                text.shape_line(&cut, &font)
            }
            _ => text.shape_line(label, &font),
        };
        let content = icon_w(i) + shaped.width;
        let mut cx = (seg.min_x() + (w - content) / 2.0).round();
        // One of a few to a screen reader: named by its label, chosen when it's the one.
        let described = || {
            use ui_kit::access::{Described, Role};
            Described::new(Role::Radio, *label, seg).checked(on).disabled(off)
        };
        ui_kit::access::group(described, || {
            if let (Some(id), Some(icons)) = (icon_of(i), icons) {
                let s = ctx.px(SEGMENT_ICON);
                let mid = seg.min_y() + seg.size.height / 2.0;
                let r = Rect::new(cx, (mid - s / 2.0).round(), s, s);
                ui_kit::draw_icon(layer, icons, id, r, ctx.fade(color));
                cx += icon_w(i);
            }
            draw_text(
                layer,
                &shaped,
                cx,
                optical_baseline(seg.min_y(), seg.size.height, &font),
                ctx.fade(color),
            );
        });
        if !off {
            each(i, seg);
        }
        x += w;
    }
}

/// How wide [`segmented`] draws these labels (to place it from its left edge).
pub fn segmented_width(text: &mut TextSystem, ctx: &Ctx<'_>, labels: &[&str]) -> f32 {
    let font = ctx.ui_font(-0.5);
    labels.iter().map(|l| (text.shape_line(l, &font).width + ctx.px(28.0)).round()).sum::<f32>()
        + ctx.px(6.0)
}

/// What [`stepper`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct StepperSpec<'a> {
    pub x: f32,
    pub width: f32,
    pub mid: f32,
    pub labels: &'a [&'a str],
    pub current: usize,
}

/// A wizard's steps across `width` from `x`, centered on `mid`: each a numbered circle and
/// its name, joined by lines — the done ones filled with a check, the current one ringed in
/// the accent (its name bold), the next ones muted.
pub fn stepper(layer: &mut Layer, text: &mut TextSystem, ctx: &Ctx<'_>, spec: StepperSpec<'_>) {
    let StepperSpec { x, width, mid, labels, current } = spec;
    let p = ctx.palette;
    let font = ctx.ui_font(-0.5);
    let bold = font.clone().with_weight(Font::SEMIBOLD);
    let number = ctx.ui_font(-2.0).with_weight(Font::SEMIBOLD);
    let d = ctx.px(22.0);
    let gap = ctx.px(10.0);
    let shaped: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| text.shape_line(l, if i == current { &bold } else { &font }))
        .collect();
    let items: f32 = shaped.iter().map(|s| d + ctx.px(8.0) + s.width).sum();
    // The lines share what's left (none narrower than a short dash).
    let joins = labels.len().saturating_sub(1).max(1) as f32;
    let line_w = ((width - items - 2.0 * gap * joins) / joins).max(ctx.px(16.0));
    let mut cx = x;
    for (i, label) in shaped.iter().enumerate() {
        let circle = Rect::new(cx.round(), (mid - d / 2.0).round(), d, d);
        let (fill, ring, color) = if i < current {
            (p.accent, p.accent, Rgba::WHITE)
        } else if i == current {
            (p.accent.with_alpha(0.08), p.accent, p.accent)
        } else {
            (Rgba::TRANSPARENT, p.border, p.text_muted)
        };
        if i == current {
            let halo = ctx.px(3.0);
            layer.quad(
                Quad::filled(
                    Rect::new(
                        circle.min_x() - halo,
                        circle.min_y() - halo,
                        d + 2.0 * halo,
                        d + 2.0 * halo,
                    ),
                    ctx.fade(p.accent.with_alpha(0.12)),
                )
                .rounded(d / 2.0 + halo),
            );
        }
        layer.quad(
            Quad::filled(circle, ctx.fade(fill))
                .rounded(d / 2.0)
                .bordered(ctx.px(1.5), ctx.fade(ring)),
        );
        let mark = if i < current { "✓".to_owned() } else { (i + 1).to_string() };
        let m = text.shape_line(&mark, &number);
        draw_text(
            layer,
            &m,
            (circle.min_x() + (d - m.width) / 2.0).round(),
            optical_baseline(circle.min_y(), d, &number),
            ctx.fade(color),
        );
        let lx = circle.max_x() + ctx.px(8.0);
        let label_color = if i == current { p.text } else { p.text_muted };
        let f = if i == current { &bold } else { &font };
        draw_text(layer, label, lx, optical_baseline(circle.min_y(), d, f), ctx.fade(label_color));
        cx = lx + label.width;
        if i + 1 < shaped.len() {
            let line = Rect::new(cx + gap, mid.round(), line_w, ctx.px(1.5).max(1.0));
            let done = i < current;
            layer.quad(Quad::filled(line, ctx.fade(if done { p.accent } else { p.border })));
            cx = line.max_x() + gap;
        }
    }
}

/// What [`radio_card`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct RadioCardSpec<'a> {
    pub rect: Rect,
    pub title: &'a str,
    pub detail: &'a str,
    pub selected: bool,
    pub hovered: bool,
}

/// A choice as a card: a radio, a title and a line under it (`rect`); the chosen one
/// ringed in the accent on a light tint.
pub fn radio_card(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: RadioCardSpec<'_>,
) {
    let RadioCardSpec { rect, title, detail, selected, hovered } = spec;
    let p = ctx.palette;
    let (fill, border) = if selected {
        (p.accent.with_alpha(0.06), p.accent)
    } else if hovered {
        (p.text.with_alpha(0.03), p.text_muted.with_alpha(0.6))
    } else {
        (Rgba::TRANSPARENT, p.border)
    };
    layer.quad(
        Quad::filled(rect, ctx.fade(fill))
            .rounded(ctx.px(radius::LG))
            .bordered(if selected { ctx.px(1.5) } else { ctx.line() }, ctx.fade(border)),
    );
    let pad = ctx.px(16.0);
    let font = ctx.ui_font(0.0).with_weight(Font::SEMIBOLD);
    let small = ctx.ui_font(-1.0);
    let d = ctx.px(18.0);
    let top = rect.min_y() + pad;
    let radio =
        Radio::new(selected).paint(layer, ctx, Point::new(rect.min_x() + pad, top + ctx.px(1.0)));
    let tx = radio.max_x() + ctx.px(12.0);
    let room = (rect.max_x() - pad - tx).max(0.0);
    let title = ellipsize(text, title, &font, room);
    let shaped = text.shape_line(&title, &font);
    draw_text(layer, &shaped, tx, optical_baseline(top, d + ctx.px(2.0), &font), ctx.fade(p.text));
    // The line under it, wrapped on two lines at most.
    let mut y = top + d + ctx.px(8.0);
    for l in wrap(text, detail, &small, room, 2) {
        let shaped = text.shape_line(&l, &small);
        draw_text(layer, &shaped, tx, y + small.size, ctx.fade(p.text_muted));
        y += small.size * 1.45;
    }
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
    let h = ctx.px(34.0);
    let w = (shaped.width + key_w + ctx.px(38.0)).round();
    let rect = Rect::new((right - w).round(), (mid - h / 2.0).round(), w, h);
    let fill = if !enabled {
        p.text.with_alpha(0.08)
    } else if hovered {
        base.mix(Rgba::WHITE, 0.12)
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

/// The app's search field: a pill with the magnifier on the left (as in the project panel).
/// Returns the area where the typed text is drawn.
pub fn search_field(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    focused: bool,
) -> Rect {
    let p = ctx.palette;
    let border = if focused { p.search_field_border_focused } else { p.search_field_border };
    layer.quad(
        Quad::filled(rect, ctx.fade(p.search_field_background))
            .rounded(rect.size.height / 2.0)
            .bordered(ctx.line(), ctx.fade(border)),
    );
    let size = ctx.px(15.0);
    let x = rect.min_x() + ctx.px(13.0);
    if let Some(id) = icons.id("search") {
        let r = Rect::new(x, (rect.min_y() + (rect.size.height - size) / 2.0).round(), size, size);
        ui_kit::draw_icon(layer, icons, id, r, ctx.fade(p.text_muted));
    }
    let left = x + size + ctx.px(9.0);
    Rect::new(left, rect.min_y(), rect.max_x() - left - ctx.px(14.0), rect.size.height)
}

/// A form's text field style (a dialog's, a wizard's) in `font`; `invalid`: its border in
/// the danger color (what's typed can't be used).
pub fn field_style(ctx: &Ctx<'_>, font: Font, invalid: bool) -> ui_kit::TextInputStyle {
    let p = ctx.palette;
    ui_kit::TextInputStyle {
        font,
        background: p.input_background,
        border: if invalid { p.danger } else { p.input_border },
        border_focused: if invalid { p.danger } else { p.accent },
        text: p.text,
        placeholder: p.text_muted,
        selection: p.selection,
        caret: p.caret,
        radius: ctx.px(super::radius::MD),
        border_width: ctx.line(),
        padding: ctx.px(10.0),
        caret_width: ctx.px(1.5).max(1.0),
        lead: 0.0,
        trail: 0.0,
    }
}

/// A search field's text style: the app's search pill, room left for its magnifier.
pub fn search_input_style(ctx: &Ctx<'_>, height: f32) -> ui_kit::TextInputStyle {
    let p = ctx.palette;
    ui_kit::TextInputStyle {
        font: ctx.ui_font(0.0),
        background: p.search_field_background,
        border: p.search_field_border,
        border_focused: p.search_field_border_focused,
        text: p.text,
        placeholder: p.text_muted,
        selection: p.selection,
        caret: p.caret,
        radius: height / 2.0,
        border_width: ctx.line(),
        padding: ctx.px(14.0),
        caret_width: ctx.px(1.5).max(1.0),
        lead: ctx.px(22.0),
        trail: 0.0,
    }
}

/// A search field being typed in (`input`) in `rect`: the pill, its magnifier, the text
/// (the placeholder while it's empty).
pub fn search_input(
    pen: &mut LayerPainter<'_>,
    rect: Rect,
    input: &ui_kit::TextInput,
    focused: bool,
    placeholder: &str,
) {
    let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let style = search_input_style(ctx, rect.size.height);
    input.paint(layer, text, rect, focused, placeholder, &style);
    if let Some(id) = icons.id("search") {
        let s = ctx.px(15.0);
        let r = Rect::new(
            (rect.min_x() + ctx.px(13.0)).round(),
            (rect.min_y() + (rect.size.height - s) / 2.0).round(),
            s,
            s,
        );
        ui_kit::draw_icon(layer, icons, id, r, ctx.fade(ctx.palette.text_muted));
    }
}

/// The round clear button of a field (×), centered on `(cx, mid)`. Returns its area.
pub fn clear_button(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    cx: f32,
    mid: f32,
    hovered: bool,
) -> Rect {
    let size = ctx.px(18.0);
    let r = Rect::new((cx - size / 2.0).round(), (mid - size / 2.0).round(), size, size);
    let p = ctx.palette;
    let fill = p.text.with_alpha(if hovered { 0.12 } else { 0.06 });
    layer.quad(Quad::filled(r, ctx.fade(fill)).rounded(size / 2.0));
    if let Some(id) = icons.id("x") {
        let icon = ctx.px(9.0);
        let ir = Rect::new(
            (r.min_x() + (size - icon) / 2.0).round(),
            (r.min_y() + (size - icon) / 2.0).round(),
            icon,
            icon,
        );
        let color = if hovered { p.text } else { p.text_muted };
        ui_kit::draw_icon(layer, icons, id, ir, ctx.fade(color));
    }
    r
}
