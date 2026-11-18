//! Image viewer toolbar: a floating pill at the top, with groups of
//! buttons (zoom, size, rotate / flip, background, save) and a tooltip on hover.

use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::Painter;
use ui_kit::{centered_baseline, draw_icon, draw_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    ZoomOut,
    ZoomIn,
    Fit,
    Actual,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Background,
    Save,
}

/// Button groups, with icon, description (a locale key) and command (its keys, as the user
/// has them, shown in the tooltip).
const GROUPS: &[&[(Tool, &str, &str, &str)]] = &[
    &[(Tool::ZoomOut, "img-zoom-out", "image.zoom_out", "image.zoom_out")],
    // Between them: the zoom percentage.
    &[(Tool::ZoomIn, "img-zoom-in", "image.zoom_in", "image.zoom_in")],
    &[
        (Tool::Fit, "img-fit", "image.fit", "image.fit"),
        (Tool::Actual, "img-actual", "image.actual", "image.actual"),
    ],
    &[
        (Tool::RotateLeft, "img-rotate-ccw", "image.rotate_left", "image.rotate_left"),
        (Tool::RotateRight, "img-rotate-cw", "image.rotate_right", "image.rotate_right"),
        (Tool::FlipHorizontal, "img-flip-h", "image.flip_h", "image.flip_horizontal"),
        (Tool::FlipVertical, "img-flip-v", "image.flip_v", "image.flip_vertical"),
    ],
    &[(Tool::Background, "img-background", "image.background", "image.background")],
    &[(Tool::Save, "img-save", "image.save", "file.save")],
];

#[derive(Debug, Default)]
pub struct Toolbar {
    buttons: Vec<(Rect, Tool, &'static str, &'static str, bool)>,
    hover: Option<Tool>,
}

impl Toolbar {
    pub fn hit(&self, p: Point) -> Option<Tool> {
        self.buttons.iter().find(|(r, _, _, _, enabled)| *enabled && r.contains(p)).map(|b| b.1)
    }

    /// Tooltip of the button under the mouse.
    pub fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        let (rect, _, label, command, _) = self.buttons.iter().find(|b| b.0.contains(p))?;
        let keys = ui_components::shortcuts::label(command);
        Some(ui_components::Tip::new(*rect, i18n::t(label)).with_shortcut(keys))
    }

    /// `true` if the mouse is over an active button (the cursor becomes a hand).
    /// The button under `p` lit: `true` if that changed (a redraw).
    pub fn hover(&mut self, p: Point) -> bool {
        let now = self.buttons.iter().find(|(r, ..)| r.contains(p)).map(|b| b.1);
        std::mem::replace(&mut self.hover, now) != now
    }

    pub fn paint(&mut self, bounds: Rect, zoom: u32, can_save: bool, s: f32, painter: Painter<'_>) {
        let Painter { text, scene, palette, icons } = painter;
        let px = |v: f32| (v * s).round();
        let (bw, bh, pad, gap) = (px(30.0), px(28.0), px(4.0), px(10.0));
        let font = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 1.0));
        let metrics = text.metrics(&font);
        let label = text.shape_line(&format!("{zoom}%"), &font);
        let label_w = px(46.0).max(label.width + px(10.0));
        let count: usize = GROUPS.iter().map(|g| g.len()).sum();
        // Separators: between groups, except between zoom − / percent / zoom +.
        let separators = GROUPS.len() - 2;
        let width = count as f32 * bw + label_w + separators as f32 * (gap + px(1.0)) + 2.0 * pad;
        let bar = Rect::new(
            (bounds.min_x() + (bounds.size.width - width) / 2.0).round(),
            bounds.min_y() + px(12.0),
            width.round(),
            bh + 2.0 * pad,
        );
        let line = s.round().max(1.0);
        let layer = scene.push_layer(Some(bounds));
        let ctx = ui_components::ds::Ctx::new(palette, s);
        ui_components::ds::surface(layer, &ctx, bar, ui_components::ds::Elevation::Low, 10.0);
        self.buttons.clear();
        let mut x = bar.min_x() + pad;
        let top = bar.min_y() + pad;
        for (g, group) in GROUPS.iter().enumerate() {
            if g == 1 {
                // Zoom percentage, between − and +.
                let lx = (x + (label_w - label.width) / 2.0).round();
                draw_text(layer, &label, lx, centered_baseline(top, bh, &metrics), palette.text);
                x += label_w;
            } else if g > 1 {
                x += gap / 2.0;
                layer.quad(Quad::filled(
                    Rect::new(x, top + px(6.0), line, bh - px(12.0)),
                    palette.border,
                ));
                x += gap / 2.0 + px(1.0);
            }
            for &(tool, icon, label, command) in *group {
                let r = Rect::new(x, top, bw, bh);
                let enabled = tool != Tool::Save || can_save;
                let hovered = self.hover == Some(tool);
                if hovered && enabled {
                    layer.quad(Quad::filled(r, palette.tab_hover).rounded(px(7.0)));
                }
                let color = match (tool, enabled) {
                    (Tool::Save, true) => palette.accent,
                    (_, true) => palette.text,
                    (_, false) => palette.text_muted.with_alpha(0.45),
                };
                if let Some(id) = icons.id(icon) {
                    let size = px(16.0);
                    let ir = Rect::new(
                        r.min_x() + (bw - size) / 2.0,
                        r.min_y() + (bh - size) / 2.0,
                        size,
                        size,
                    );
                    draw_icon(layer, icons, id, ir, color);
                }
                self.buttons.push((r, tool, label, command, enabled));
                x += bw;
            }
        }
    }
}
