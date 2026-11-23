//! A strip of pill tabs: each tab a rounded pill with an optional icon, its label and a slot
//! on its right for a × (closable) or a pin (pinned); the one in force tinted (in the accent
//! when its strip is the current one, grey otherwise), the one under the mouse lit; under
//! the strip, its border. A bottom tool window's views are such a strip, and any view that
//! keeps tabs of its own draws them with this.
//!
//! A bar of underlined tabs (a form's sections) is the other look: [`UnderlineTab`].
//!
//! It only draws and says where things are ([`PillTabRects`]); its owner keeps which tab
//! is which and what a click does.

use icons::Icons;
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::TextSystem;
use ui_kit::{draw_icon, draw_text};

use crate::ds::{self, Ctx};

/// A pill's height, its label's room on each side, its slot's side and the gaps around it,
/// its icon's side and the gap after it (logical pixels).
pub const TAB_H: f32 = 24.0;
const TAB_PAD: f32 = 10.0;
const SLOT: f32 = 14.0;
const SLOT_GAP: f32 = 4.0;
const SLOT_INSET: f32 = 6.0;
const ICON: f32 = 14.0;
const ICON_GAP: f32 = 6.0;
/// The space between two tabs (logical pixels).
pub const TAB_GAP: f32 = 4.0;
/// The fills' alphas: the one in force in the current strip (accent), in another (text),
/// under the mouse (text).
const ON_LIT: f32 = 0.14;
const ON: f32 = 0.08;
const HOVER: f32 = 0.06;

/// What a tab's right slot holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabSlot {
    /// No slot (a fixed tab: neither pinned nor closable).
    #[default]
    None,
    /// Room for a ×, shown (on the tab in force or under the mouse) or not; `lit` under the
    /// mouse.
    Close { shown: bool, lit: bool },
    /// A pin: it doesn't close by a stray click.
    Pin,
}

/// One pill tab, as configured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillTab<'a> {
    pub label: &'a str,
    /// An icon before the label (its name in the app's set).
    pub icon: Option<&'a str>,
    pub slot: TabSlot,
    /// The one in force in its strip.
    pub on: bool,
    /// Its strip is the current one: the one in force in the accent.
    pub lit: bool,
    pub hovered: bool,
}

impl<'a> PillTab<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, icon: None, slot: TabSlot::None, on: false, lit: false, hovered: false }
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn slot(mut self, slot: TabSlot) -> Self {
        self.slot = slot;
        self
    }

    /// In force (`on`), in the current strip (`lit`).
    pub fn on(mut self, on: bool, lit: bool) -> Self {
        self.on = on;
        self.lit = lit;
        self
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Its width (physical pixels).
    pub fn width(&self, text: &mut TextSystem, ctx: &Ctx<'_>) -> f32 {
        let label = text.shape_line(self.label, &font(ctx)).width;
        self.width_of(label, ctx)
    }

    fn width_of(&self, label: f32, ctx: &Ctx<'_>) -> f32 {
        let px = |v: f32| ctx.px(v);
        let icon = if self.icon.is_some() { px(ICON) + px(ICON_GAP) } else { 0.0 };
        let slot = if self.slot == TabSlot::None { 0.0 } else { px(SLOT) + px(SLOT_GAP) };
        label + px(2.0 * TAB_PAD) + icon + slot
    }

    /// Its fill and its label's color.
    fn colors(&self, p: &crate::palette::Palette) -> (Rgba, Rgba) {
        let fill = if self.on && self.lit {
            p.accent.with_alpha(ON_LIT)
        } else if self.on {
            p.text.with_alpha(ON)
        } else if self.hovered {
            p.text.with_alpha(HOVER)
        } else {
            Rgba::TRANSPARENT
        };
        (fill, if self.on && self.lit { p.accent } else { p.text })
    }
}

/// Where a drawn tab is: its pill and, when shown, its ×.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillTabRects {
    pub pill: Rect,
    pub close: Option<Rect>,
}

/// The tabs' label font.
pub fn font(ctx: &Ctx<'_>) -> text_layout::Font {
    ctx.ui_font(-0.5)
}

/// The pill of a tab `width` wide from `x`, centered on `mid` (a slot kept empty while its
/// tab is dragged, a lifted tab's surface).
pub fn pill(ctx: &Ctx<'_>, (x, mid): (f32, f32), width: f32) -> Rect {
    Rect::new(x, mid - ctx.px(TAB_H / 2.0), width, ctx.px(TAB_H))
}

/// Where `tab` drawn from `x`, centered on `mid`, stands: its pill.
fn tab_pill(text: &mut TextSystem, ctx: &Ctx<'_>, at: (f32, f32), tab: &PillTab<'_>) -> Rect {
    let width = text.shape_line(tab.label, &font(ctx)).width;
    pill(ctx, at, tab.width_of(width, ctx))
}

/// A tab to a screen reader: named by its label, chosen while it's the one in force (its
/// label, drawn in it, is its name).
fn tab_node(label: &str, on: bool, bounds: Rect) -> ui_kit::access::Described {
    use ui_kit::access::{Described, Role};
    Described::new(Role::Tab, label, bounds).selected(on)
}

/// Draws `tab` from `x`, centered on `mid`.
pub fn pill_tab(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ctx: &Ctx<'_>,
    (x, mid): (f32, f32),
    tab: &PillTab<'_>,
) -> PillTabRects {
    if !ui_kit::access::recording() {
        return paint_pill_tab(layer, text, icons, ctx, (x, mid), tab);
    }
    let bounds = tab_pill(text, ctx, (x, mid), tab);
    ui_kit::access::group(
        || tab_node(tab.label, tab.on, bounds),
        || paint_pill_tab(layer, text, icons, ctx, (x, mid), tab),
    )
}

fn paint_pill_tab(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ctx: &Ctx<'_>,
    (x, mid): (f32, f32),
    tab: &PillTab<'_>,
) -> PillTabRects {
    let px = |v: f32| ctx.px(v);
    let p = ctx.palette;
    let font = font(ctx);
    let shaped = text.shape_line(tab.label, &font);
    let pill = tab_pill(text, ctx, (x, mid), tab);
    let (fill, color) = tab.colors(p);
    layer.quad(Quad::filled(pill, ctx.fade(fill)).rounded(px(ds::radius::MD)));
    let mut tx = x + px(TAB_PAD);
    if let Some(id) = tab.icon.and_then(|name| icons.id(name)) {
        let side = px(ICON);
        let at = Rect::new(tx.round(), (mid - side / 2.0).round(), side, side);
        draw_icon(layer, icons, id, at, ctx.fade(color));
        tx += side + px(ICON_GAP);
    } else if tab.icon.is_some() {
        tx += px(ICON) + px(ICON_GAP);
    }
    let base = ds::optical_baseline(pill.min_y(), pill.size.height, &font);
    draw_text(layer, &shaped, tx, base, ctx.fade(color));
    let close = px(SLOT);
    let slot = Rect::new(pill.max_x() - close - px(SLOT_INSET), mid - close / 2.0, close, close);
    let ghost = ds::ButtonSpec::new(ds::ButtonKind::Ghost).rest_tone(ds::Tone::Neutral);
    let close = match tab.slot {
        TabSlot::Pin => {
            ds::button(layer, text, icons, ctx, slot, ghost.icon("pin"));
            None
        }
        TabSlot::Close { shown: true, lit } => {
            ds::button(layer, text, icons, ctx, slot, ghost.icon("x").round().hovered(lit));
            Some(slot)
        }
        TabSlot::Close { shown: false, .. } | TabSlot::None => None,
    };
    PillTabRects { pill, close }
}

/// The strip's border: a line along its bottom edge at `y`, from `x0` to `x1`, in the
/// cards' border color.
pub fn strip_border(layer: &mut Layer, ctx: &Ctx<'_>, x0: f32, x1: f32, y: f32) {
    ds::divider_card(layer, ctx, x0, x1, y);
}

/// An underlined tab's label room on each side by default, and its line's thickness
/// (logical pixels).
const UNDERLINE_PAD: f32 = 10.0;
const UNDERLINE: f32 = 2.0;

/// A tab of a bar of underlined tabs (a form's sections): its label, the one in force in
/// the text's color over a line in the accent along the bar's bottom, the others muted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnderlineTab<'a> {
    pub label: &'a str,
    /// The one in force.
    pub on: bool,
    /// The room on each side of its label (logical pixels).
    pub pad: f32,
}

impl<'a> UnderlineTab<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, on: false, pad: UNDERLINE_PAD }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// The room on each side of its label (logical pixels: a narrow bar's tabs closer).
    pub fn pad(mut self, pad: f32) -> Self {
        self.pad = pad.max(0.0);
        self
    }

    /// Its width in `font` (physical pixels).
    pub fn width(&self, text: &mut TextSystem, ctx: &Ctx<'_>, font: &text_layout::Font) -> f32 {
        text.shape_line(self.label, font).width + 2.0 * ctx.px(self.pad)
    }
}

/// Draws `tab` in `font` from `x`, as high as its bar (`top`, `height`); its area.
pub fn underline_tab(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    font: &text_layout::Font,
    (x, top, height): (f32, f32, f32),
    tab: &UnderlineTab<'_>,
) -> Rect {
    let p = ctx.palette;
    let pad = ctx.px(tab.pad);
    let shaped = text.shape_line(tab.label, font);
    let r = Rect::new(x, top, shaped.width + 2.0 * pad, height);
    let base = ds::optical_baseline(top, height, font);
    let color = if tab.on { p.text } else { p.text_muted };
    ui_kit::access::group(
        || tab_node(tab.label, tab.on, r),
        || draw_text(layer, &shaped, x + pad, base, ctx.fade(color)),
    );
    if tab.on {
        let line = ctx.px(UNDERLINE);
        let under = Rect::new(r.min_x(), r.max_y() - line, r.size.width, line);
        layer.quad(Quad::filled(under, ctx.fade(p.accent)));
    }
    r
}

#[cfg(test)]
#[path = "../tests/unit/tab_strip.rs"]
mod tests;
