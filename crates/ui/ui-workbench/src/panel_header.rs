//! A side panel header: the title on the left and icon buttons on the right, grouped
//! (with a vertical separator between groups). A panel at the bottom (Services) has the
//! bottom windows' header instead ([`PanelHeader::set_bottom`]): lower, its icon in the
//! accent, a line under it — as the terminal's.

use icons::IconId;
use kernel::{Point, Rect};
use text_layout::Font;
use ui_kit::{centered_baseline, draw_icon, draw_text};

use crate::{Painter, ds};

/// Header height, in logical pixels.
const HEIGHT: f32 = 44.0;
/// A bottom panel's header height (the bottom windows').
const BOTTOM_HEIGHT: f32 = 34.0;
/// A bottom window's header buttons (logical pixels): smaller, as its header is.
const BOTTOM_BUTTON: f32 = 24.0;
/// What a title keeps at least (logical pixels): shorter, it's cut with `…`; the panel
/// isn't dragged narrower than its buttons and this.
const TITLE_MIN: f32 = 56.0;

/// A header button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelAction {
    /// The identifier returned on click.
    pub id: &'static str,
    pub icon: IconId,
    /// In effect (a bottom panel's half height): lit in the accent.
    pub lit: bool,
}

#[derive(Debug, Default)]
pub struct PanelHeader {
    bounds: Rect,
    scale: f32,
    rects: Vec<(Rect, &'static str)>,
    hover: Option<&'static str>,
    /// Drawn as the bottom windows' header.
    bottom: bool,
    /// The width its icon, a short title and its buttons need (logical, from the last paint).
    needs: f32,
}

impl PanelHeader {
    pub fn height(scale: f32) -> f32 {
        (HEIGHT * scale).round()
    }

    /// A bottom panel's header height (the bottom windows').
    pub fn bottom_height(scale: f32) -> f32 {
        (BOTTOM_HEIGHT * scale).round()
    }

    /// Drawn as the bottom windows' header (lower, the icon in the accent, a line under it).
    pub fn set_bottom(&mut self, bottom: bool) {
        self.bottom = bottom;
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    /// The least width the panel may have: its icon, a short title and its buttons side by
    /// side (logical pixels, as last drawn; 0 before).
    pub fn min_width(&self) -> f32 {
        self.needs
    }

    pub fn contains(&self, p: Point) -> bool {
        self.bounds.contains(p)
    }

    /// The action under `p` and its area (for the tooltip).
    pub fn hit_rect(&self, p: Point) -> Option<(Rect, &'static str)> {
        self.rects.iter().find(|(r, _)| r.contains(p)).copied()
    }

    /// Where button `id` was drawn (the last paint).
    pub fn bounds_of(&self, id: &str) -> Option<Rect> {
        self.rects.iter().find(|(_, i)| *i == id).map(|(r, _)| *r)
    }

    pub fn hit(&self, p: Point) -> Option<&'static str> {
        self.rects.iter().find(|(r, _)| r.contains(p)).map(|(_, id)| *id)
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// `icon`: the icon before the title; `groups`: the button groups, left to right.
    pub fn paint(
        &mut self,
        icon: Option<IconId>,
        title: &str,
        groups: &[&[PanelAction]],
        painter: Painter<'_>,
    ) {
        let Painter { text, scene, palette, icons } = painter;
        let b = self.bounds;
        self.rects.clear();
        let px = |v: f32| (v * self.scale).round();
        let line = self.scale.round().max(1.0);
        let layer = scene.push_layer(Some(b));

        let size_delta = if self.bottom { -0.5 } else { 0.0 };
        let font = Font::new(palette.ui_font.clone(), px(palette.ui_font_size + size_delta))
            .with_weight(Font::SEMIBOLD);
        let baseline = if self.bottom {
            // As the bottom windows' titles sit.
            (b.min_y() + (b.size.height + font.size * 0.72) / 2.0).round()
        } else {
            let metrics = text.metrics(&font);
            centered_baseline(b.min_y(), b.size.height, &metrics)
        };
        let button = px(if self.bottom { BOTTOM_BUTTON } else { ds::button_size::MEDIUM });
        let ctx = ds::Ctx::new(palette, self.scale);
        let gap = px(2.0);
        let top = (b.min_y() + (b.size.height - button) / 2.0).round();
        let mut x = b.max_x() - px(10.0);
        for (g, group) in groups.iter().enumerate().rev() {
            for action in group.iter().rev() {
                x -= button;
                let rect = Rect::new(x, top, button, button);
                let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                    .icon(icons.name(action.icon).unwrap_or_default())
                    .rest_tone(ds::Tone::Neutral)
                    .on(action.lit)
                    .hovered(self.hover == Some(action.id));
                ds::button(layer, text, icons, &ctx, rect, spec);
                self.rects.push((rect, action.id));
                x -= gap;
            }
            if g > 0 {
                x -= px(5.0);
                let h = px(16.0);
                let y = (b.min_y() + (b.size.height - h) / 2.0).round();
                ds::divider_vertical(layer, &ctx, x, y, y + h);
                x -= px(5.0);
            }
        }
        // The title in what the buttons leave (up to a little before the first of them),
        // cut with `…` when it doesn't fit.
        let buttons_left = x + gap;
        let mut x = b.min_x() + px(14.0);
        if let Some(id) = icon {
            let (size, color) = if self.bottom {
                (px(15.0), palette.accent)
            } else {
                (px(16.0), palette.text_muted)
            };
            let r = Rect::new(x, (b.min_y() + (b.size.height - size) / 2.0).round(), size, size);
            draw_icon(layer, icons, id, r, color);
            x += size + px(8.0);
        }
        let room = (buttons_left - px(4.0) - x).max(0.0);
        let shown = crate::ds::ellipsize(text, title, &font, room);
        let shaped = text.shape_line(&shown, &font);
        // Not even its `…`: nothing (never under the buttons).
        if shaped.width <= room {
            draw_text(layer, &shaped, x, baseline, palette.text);
        }
        // A title shorter than the least kept needs only its own width: a panel whose title
        // fits is never made wider.
        let icon_w = x - b.min_x();
        let buttons_w = b.max_x() - buttons_left;
        let title_w = text.shape_line(title, &font).width.min(px(TITLE_MIN));
        self.needs = ((icon_w + title_w + px(4.0) + buttons_w) / self.scale.max(0.1)).ceil();
        if self.bottom {
            ds::divider_card(layer, &ctx, b.min_x(), b.max_x(), b.max_y() - line);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/panel_header.rs"]
mod tests;
