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
mod button;
mod choices;
mod controls;
mod dropdown;
mod feedback;
mod number;
mod place;
mod progress;
mod scroll;
mod search;
mod segmented;
mod stepper;
mod text;
mod toggles;
pub use badges::*;
pub use brand::*;
pub use button::*;
pub use choices::*;
pub use controls::*;
pub use dropdown::*;
pub use feedback::*;
pub use number::*;
pub use place::*;
pub use progress::*;
pub use scroll::*;
pub use search::*;
pub use segmented::*;
pub use stepper::*;
pub use text::*;
pub use toggles::*;

/// How faintly a band set apart from what it's under is tinted (the text's alpha): a
/// footer, a bar, a group's card.
pub const BAND_TINT: f32 = 0.022;

/// The space between a popover (pickers, menus opened from a button or the status bar)
/// and what it is anchored to (logical pixels): the same everywhere.
pub const POPOVER_GAP: f32 = 10.0;

/// Spacing, in logical pixels (4 / 8 scale).
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

/// A list row's background: the one chosen (under the keys) in an accent tint, the one under
/// the mouse in a faint one — every list the same (the projects, a picker's).
pub fn row_highlight(layer: &mut Layer, ctx: &Ctx<'_>, r: Rect, selected: bool, hovered: bool) {
    RowHighlight::new(selected, hovered).paint(layer, ctx, r);
}

/// What a row under the mouse is lit with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoverFill {
    /// The text at this alpha (the usual: [`ROW_HOVER`]).
    Faint(f32),
    /// The theme's `hover_background`.
    Background,
    /// The theme's tree hover (`tree_hover`).
    Tree,
    /// A color of the caller's (a severity's, faint).
    Color(Rgba),
}

/// The usual row highlight's alphas: the chosen one's accent, the hovered one's text, and the
/// chosen one's text while its list doesn't have the keys.
pub const ROW_SELECTED: f32 = 0.10;
pub const ROW_HOVER: f32 = 0.045;
pub const ROW_INACTIVE: f32 = 0.07;

/// A row's highlight, as configured: chosen, under the mouse, its list without the keys
/// (an inactive selection: grey), its corners, its fills. Defaults are [`row_highlight`]'s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowHighlight {
    pub selected: bool,
    pub hovered: bool,
    /// The chosen one's list doesn't have the keys: grey, not the accent.
    pub inactive: bool,
    /// Its corners (logical pixels).
    pub radius: f32,
    pub hover: HoverFill,
    /// The chosen one's accent alpha.
    pub selected_alpha: f32,
    /// The chosen one's own fill (a token: `tree_selection`), over the accent's.
    pub selected_fill: Option<Rgba>,
    /// An outline around the fill, a line wide (a drop target); `None`: none.
    pub outline: Option<Rgba>,
    /// The outline's width (logical pixels); `None`: a line.
    pub outline_width: Option<f32>,
}

impl RowHighlight {
    pub fn new(selected: bool, hovered: bool) -> Self {
        Self {
            selected,
            hovered,
            inactive: false,
            radius: radius::MD,
            hover: HoverFill::Faint(ROW_HOVER),
            selected_alpha: ROW_SELECTED,
            selected_fill: None,
            outline: None,
            outline_width: None,
        }
    }

    /// Its list doesn't have the keys.
    pub fn inactive(mut self, inactive: bool) -> Self {
        self.inactive = inactive;
        self
    }

    /// Its corners (logical pixels).
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub fn hover(mut self, hover: HoverFill) -> Self {
        self.hover = hover;
        self
    }

    pub fn selected_alpha(mut self, alpha: f32) -> Self {
        self.selected_alpha = alpha;
        self
    }

    /// The chosen one in `fill` (a token), over the accent's.
    pub fn selected_fill(mut self, fill: Rgba) -> Self {
        self.selected_fill = Some(fill);
        self
    }

    /// Outlined in `color`, a line wide, wherever it's lit.
    pub fn outline(mut self, color: Rgba) -> Self {
        self.outline = Some(color);
        self
    }

    /// Its outline `width` logical pixels wide (on whole pixels, at least a line).
    pub fn outline_width(mut self, width: f32) -> Self {
        self.outline_width = Some(width);
        self
    }

    /// Its fill, if any.
    pub fn fill(&self, p: &Palette) -> Option<Rgba> {
        if self.selected {
            let fill = if self.inactive {
                p.text.with_alpha(ROW_INACTIVE)
            } else {
                self.selected_fill.unwrap_or(p.accent.with_alpha(self.selected_alpha))
            };
            return Some(fill);
        }
        self.hovered.then_some(match self.hover {
            HoverFill::Faint(alpha) => p.text.with_alpha(alpha),
            HoverFill::Background => p.hover_background,
            HoverFill::Tree => p.tree_hover,
            HoverFill::Color(color) => color,
        })
    }

    /// Drawn over `r` (nothing when neither chosen nor hovered).
    pub fn paint(&self, layer: &mut Layer, ctx: &Ctx<'_>, r: Rect) {
        let Some(fill) = self.fill(ctx.palette) else { return };
        let quad = Quad::filled(r, ctx.fade(fill)).rounded(ctx.px(self.radius));
        let width = self.outline_width.map_or(ctx.line(), |w| ctx.px(w).round().max(ctx.line()));
        layer.quad(match self.outline {
            Some(outline) => quad.bordered(width, ctx.fade(outline)),
            None => quad,
        });
    }
}

/// A horizontal separator (at `y`, from `x0` to `x1`).
pub fn divider(layer: &mut Layer, ctx: &Ctx<'_>, x0: f32, x1: f32, y: f32) {
    divider_in(layer, ctx, (x0, x1, y), ctx.palette.border);
}

/// A horizontal separator in `color` (a token: `card_border`, a faded border).
pub fn divider_in(layer: &mut Layer, ctx: &Ctx<'_>, (x0, x1, y): (f32, f32, f32), color: Rgba) {
    layer.quad(Quad::filled(Rect::new(x0, y, x1 - x0, ctx.line()), ctx.fade(color)));
}

/// A horizontal separator in the cards' border color (under a panel's header).
pub fn divider_card(layer: &mut Layer, ctx: &Ctx<'_>, x0: f32, x1: f32, y: f32) {
    divider_in(layer, ctx, (x0, x1, y), ctx.palette.card_border);
}

/// A vertical separator in `color`.
pub fn divider_vertical_in(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    (x, y0, y1): (f32, f32, f32),
    color: Rgba,
) {
    layer.quad(Quad::filled(Rect::new(x, y0, ctx.line(), y1 - y0), ctx.fade(color)));
}

/// A vertical separator.
pub fn divider_vertical(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, y0: f32, y1: f32) {
    layer.quad(Quad::filled(Rect::new(x, y0, ctx.line(), y1 - y0), ctx.fade(ctx.palette.border)));
}

/// A line of `color` along `from..from + len`, drawn in `steps` whole-pixel pieces, each
/// as strong as `strength` says at its middle (0 at its start … 1 at its end): the faded
/// separators' one drawing, no gradient needed from the renderer. `piece` turns a run
/// `(start, end)` into its rectangle.
fn faded_line(
    layer: &mut Layer,
    (from, len, steps): (f32, f32, usize),
    color: Rgba,
    strength: impl Fn(f32) -> f32,
    piece: impl Fn(f32, f32) -> Rect,
) {
    if len <= 0.0 {
        return;
    }
    let mut start = from.round();
    for i in 0..steps {
        let end = (from + len * (i + 1) as f32 / steps as f32).round();
        if end > start {
            let k = strength((i as f32 + 0.5) / steps as f32);
            layer.quad(Quad::filled(piece(start, end), color.with_alpha(color.a * k * k)));
        }
        start = end;
    }
}

/// From 0 at the ends to 1 at the middle, easing in (a sine's arch).
fn arch(t: f32) -> f32 {
    (std::f32::consts::PI * t).sin()
}

/// A vertical separator that fades: the border color at its middle, transparent toward
/// both ends.
pub fn divider_vertical_faded(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, y0: f32, y1: f32) {
    /// Its pieces.
    const STEPS: usize = 24;
    let color = ctx.fade(ctx.palette.border);
    let line = ctx.line();
    faded_line(layer, (y0, y1 - y0, STEPS), color, arch, |top, bottom| {
        Rect::new(x, top, line, bottom - top)
    });
}

/// A horizontal separator that fades: the border color at its middle, transparent toward
/// both ends — the horizontal [`divider_vertical_faded`] (between a strip's groups of
/// buttons).
pub fn divider_horizontal_faded(layer: &mut Layer, ctx: &Ctx<'_>, x0: f32, x1: f32, y: f32) {
    /// Its pieces (a short line: fewer).
    const STEPS: usize = 12;
    let color = ctx.fade(ctx.palette.border);
    let (y, thick) = (y.round(), ctx.line());
    faded_line(layer, (x0, x1 - x0, STEPS), color, arch, |left, right| {
        Rect::new(left, y, right - left, thick)
    });
}

/// A horizontal separator at `y` from `x0` to `x1` that fades: `color` at its start (right
/// after what it follows: a caption), transparent toward its end. The usual color is
/// `palette.border`.
pub fn divider_faded_right(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    x0: f32,
    x1: f32,
    y: f32,
    color: Rgba,
) {
    /// Its pieces.
    const STEPS: usize = 24;
    let color = ctx.fade(color);
    let (y, thick) = (y.round(), ctx.line());
    // From 1 at the start to 0 at the end, easing out (strongest near the start).
    faded_line(
        layer,
        (x0, x1 - x0, STEPS),
        color,
        |t| 1.0 - t,
        |left, right| Rect::new(left, y, right - left, thick),
    );
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
