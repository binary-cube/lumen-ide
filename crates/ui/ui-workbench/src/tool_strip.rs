//! The vertical tool strip on the left: an icon button
//! for each panel (project, terminal…). The top and bottom buttons are
//! grouped separately; the active one is highlighted with the theme accent. A button may
//! carry a count (what its window lists: files to upload, files changed) in its corner.
//! Between neighbors of different groups (what they're about: code, services, documents)
//! a short line that fades toward its ends, with a little more room around it.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use ui_kit::draw_icon;

use crate::palette::Palette;

/// The strip width, in logical pixels.
const WIDTH: f32 = 44.0;

/// The strip's appearance; built from the theme or given individually.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolStripStyle {
    pub icon: Rgba,
    pub hover: Rgba,
    pub active_background: Rgba,
    pub active_icon: Rgba,
    /// The side of the button and of the icon, in logical pixels.
    pub button_size: f32,
    pub icon_size: f32,
}

impl ToolStripStyle {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            icon: p.tool_strip_icon,
            hover: p.tool_strip_hover,
            active_background: p.tool_strip_active_background,
            active_icon: p.tool_strip_active_icon,
            button_size: 32.0,
            icon_size: 18.0,
        }
    }
}

impl Default for ToolStripStyle {
    fn default() -> Self {
        Self::from_palette(&Palette::dark())
    }
}

/// A strip button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolButton {
    /// The identifier returned on click (e.g. `"project"`).
    pub id: &'static str,
    pub icon: IconId,
    pub active: bool,
    /// The bottom group (e.g. the terminal).
    pub bottom: bool,
    /// A dot at its icon's corner (something to see: its count in the tooltip), and what it
    /// means (see `ds::dot_badge`).
    pub badge: Option<crate::ds::BadgeKind>,
    /// Its group (`ToolWindowProvider::group`): a line between neighbors of different ones.
    pub group: &'static str,
}

#[derive(Debug, Default)]
pub struct ToolStrip {
    style: ToolStripStyle,
    bounds: Rect,
    scale: f32,
    rects: Vec<(Rect, &'static str)>,
    hover: Option<&'static str>,
}

impl ToolStrip {
    pub fn new(style: ToolStripStyle) -> Self {
        Self { style, ..Self::default() }
    }

    pub fn width(scale: f32) -> f32 {
        (WIDTH * scale).round()
    }

    pub fn set_style(&mut self, style: ToolStripStyle) {
        self.style = style;
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The button under `p` and its area (for the tooltip).
    pub fn hit_rect(&self, p: Point) -> Option<(Rect, &'static str)> {
        self.rects.iter().find(|(r, _)| r.contains(p)).copied()
    }

    pub fn hit(&self, p: Point) -> Option<&'static str> {
        self.rects.iter().find(|(r, _)| r.contains(p)).map(|(_, id)| *id)
    }

    /// `true` if the button under the cursor changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn paint(
        &mut self,
        buttons: &[ToolButton],
        icons: &Icons,
        palette: &Palette,
        scene: &mut Scene,
    ) {
        let b = self.bounds;
        self.rects.clear();
        if b.size.width <= 0.0 {
            return;
        }
        let px = |v: f32| (v * self.scale).round();
        let st = &self.style;
        let size = px(st.button_size);
        let icon = px(st.icon_size);
        let gap = px(6.0);
        let x = (b.min_x() + (b.size.width - size) / 2.0).round();
        let layer = scene.push_layer(Some(b));

        let top: Vec<_> = buttons.iter().filter(|b| !b.bottom).collect();
        let bottom: Vec<_> = buttons.iter().filter(|b| b.bottom).collect();
        let mut place = Vec::with_capacity(buttons.len());
        // A group's line: the room it adds between two buttons, and where it's drawn — none
        // when the buttons wouldn't fit with it (a short window: they'd overlap sooner).
        let breaks =
            |side: &[&ToolButton]| side.windows(2).filter(|w| w[0].group != w[1].group).count();
        let count = (top.len() + bottom.len()) as f32;
        let plain = px(16.0) + count * size + (count - 2.0).max(0.0) * gap;
        let room = px(10.0);
        let room_all = (breaks(&top) + breaks(&bottom)) as f32 * room;
        let lined = plain + room_all <= b.size.height;
        let mut lines = Vec::new();
        let mut y = b.min_y() + px(8.0);
        let mut before: Option<&str> = None;
        for button in top {
            if lined && before.is_some_and(|g| g != button.group) {
                lines.push((y - gap + (gap + room) / 2.0).round());
                y += room;
            }
            before = Some(button.group);
            place.push((Rect::new(x, y, size, size), button));
            y += size + gap;
        }
        // From the corner up: each above the one before it.
        let mut y = b.max_y() - px(8.0) - size;
        let mut below: Option<&str> = None;
        for button in bottom.into_iter().rev() {
            if lined && below.is_some_and(|g| g != button.group) {
                y -= room;
                lines.push((y + size + (gap + room) / 2.0).round());
            }
            below = Some(button.group);
            place.push((Rect::new(x, y, size, size), button));
            y -= size + gap;
        }
        if !lines.is_empty() {
            let ctx = crate::ds::Ctx::new(palette, self.scale);
            let (x0, x1) = (b.min_x() + px(10.0), b.max_x() - px(10.0));
            for line in lines {
                crate::ds::divider_horizontal_faded(layer, &ctx, x0, x1, line);
            }
        }

        for (rect, button) in place {
            let color = if button.active {
                layer.quad(Quad::filled(rect, st.active_background).rounded(px(8.0)));
                st.active_icon
            } else {
                if self.hover == Some(button.id) {
                    layer.quad(Quad::filled(rect, st.hover).rounded(px(8.0)));
                }
                st.icon
            };
            let r = Rect::new(
                rect.min_x() + (size - icon) / 2.0,
                (rect.min_y() + (size - icon) / 2.0).round(),
                icon,
                icon,
            );
            draw_icon(layer, icons, button.icon, r, color);
            if let Some(kind) = button.badge {
                let ctx = crate::ds::Ctx::new(palette, self.scale);
                // At the icon's top right corner, cut out of what's behind it there: the lit
                // button's own color, the hover's tint over the strip, else the strip.
                // Inside the button, ring and all (nothing of it over the strip beside).
                let (d, ring) = (px(crate::ds::DOT_BADGE), px(2.0).max(1.0));
                let corner = Point::new(
                    (r.max_x() - px(1.0)).min(rect.max_x() - ring - d / 2.0),
                    (r.min_y() + px(1.0)).max(rect.min_y() + ring + d / 2.0),
                );
                let strip = palette.window_background;
                let outline = if button.active {
                    st.active_background
                } else if self.hover == Some(button.id) {
                    strip.mix(st.hover.with_alpha(1.0), st.hover.a)
                } else {
                    strip
                };
                let badge = crate::ds::DotBadge { kind, lit: button.active, outline };
                crate::ds::dot_badge(layer, &ctx, corner, badge);
            }
            self.rects.push((rect, button.id));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/tool_strip.rs"]
mod tests;
