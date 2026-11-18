//! The hints that a list scrolls (faded edges, arrows).

use super::*;

/// Which way an area scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

/// What [`scroll_hints`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct ScrollHintsSpec {
    pub area: Rect,
    pub axis: Axis,
    pub before: bool,
    pub after: bool,
    pub background: Rgba,
}

/// There's more to see in a scrolled area: a soft, glass-like fade into `background` at
/// each edge that hides content (above / left: `before`, below / right: `after`) — eased,
/// so nothing looks cut — with a small translucent chip pointing there. Painted over the
/// content, inside `area`'s border (a card's edge stays visible).
pub fn scroll_hints(layer: &mut Layer, icons: &icons::Icons, ctx: &Ctx<'_>, spec: ScrollHintsSpec) {
    let ScrollHintsSpec { area, axis, before, after, background } = spec;
    const STEPS: usize = 24;
    let inset = (2.0 * ctx.line()).ceil();
    let area = match axis {
        Axis::Vertical => Rect::new(
            area.min_x() + inset,
            area.min_y(),
            (area.size.width - 2.0 * inset).max(0.0),
            area.size.height,
        ),
        Axis::Horizontal => Rect::new(
            area.min_x(),
            area.min_y() + inset,
            area.size.width,
            (area.size.height - 2.0 * inset).max(0.0),
        ),
    };
    let depth = ctx.px(44.0).min(match axis {
        Axis::Vertical => area.size.height / 3.0,
        Axis::Horizontal => area.size.width / 3.0,
    });
    let chip = ctx.px(22.0);
    let step = depth / STEPS as f32;
    for (show, at_start) in [(before, true), (after, false)] {
        if !show {
            continue;
        }
        // The fade: whole-pixel strips, opaque at the edge easing out to clear.
        for i in 0..STEPS {
            let t = (i as f32 + 0.5) / STEPS as f32;
            let alpha = (1.0 - t) * (1.0 - t) * 0.95;
            let from = (i as f32 * step).round();
            let to = ((i + 1) as f32 * step).round();
            if to <= from {
                continue;
            }
            let strip = match (axis, at_start) {
                (Axis::Vertical, true) => {
                    Rect::new(area.min_x(), area.min_y() + from, area.size.width, to - from)
                }
                (Axis::Vertical, false) => {
                    Rect::new(area.min_x(), area.max_y() - to, area.size.width, to - from)
                }
                (Axis::Horizontal, true) => {
                    Rect::new(area.min_x() + from, area.min_y(), to - from, area.size.height)
                }
                (Axis::Horizontal, false) => {
                    Rect::new(area.max_x() - to, area.min_y(), to - from, area.size.height)
                }
            };
            layer.quad(Quad::filled(strip, ctx.fade(background.with_alpha(alpha))));
        }
        // The chevron, in a translucent round chip near the edge's middle.
        let gap = ctx.px(6.0) + chip / 2.0;
        let (cx, cy, icon) = match (axis, at_start) {
            (Axis::Vertical, true) => {
                (area.min_x() + area.size.width / 2.0, area.min_y() + gap, "chevron-up")
            }
            (Axis::Vertical, false) => {
                (area.min_x() + area.size.width / 2.0, area.max_y() - gap, "chevron-down")
            }
            (Axis::Horizontal, true) => {
                (area.min_x() + gap, area.min_y() + area.size.height / 2.0, "chevron-left")
            }
            (Axis::Horizontal, false) => {
                (area.max_x() - gap, area.min_y() + area.size.height / 2.0, "chevron-right")
            }
        };
        let r = Rect::new((cx - chip / 2.0).round(), (cy - chip / 2.0).round(), chip, chip);
        layer.quad(
            Quad::filled(r, ctx.fade(background.with_alpha(0.72)))
                .rounded(chip / 2.0)
                .bordered(ctx.line(), ctx.fade(ctx.palette.popup_border.with_alpha(0.7))),
        );
        if let Some(id) = icons.id(icon) {
            let s = ctx.px(12.0);
            let ir = Rect::new(r.min_x() + (chip - s) / 2.0, r.min_y() + (chip - s) / 2.0, s, s);
            ui_kit::draw_icon(layer, icons, id, ir, ctx.fade(ctx.palette.text_muted));
        }
    }
}
