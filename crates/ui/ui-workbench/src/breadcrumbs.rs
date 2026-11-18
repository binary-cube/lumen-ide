//! The navigation bar below the tabs: `class.php › Car › stop()`. Each segment is clickable
//! (jumps to the declaration); the last one, the declaration at the cursor, is highlighted.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::TextSystem;
use ui_kit::{draw_icon, draw_text};

use crate::ds::{self, Ctx};
use crate::palette::Palette;

/// A segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Crumb {
    pub label: String,
    /// The icon and its color (e.g. the file type's).
    pub icon: Option<(IconId, Rgba)>,
}

#[derive(Debug, Default)]
pub struct Breadcrumbs {
    bounds: Rect,
    scale: f32,
    segments: Vec<Rect>,
    hover: Option<usize>,
}

impl Breadcrumbs {
    /// Bar height (physical pixels).
    pub fn height(scale: f32) -> f32 {
        (30.0 * scale).round()
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The segment at `p`.
    pub fn hit(&self, p: Point) -> Option<usize> {
        if !self.bounds.contains(p) {
            return None;
        }
        self.segments.iter().position(|r| r.contains(p))
    }

    /// `true` if the segment under the mouse changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn clear_hover(&mut self) -> bool {
        self.hover.take().is_some()
    }

    pub fn paint(
        &mut self,
        crumbs: &[Crumb],
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.segments.clear();
        let b = self.bounds;
        if b.size.height <= 0.0 || crumbs.is_empty() {
            return;
        }
        let ctx = Ctx::new(palette, self.scale);
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(b));
        ds::divider(layer, &ctx, b.min_x(), b.max_x(), b.max_y() - ctx.line());
        let font = ctx.ui_font(-1.0);
        let strong = font.clone().with_weight(500);
        let sep = text.shape_line("›", &font);
        let pill_h = px(22.0);
        let top = b.min_y() + ((b.size.height - ctx.line() - pill_h) / 2.0).round();
        let mut x = b.min_x() + px(12.0);
        let last = crumbs.len() - 1;
        for (i, crumb) in crumbs.iter().enumerate() {
            let current = i == last;
            let f = if current { &strong } else { &font };
            let icon_w = if crumb.icon.is_some() { px(14.0) + px(6.0) } else { 0.0 };
            // A long name cut in the middle (its extension kept), at most a crumb's width, as
            // much as is left for the last one; too little left: nothing more.
            let room = (b.max_x() - px(8.0) - x - px(12.0) - icon_w).min(px(crate::ds::NAME_MAX));
            if room < px(40.0) {
                break;
            }
            let label = ds::ellipsize_file(text, &crumb.label, f, room);
            let shaped = text.shape_line(&label, f);
            let w = px(6.0) + icon_w + shaped.width + px(6.0);
            let r = Rect::new(x, top, w, pill_h);
            if self.hover == Some(i) {
                layer.quad(
                    Quad::filled(r, palette.text.with_alpha(0.055)).rounded(px(ds::radius::SM)),
                );
            }
            let color = if current { palette.accent } else { palette.text_muted };
            let mut tx = r.min_x() + px(6.0);
            if let Some((id, tint)) = crumb.icon {
                let s = px(14.0);
                draw_icon(layer, icons, id, Rect::new(tx, top + (pill_h - s) / 2.0, s, s), tint);
                tx += s + px(6.0);
            }
            draw_text(layer, &shaped, tx, ds::optical_baseline(top, pill_h, f), color);
            self.segments.push(r);
            x = r.max_x();
            if !current {
                x += px(3.0);
                let base = ds::optical_baseline(top, pill_h, &font);
                draw_text(layer, &sep, x, base, palette.text_muted.with_alpha(0.6));
                x += sep.width + px(3.0);
            }
        }
    }
}
