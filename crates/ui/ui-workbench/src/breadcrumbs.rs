//! The navigation bar below the tabs: `class.php › Car › stop()`. Each segment is clickable
//! (jumps to the declaration); the last one, the declaration at the cursor, is highlighted.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::Scene;
use text_layout::TextSystem;
use ui_kit::draw_text;

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

/// A crumb's icon (logical pixels).
const CRUMB_ICON: f32 = 14.0;

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
        let font = ds::button_label_font(&ctx);
        let sep = text.shape_line("›", &font);
        let pill_h = px(ds::button_size::SMALL);
        let top = b.min_y() + ((b.size.height - ctx.line() - pill_h) / 2.0).round();
        let mut x = b.min_x() + px(12.0);
        let last = crumbs.len() - 1;
        for (i, crumb) in crumbs.iter().enumerate() {
            let current = i == last;
            let icon_w = if crumb.icon.is_some() { px(CRUMB_ICON) + px(6.0) } else { 0.0 };
            // A long name cut in the middle (its extension kept), at most a crumb's width, as
            // much as is left for the last one; too little left: nothing more.
            let room = (b.max_x() - px(8.0) - x - px(16.0) - icon_w).min(px(crate::ds::NAME_MAX));
            if room < px(40.0) {
                break;
            }
            let label = ds::ellipsize_file(text, &crumb.label, &font, room);
            let mut spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .label(&label)
                .icon_size(CRUMB_ICON)
                .hovered(self.hover == Some(i));
            spec = if current {
                spec.tone(ds::Tone::Accent)
            } else {
                spec.rest_tone(ds::Tone::Neutral)
            };
            if let Some((id, tint)) = crumb.icon {
                spec = spec.icon_id(id).icon_color(tint);
            }
            let w = ds::button_width(text, &ctx, &spec, pill_h);
            let r = Rect::new(x, top, w, pill_h);
            ds::button(layer, text, icons, &ctx, r, spec);
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
