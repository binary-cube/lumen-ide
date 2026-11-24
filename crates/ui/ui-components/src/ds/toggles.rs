//! Controls that are on or off: the switch, the checkbox, the radio and the radio card.

use super::*;

/// A toggle (on / off), vertically centered on `mid`. Returns its area.
pub fn switch(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, mid: f32, on: bool, hovered: bool) -> Rect {
    switch_sized(layer, ctx, (x, mid), (ctx.px(SWITCH.0), ctx.px(SWITCH.1)), on, hovered, true)
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
    let sized = (ctx.px(SWITCH.0), ctx.px(SWITCH.1));
    switch_sized(layer, ctx, (x, mid), sized, on, hovered && enabled, enabled)
}

/// A switch's width and height (logical pixels).
pub const SWITCH: (f32, f32) = (36.0, 20.0);

/// The width of a [`small_switch`], and its height.
pub const SMALL_SWITCH_W: f32 = 26.0;
const SMALL_SWITCH_H: f32 = 15.0;

/// What a switch off limits keeps of its colors.
pub const DISABLED_ALPHA: f32 = 0.45;

/// The switch made small, for a menu's row: only says how a setting stands.
pub fn small_switch(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, mid: f32, on: bool) -> Rect {
    switch_sized(
        layer,
        ctx,
        (x, mid),
        (ctx.px(SMALL_SWITCH_W), ctx.px(SMALL_SWITCH_H)),
        on,
        false,
        true,
    )
}

/// The room between a switch's knob and its edge (logical pixels).
const SWITCH_KNOB_INSET: f32 = 2.0;

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
    let inset = ctx.px(SWITCH_KNOB_INSET);
    // Both insets rounded as one (as wide at every scale).
    let knob = h - ctx.px(2.0 * SWITCH_KNOB_INSET);
    let kx = if on { rect.max_x() - inset - knob } else { rect.min_x() + inset };
    let kr = Rect::new(kx, rect.min_y() + inset, knob, knob);
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
/// Its corners' radius (logical pixels).
const CHECKBOX_RADIUS: f32 = 4.0;
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
            layer.quad(Quad::filled(r, Rgba::TRANSPARENT).rounded(px(CHECKBOX_RADIUS)).bordered(
                ctx.line().max(1.0),
                ctx.fade(ctx.palette.text_muted.with_alpha(border)),
            ));
            return r;
        }
        let fill = if self.enabled { ctx.palette.accent } else { ctx.palette.text_muted };
        layer.quad(Quad::filled(r, ctx.fade(fill)).rounded(px(CHECKBOX_RADIUS)));
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

/// What [`radio_card`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct RadioCardSpec<'a> {
    pub rect: Rect,
    pub title: &'a str,
    pub detail: &'a str,
    pub selected: bool,
    pub hovered: bool,
}

impl<'a> RadioCardSpec<'a> {
    /// A card in `rect`: its title and the line under it, not chosen, not under the mouse.
    pub fn new(rect: Rect, title: &'a str, detail: &'a str) -> Self {
        Self { rect, title, detail, selected: false, hovered: false }
    }

    /// The one in force.
    pub fn selected(self, selected: bool) -> Self {
        Self { selected, ..self }
    }

    pub fn hovered(self, hovered: bool) -> Self {
        Self { hovered, ..self }
    }
}

/// A radio card's padding (logical pixels).
const RADIO_CARD_PAD: f32 = 16.0;

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
    let pad = ctx.px(RADIO_CARD_PAD);
    let font = ctx.ui_font(0.0).with_weight(Font::SEMIBOLD);
    let small = ctx.ui_font(-1.0);
    let d = ctx.px(RADIO);
    let top = rect.min_y() + pad;
    let radio =
        Radio::new(selected).paint(layer, ctx, Point::new(rect.min_x() + pad, top + ctx.px(1.0)));
    let tx = radio.max_x() + ctx.px(12.0);
    let room = (rect.max_x() - pad - tx).max(0.0);
    let baseline = optical_baseline(top, d + ctx.px(2.0), &font);
    draw_ellipsized(layer, text, title, &font, (tx, baseline), room, ctx.fade(p.text));
    // The line under it, wrapped on two lines at most.
    let mut y = top + d + ctx.px(8.0);
    for l in wrap(text, detail, &small, room, 2) {
        let shaped = text.shape_line(&l, &small);
        draw_text(layer, &shaped, tx, y + small.size, ctx.fade(p.text_muted));
        y += small.size * 1.45;
    }
}
