//! Small controls for a properties panel: a number with − and + on its sides (its value
//! clickable, to be typed), a row of color swatches — the theme's default first, then
//! "none" (crossed out), then the colors — the chosen one ringed, a color's swatch alone
//! (ringed under the mouse and while picked), and a color's field (its swatch, its value, a
//! pipette) that opens the color picker.

use super::*;

/// Where a [`number_field`]'s parts went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberArea {
    pub minus: Rect,
    pub value: Rect,
    pub plus: Rect,
}

/// A number in `rect`: − on its left, + on its right, `shown` between (a field being typed
/// in: `typing`, its text drawn by the caller over [`NumberArea::value`]).
pub fn number_field(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    shown: &str,
    typing: bool,
) -> NumberArea {
    let p = ctx.palette;
    let border = if typing { p.search_field_border_focused } else { p.input_border };
    layer.quad(
        Quad::filled(rect, ctx.fade(p.input_background))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(border)),
    );
    let side = rect.size.height.min(rect.size.width / 3.0);
    let minus = Rect::new(rect.min_x(), rect.min_y(), side, rect.size.height);
    let plus = Rect::new(rect.max_x() - side, rect.min_y(), side, rect.size.height);
    let value = Rect::new(
        minus.max_x(),
        rect.min_y(),
        (plus.min_x() - minus.max_x()).max(0.0),
        rect.size.height,
    );
    let font = ctx.ui_font(-0.5);
    for (r, sign) in [(minus, "−"), (plus, "+")] {
        let shaped = text.shape_line(sign, &font);
        let x = (r.center().x - shaped.width / 2.0).round();
        draw_text(
            layer,
            &shaped,
            x,
            optical_baseline(r.min_y(), r.size.height, &font),
            ctx.fade(p.text_muted),
        );
    }
    if !typing {
        let shaped = text.shape_line(shown, &font);
        let x = (value.center().x - shaped.width / 2.0).round();
        draw_text(
            layer,
            &shaped,
            x,
            optical_baseline(value.min_y(), value.size.height, &font),
            ctx.fade(p.text),
        );
    }
    NumberArea { minus, value, plus }
}

/// A swatch: the theme's default, none at all, or a color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Swatch {
    Default,
    None,
    Color(Rgba),
}

/// A row of `swatches` from `x`, centered on `mid`, `chosen` ringed. Returns each one's area.
pub fn swatch_row(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    (x, mid): (f32, f32),
    swatches: &[Swatch],
    chosen: Option<usize>,
) -> Vec<Rect> {
    let p = ctx.palette;
    let (size, gap) = (ctx.px(18.0), ctx.px(5.0));
    let mut out = Vec::with_capacity(swatches.len());
    for (i, swatch) in swatches.iter().enumerate() {
        let r = Rect::new(
            (x + (size + gap) * i as f32).round(),
            (mid - size / 2.0).round(),
            size,
            size,
        );
        let round = ctx.px(4.0);
        match swatch {
            Swatch::Color(c) => {
                layer.quad(
                    Quad::filled(r, ctx.fade(*c))
                        .rounded(round)
                        .bordered(ctx.line(), ctx.fade(p.border)),
                );
            }
            Swatch::Default => {
                // The theme's: half its text color, half its background.
                layer.quad(
                    Quad::filled(r, ctx.fade(p.editor_background))
                        .rounded(round)
                        .bordered(ctx.line(), ctx.fade(p.border)),
                );
                let half = Rect::new(r.min_x(), r.min_y(), r.size.width / 2.0, r.size.height);
                layer.quad(Quad::filled(half, ctx.fade(p.text_muted)).rounded(round));
            }
            Swatch::None => {
                layer.quad(
                    Quad::filled(r, ctx.fade(p.input_background))
                        .rounded(round)
                        .bordered(ctx.line(), ctx.fade(p.border)),
                );
                // Crossed out: a stroke of small squares across its diagonal.
                let n = 8;
                let dot = ctx.px(1.5);
                for k in 0..=n {
                    let t = k as f32 / n as f32;
                    let (cx, cy) = (r.min_x() + r.size.width * t, r.max_y() - r.size.height * t);
                    layer.quad(Quad::filled(
                        Rect::new(cx - dot / 2.0, cy - dot / 2.0, dot, dot),
                        ctx.fade(p.danger),
                    ));
                }
            }
        }
        if chosen == Some(i) {
            let ring = ctx.px(2.0);
            let outer = Rect::new(
                r.min_x() - ring - 1.0,
                r.min_y() - ring - 1.0,
                r.size.width + 2.0 * (ring + 1.0),
                r.size.height + 2.0 * (ring + 1.0),
            );
            layer.quad(
                Quad::filled(outer, Rgba::TRANSPARENT)
                    .rounded(round + ring)
                    .bordered(ring, ctx.fade(p.accent)),
            );
        }
        out.push(r);
    }
    out
}

/// How wide [`swatch_row`] draws `count` swatches.
pub fn swatch_row_width(ctx: &Ctx<'_>, count: usize) -> f32 {
    let (size, gap) = (ctx.px(18.0), ctx.px(5.0));
    count as f32 * size + count.saturating_sub(1) as f32 * gap
}

/// A color's swatch's corners, its ring at rest and under the mouse (alpha of the text), in
/// logical pixels.
const SWATCH_RADIUS: f32 = 6.0;
const SWATCH_RING: f32 = 0.18;
const SWATCH_RING_HOVER: f32 = 0.5;
/// A ring around a swatch: its gap from it and its width (logical pixels), its alpha of the
/// muted text under the mouse.
const SWATCH_AROUND_GAP: f32 = 3.0;
const SWATCH_AROUND_W: f32 = 1.5;
const SWATCH_AROUND_HOVER: f32 = 0.5;

/// How a [`ColorSwatch`] is ringed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SwatchRing {
    /// Its own edge: the text's at rest, stronger under the mouse, the accent while picked.
    #[default]
    Edge,
    /// No edge; a ring around it at a gap: in its color while it's the one chosen, muted
    /// under the mouse (a color to choose among a few).
    Around { chosen: bool },
}

/// A color's swatch alone (one of a list of colors, each opening the picker, or one of a few
/// to choose from): ringed by its state ([`SwatchRing`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorSwatch {
    pub color: Rgba,
    pub hovered: bool,
    /// Its color being picked (the picker open on it).
    pub editing: bool,
    /// Its corners (logical pixels).
    pub radius: f32,
    pub ring: SwatchRing,
}

impl ColorSwatch {
    pub fn new(color: Rgba) -> Self {
        Self {
            color,
            hovered: false,
            editing: false,
            radius: SWATCH_RADIUS,
            ring: SwatchRing::Edge,
        }
    }

    /// Ringed around at a gap, in its color when `chosen` (a color to choose among a few).
    pub fn ringed_around(mut self, chosen: bool) -> Self {
        self.ring = SwatchRing::Around { chosen };
        self
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Its color being picked.
    pub fn editing(mut self, editing: bool) -> Self {
        self.editing = editing;
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Its ring's color.
    pub fn ring(&self, p: &Palette) -> Rgba {
        if self.editing {
            p.accent
        } else if self.hovered {
            p.text.with_alpha(SWATCH_RING_HOVER)
        } else {
            p.text.with_alpha(SWATCH_RING)
        }
    }

    /// Drawn in `rect`.
    pub fn paint(&self, layer: &mut Layer, ctx: &Ctx<'_>, rect: Rect) {
        if let SwatchRing::Around { chosen } = self.ring {
            let r = ctx.px(self.radius);
            if chosen || self.hovered {
                let gap = ctx.px(SWATCH_AROUND_GAP);
                let outer = Rect::new(
                    rect.min_x() - gap,
                    rect.min_y() - gap,
                    rect.size.width + 2.0 * gap,
                    rect.size.height + 2.0 * gap,
                );
                let edge = if chosen {
                    self.color
                } else {
                    ctx.palette.text_muted.with_alpha(SWATCH_AROUND_HOVER)
                };
                layer.quad(
                    Quad::filled(outer, Rgba::TRANSPARENT)
                        .rounded(r + gap)
                        .bordered(ctx.px(SWATCH_AROUND_W), ctx.fade(edge)),
                );
            }
            layer.quad(Quad::filled(rect, ctx.fade(self.color)).rounded(r));
            return;
        }
        layer.quad(
            Quad::filled(rect, ctx.fade(self.color))
                .rounded(ctx.px(self.radius))
                .bordered(ctx.line(), ctx.fade(self.ring(ctx.palette))),
        );
    }
}

/// A color field's fill at rest and under the mouse (alpha of the text), and while picked
/// (alpha of the accent), its border while picked (alpha of the accent).
const COLOR_FIELD_FILL: f32 = 0.03;
const COLOR_FIELD_HOVER: f32 = 0.06;
const COLOR_FIELD_EDITING: f32 = 0.10;
const COLOR_FIELD_EDITING_BORDER: f32 = 0.7;
/// Its swatch's side, its corners, its room from the field's left, the room between it and
/// the value, the icon's side and its room from the right (logical pixels).
const COLOR_FIELD_SWATCH: f32 = 18.0;
const COLOR_FIELD_SWATCH_RADIUS: f32 = 4.0;
const COLOR_FIELD_INSET: f32 = 4.0;
const COLOR_FIELD_GAP: f32 = 10.0;
const COLOR_FIELD_ICON: f32 = 13.0;
const COLOR_FIELD_ICON_INSET: f32 = 8.0;
/// The value's font (relative to the code font).
const COLOR_FIELD_FONT: f32 = -1.5;

/// A field showing a color, clicked to pick it (with [`crate::color_picker`]): its swatch,
/// its value as written (`#1e1e2e`), an icon on its right (a pipette by default); lit under
/// the mouse, in the accent while the picker is open on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorField<'a> {
    pub color: Rgba,
    /// The color as written.
    pub value: &'a str,
    pub hovered: bool,
    /// The picker open on it.
    pub editing: bool,
    /// The icon on its right; `None`: none.
    pub icon: Option<&'a str>,
}

impl<'a> ColorField<'a> {
    pub fn new(color: Rgba, value: &'a str) -> Self {
        Self { color, value, hovered: false, editing: false, icon: Some("pipette") }
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// The picker open on it.
    pub fn editing(mut self, editing: bool) -> Self {
        self.editing = editing;
        self
    }

    /// The icon on its right (`None`: none).
    pub fn icon(mut self, icon: Option<&'a str>) -> Self {
        self.icon = icon;
        self
    }

    /// Drawn in `rect`. Returns its swatch's area (where the picker opens).
    pub fn paint(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ctx: &Ctx<'_>,
        rect: Rect,
    ) -> Rect {
        let p = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let fill = if self.editing {
            p.accent.with_alpha(COLOR_FIELD_EDITING)
        } else if self.hovered {
            p.text.with_alpha(COLOR_FIELD_HOVER)
        } else {
            p.text.with_alpha(COLOR_FIELD_FILL)
        };
        let border =
            if self.editing { p.accent.with_alpha(COLOR_FIELD_EDITING_BORDER) } else { p.border };
        layer.quad(
            Quad::filled(rect, ctx.fade(fill))
                .rounded(px(radius::SM))
                .bordered(ctx.line(), ctx.fade(border)),
        );
        let mid = rect.min_y() + rect.size.height / 2.0;
        let side = px(COLOR_FIELD_SWATCH);
        let swatch =
            Rect::new(rect.min_x() + px(COLOR_FIELD_INSET), (mid - side / 2.0).round(), side, side);
        ColorSwatch::new(self.color).radius(COLOR_FIELD_SWATCH_RADIUS).paint(layer, ctx, swatch);
        let font = ctx.code_font(COLOR_FIELD_FONT);
        let shaped = text.shape_line(self.value, &font);
        draw_text(
            layer,
            &shaped,
            swatch.max_x() + px(COLOR_FIELD_GAP),
            optical_baseline(rect.min_y(), rect.size.height, &font),
            ctx.fade(p.text),
        );
        if let Some(id) = self.icon.and_then(|name| icons.id(name)) {
            let side = px(COLOR_FIELD_ICON);
            let at = Rect::new(
                rect.max_x() - px(COLOR_FIELD_ICON_INSET) - side,
                (mid - side / 2.0).round(),
                side,
                side,
            );
            let lit = self.hovered || self.editing;
            ui_kit::draw_icon(
                layer,
                icons,
                id,
                at,
                ctx.fade(if lit { p.text } else { p.text_muted }),
            );
        }
        swatch
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ds/number.rs"]
mod tests;
