//! Two small controls for a properties panel: a number with − and + on its sides (its value
//! clickable, to be typed), and a row of color swatches — the theme's default first, then
//! "none" (crossed out), then the colors — the chosen one ringed.

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

#[cfg(test)]
#[path = "../../tests/unit/ds/number.rs"]
mod tests;
