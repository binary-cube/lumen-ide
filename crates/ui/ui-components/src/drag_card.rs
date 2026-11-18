//! What's dragged, following the mouse: a small raised card beside the cursor with the
//! thing's icon (or a short colored tag, an HTTP request's method) and its name. One look for
//! every list that moves things by dragging (projects into groups, files into folders, data
//! sources, requests, diagrams), drawn last, over everything in its area.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::Layer;
use text_layout::TextSystem;
use ui_kit::{draw_icon, draw_text};

use crate::ds::{self, Ctx};

/// What the card shows before the name.
#[derive(Debug, Clone, Copy)]
pub enum Lead<'a> {
    /// An icon of the set, in its color.
    Icon(IconId, Rgba),
    /// A short word in its color (an HTTP request's method).
    Tag(&'a str, Rgba),
    Nothing,
}

/// The card's height (logical pixels).
const HEIGHT: f32 = 28.0;
/// The widest a name is shown before it's cut (logical pixels).
const NAME_MAX: f32 = 280.0;

/// Draws the card of `name` (led by `lead`) beside the mouse at `at`; its area.
pub fn paint(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    text: &mut TextSystem,
    icons: &Icons,
    lead: Lead<'_>,
    name: &str,
    at: Point,
) -> Rect {
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let font = ctx.ui_font(-0.5);
    let shown = ds::ellipsize(text, name, &font, px(NAME_MAX));
    let shaped = text.shape_line(&shown, &font);
    let (h, icon) = (px(HEIGHT), px(14.0));
    let tag = match lead {
        Lead::Tag(tag, color) => Some((text.shape_line(tag, &ctx.ui_font(-1.5)), color)),
        _ => None,
    };
    let lead_w = match (&lead, &tag) {
        (Lead::Icon(..), _) => icon + px(8.0),
        (_, Some((shaped, _))) => shaped.width + px(8.0),
        _ => 0.0,
    };
    let r =
        Rect::new(at.x + px(12.0), at.y + px(6.0), px(10.0) + lead_w + shaped.width + px(12.0), h);
    ds::surface(layer, ctx, r, ds::Elevation::High, ds::radius::MD);
    let x = r.min_x() + px(10.0);
    let base = ds::optical_baseline(r.min_y(), h, &font);
    match (lead, tag) {
        (Lead::Icon(id, color), _) => {
            let ir = Rect::new(x, r.min_y() + (h - icon) / 2.0, icon, icon);
            draw_icon(layer, icons, id, ir, ctx.fade(color));
        }
        (_, Some((shaped, color))) => draw_text(layer, &shaped, x, base, ctx.fade(color)),
        _ => {}
    }
    draw_text(layer, &shaped, x + lead_w, base, ctx.fade(p.text));
    r
}

#[cfg(test)]
#[path = "../tests/unit/drag_card.rs"]
mod tests;
