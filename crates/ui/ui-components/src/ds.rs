//! The interface design system: the tokens (spacing, radii, shadows, typography) and
//! the base components used by all panels — floating surfaces
//! (popover, dialog), dimmed backdrop, code pills, badges, circled icons,
//! separators, key footers, empty states.
//!
//! Everything takes its colors from [`Palette`] (hence from the theme) and sizes from the
//! scale factor; components draw into a given [`Layer`] and return what they occupied,
//! stateless.

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::{KeyHint, draw_text, paint_key_hints};

use crate::backdrop::BackdropRole;
use crate::painter::LayerPainter;
use crate::palette::Palette;

mod badges;
mod brand;
mod controls;
mod dropdown;
mod feedback;
mod number;
mod scroll;
mod text;
pub use badges::*;
pub use brand::*;
pub use controls::*;
pub use dropdown::*;
pub use feedback::*;
pub use number::*;
pub use scroll::*;
pub use text::*;

/// Spacing, in logical pixels (4 / 8 scale).
/// The space between a popover (pickers, menus opened from a button or the status bar)
/// and what it is anchored to (logical pixels): the same everywhere.
pub const POPOVER_GAP: f32 = 10.0;

pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Corner radii, in logical pixels.
pub mod radius {
    /// The workbench's cards (the editor's, the side panels').
    pub const CARD: f32 = 10.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 9.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
}

/// How "high" a surface floats (shadow size and intensity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    /// Tooltips, floating pills.
    Low,
    /// Popovers, menus, cards next to the cursor.
    Medium,
    /// Dialogs, the command palette, search.
    High,
}

/// A component's tone (the accent color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Danger,
    /// Something to keep in mind (a transaction left open).
    Warning,
    /// All good (a copy the same as the server's).
    Success,
}

impl Tone {
    pub fn color(self, p: &Palette) -> Rgba {
        match self {
            Self::Neutral => p.text_muted,
            Self::Accent => p.accent,
            Self::Danger => p.danger,
            Self::Warning => p.warning,
            Self::Success => crate::tag_colors::named("green", p).unwrap_or(p.vcs_added),
        }
    }
}

/// The scale factor and opacity (for animated appearances) used for drawing.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    pub palette: &'a Palette,
    pub scale: f32,
    /// 0..1, multiplies all colors (fade).
    pub opacity: f32,
}

impl<'a> Ctx<'a> {
    pub fn new(palette: &'a Palette, scale: f32) -> Self {
        Self { palette, scale, opacity: 1.0 }
    }

    pub fn with_opacity(self, opacity: f32) -> Self {
        Self { opacity, ..self }
    }

    /// Logical → physical pixels, rounded.
    pub fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// Line thickness (1 logical pixel, at least 1 physical).
    pub fn line(&self) -> f32 {
        self.scale.round().max(1.0)
    }

    pub fn fade(&self, c: Rgba) -> Rgba {
        c.with_alpha(c.a * self.opacity)
    }

    pub fn ui_font(&self, delta: f32) -> Font {
        Font::new(self.palette.ui_font.clone(), self.px(self.palette.ui_font_size + delta))
    }

    pub fn code_font(&self, delta: f32) -> Font {
        Font::new(self.palette.code_font.clone(), self.px(self.palette.code_font_size + delta))
    }
}

/// The baseline that *optically* centers text in a row of height `height` starting
/// at `top`: by cap height (~0.72 em), not by ascent / descent (which makes
/// small text look like it sags in short pills).
pub fn optical_baseline(top: f32, height: f32, font: &Font) -> f32 {
    (top + (height + font.size * 0.72) / 2.0).round()
}

/// A floating surface: soft layered shadow, background, thin border.
pub fn surface(layer: &mut Layer, ctx: &Ctx<'_>, rect: Rect, elevation: Elevation, r: f32) {
    surface_with(layer, ctx, rect, elevation, r, ctx.palette.popup_background);
}

/// A floating surface with its own background (e.g. the code hover card's).
pub fn surface_with(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    rect: Rect,
    elevation: Elevation,
    r: f32,
    background: Rgba,
) {
    let p = ctx.palette;
    // The shadow: a "blur" approximated by concentric low-opacity layers that
    // accumulate toward the center (no hard edges). (radius, downward offset, total opacity)
    let (blur, offset, strength) = match elevation {
        Elevation::Low => (6.0, 1.0, 0.10),
        Elevation::Medium => (14.0, 3.0, 0.14),
        Elevation::High => (32.0, 10.0, 0.18),
    };
    const STEPS: usize = 10;
    let blur = ctx.px(blur);
    for i in (1..=STEPS).rev() {
        let k = i as f32 / STEPS as f32;
        let spread = blur * k;
        let shadow = Rect::new(
            rect.min_x() - spread,
            rect.min_y() - spread + ctx.px(offset) * k,
            rect.size.width + 2.0 * spread,
            rect.size.height + 2.0 * spread,
        );
        let color = p.shadow.with_alpha(p.shadow.a * strength / STEPS as f32);
        layer.quad(Quad::filled(shadow, ctx.fade(color)).rounded(ctx.px(r) + spread));
    }
    layer.quad(
        Quad::filled(rect, ctx.fade(background))
            .rounded(ctx.px(r))
            .bordered(ctx.line(), ctx.fade(p.popup_border)),
    );
}

/// The keyboard's focus ring around `rect` (corner radius `r`, physical pixels): a 2 px
/// accent outline just outside it, the same on every control that takes the keys.
pub fn focus_ring(layer: &mut Layer, ctx: &Ctx<'_>, rect: Rect, r: f32) {
    let ring = ctx.px(2.0);
    let outer = Rect::new(
        rect.min_x() - ring,
        rect.min_y() - ring,
        rect.size.width + 2.0 * ring,
        rect.size.height + 2.0 * ring,
    );
    layer.quad(
        Quad::filled(outer, Rgba::TRANSPARENT)
            .rounded(r + ring)
            .bordered(ring, ctx.fade(ctx.palette.accent.with_alpha(0.6))),
    );
}

/// The backdrop under a modal or a dialog (the palette's style), over `viewport`, fading
/// with the context's opacity.
pub fn backdrop(layer: &mut Layer, ctx: &Ctx<'_>, viewport: Rect) {
    backdrop_faded(layer, ctx, viewport, BackdropRole::Dialog, ctx.opacity, None);
}

/// The backdrop under `role`'s window over `area`, `shown` of it (0..1: its animation),
/// leaving `hole` clear (the window's own buttons, on Windows). Draw it first in the
/// overlay's layer.
pub fn backdrop_faded(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    area: Rect,
    role: BackdropRole,
    shown: f32,
    hole: Option<Rect>,
) {
    let style = &ctx.palette.backdrop;
    let tint = style.tint(role);
    let tint = tint.with_alpha(tint.a * shown);
    let radius = style.blur_radius(ctx.scale);
    for part in around(area, hole) {
        // Glass: the window behind blurred first (as much of it as is shown), the color over.
        if let Some(radius) = radius {
            layer.backdrop_blur(part, radius, shown);
        }
        layer.quad(Quad::filled(part, tint));
    }
}

/// `area` without `hole` (a rectangle inside it): the parts around it, at most four.
pub fn around(area: Rect, hole: Option<Rect>) -> Vec<Rect> {
    let Some(h) = hole.filter(|h| h.size.width > 0.0 && h.size.height > 0.0) else {
        return vec![area];
    };
    let parts = [
        Rect::new(area.min_x(), area.min_y(), area.size.width, h.min_y() - area.min_y()),
        Rect::new(area.min_x(), h.min_y(), h.min_x() - area.min_x(), h.size.height),
        Rect::new(h.max_x(), h.min_y(), area.max_x() - h.max_x(), h.size.height),
        Rect::new(area.min_x(), h.max_y(), area.size.width, area.max_y() - h.max_y()),
    ];
    parts.into_iter().filter(|r| r.size.width > 0.0 && r.size.height > 0.0).collect()
}

/// A horizontal separator (at `y`, from `x0` to `x1`).
/// A list row's background: the one chosen (under the keys) in an accent tint, the one under
/// the mouse in a faint one — every list the same (the projects, a picker's).
pub fn row_highlight(layer: &mut Layer, ctx: &Ctx<'_>, r: Rect, selected: bool, hovered: bool) {
    let p = ctx.palette;
    let fill = if selected {
        p.accent.with_alpha(0.10)
    } else if hovered {
        p.text.with_alpha(0.045)
    } else {
        return;
    };
    layer.quad(Quad::filled(r, ctx.fade(fill)).rounded(ctx.px(radius::MD)));
}

pub fn divider(layer: &mut Layer, ctx: &Ctx<'_>, x0: f32, x1: f32, y: f32) {
    layer.quad(Quad::filled(Rect::new(x0, y, x1 - x0, ctx.line()), ctx.fade(ctx.palette.border)));
}

/// A vertical separator.
pub fn divider_vertical(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, y0: f32, y1: f32) {
    layer.quad(Quad::filled(Rect::new(x, y0, ctx.line(), y1 - y0), ctx.fade(ctx.palette.border)));
}

/// A vertical separator that fades: the border color at its middle, transparent toward
/// both ends (whole-pixel steps, no gradient needed from the renderer).
pub fn divider_vertical_faded(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, y0: f32, y1: f32) {
    const STEPS: usize = 24;
    let len = y1 - y0;
    if len <= 0.0 {
        return;
    }
    let color = ctx.fade(ctx.palette.border);
    let mut top = y0.round();
    for i in 0..STEPS {
        let bottom = (y0 + len * (i + 1) as f32 / STEPS as f32).round();
        if bottom > top {
            // From 0 at the ends to 1 at the middle, easing in (a sine's arch).
            let t = (i as f32 + 0.5) / STEPS as f32;
            let k = (std::f32::consts::PI * t).sin();
            layer.quad(Quad::filled(
                Rect::new(x, top, ctx.line(), bottom - top),
                color.with_alpha(color.a * k * k),
            ));
        }
        top = bottom;
    }
}

/// A horizontal separator that fades: the border color at its middle, transparent toward
/// both ends (whole-pixel steps, no gradient needed from the renderer) — the horizontal
/// [`divider_vertical_faded`] (between a strip's groups of buttons).
pub fn divider_horizontal_faded(layer: &mut Layer, ctx: &Ctx<'_>, x0: f32, x1: f32, y: f32) {
    const STEPS: usize = 12;
    let len = x1 - x0;
    if len <= 0.0 {
        return;
    }
    let color = ctx.fade(ctx.palette.border);
    let (y, thick) = (y.round(), ctx.line());
    let mut left = x0.round();
    for i in 0..STEPS {
        let right = (x0 + len * (i + 1) as f32 / STEPS as f32).round();
        if right > left {
            // From 0 at the ends to 1 at the middle, easing in (a sine's arch).
            let t = (i as f32 + 0.5) / STEPS as f32;
            let k = (std::f32::consts::PI * t).sin();
            layer.quad(Quad::filled(
                Rect::new(left, y, right - left, thick),
                color.with_alpha(color.a * k * k),
            ));
        }
        left = right;
    }
}

/// A horizontal separator at `y` from `x0` to `x1` that fades: `color` at its start (right
/// after what it follows: a caption), transparent toward its end (whole-pixel steps, no
/// gradient needed from the renderer). The usual color is `palette.border`.
pub fn divider_faded_right(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    x0: f32,
    x1: f32,
    y: f32,
    color: Rgba,
) {
    const STEPS: usize = 24;
    let len = x1 - x0;
    if len <= 0.0 {
        return;
    }
    let color = ctx.fade(color);
    let (y, thick) = (y.round(), ctx.line());
    let mut left = x0.round();
    for i in 0..STEPS {
        let right = (x0 + len * (i + 1) as f32 / STEPS as f32).round();
        if right > left {
            // From 1 at the start to 0 at the end, easing out (strongest near the start).
            let k = 1.0 - (i as f32 + 0.5) / STEPS as f32;
            layer.quad(Quad::filled(
                Rect::new(left, y, right - left, thick),
                color.with_alpha(color.a * k * k),
            ));
        }
        left = right;
    }
}

/// What a part of a view draws with: its layer, the text system and the design system's
/// context (passed as one to the helpers that draw a card, a row, a cell).
pub struct Pen<'a, 'b> {
    pub layer: &'a mut Layer,
    pub text: &'a mut TextSystem,
    pub ds: &'a Ctx<'b>,
}

impl<'b> Pen<'_, 'b> {
    /// Lends it to one part, keeping it usable after.
    pub fn lend(&mut self) -> Pen<'_, 'b> {
        Pen { layer: self.layer, text: self.text, ds: self.ds }
    }
}

impl std::fmt::Debug for Pen<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pen").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "../tests/unit/ds.rs"]
mod tests;
