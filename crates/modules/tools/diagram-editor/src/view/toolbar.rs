//! The bar over the sheet: the palette shown or not; undo, redo; delete; to the front, to the
//! back; the zoom (−, its value — a click fits the page —, +); the properties shown or not.
//! Each button greyed out where it can't act (nothing chosen, nothing to undo, locked).

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::Viewer;
use ui_components::ds::{self, Ctx};
use ui_kit::draw_text;

use super::DiagramView;

/// The bar's height (logical pixels).
pub(super) const HEIGHT: f32 = 36.0;

/// A button of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tool {
    Palette,
    Select,
    DrawRect,
    DrawEllipse,
    DrawRhombus,
    DrawText,
    DrawLine,
    Undo,
    Redo,
    Delete,
    Front,
    Back,
    ZoomOut,
    Fit,
    ZoomIn,
    Properties,
}

impl Tool {
    fn icon(self) -> &'static str {
        match self {
            Self::Palette => "palette",
            Self::Select => "tool-select",
            Self::DrawRect => "square",
            Self::DrawEllipse => "tool-ellipse",
            Self::DrawRhombus => "tool-rhombus",
            Self::DrawText => "tool-text",
            Self::DrawLine => "tool-line",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Delete => "trash",
            Self::Front => "arrow-up",
            Self::Back => "arrow-down",
            Self::ZoomOut => "minus",
            Self::Fit => "maximize-2",
            Self::ZoomIn => "plus",
            Self::Properties => "panel-right",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Self::Palette => "diagram_editor.tool_palette",
            Self::Select => "diagram_editor.tool_select",
            Self::DrawRect => "diagram_editor.tool_rect",
            Self::DrawEllipse => "diagram_editor.tool_ellipse",
            Self::DrawRhombus => "diagram_editor.tool_rhombus",
            Self::DrawText => "diagram_editor.tool_text",
            Self::DrawLine => "diagram_editor.tool_line",
            Self::Undo => "diagram_editor.tool_undo",
            Self::Redo => "diagram_editor.tool_redo",
            Self::Delete => "diagram_editor.delete",
            Self::Front => "diagram_editor.to_front",
            Self::Back => "diagram_editor.to_back",
            Self::ZoomOut => "diagram_editor.tool_zoom_out",
            Self::Fit => "diagram_editor.tool_fit",
            Self::ZoomIn => "diagram_editor.tool_zoom_in",
            Self::Properties => "diagram_editor.tool_properties",
        }
    }
}

impl Tool {
    /// The drawing tool it chooses.
    fn draw(self) -> Option<super::draw::Draw> {
        use super::draw::Draw;
        Some(match self {
            Self::Select => Draw::Select,
            Self::DrawRect => Draw::Rect,
            Self::DrawEllipse => Draw::Ellipse,
            Self::DrawRhombus => Draw::Rhombus,
            Self::DrawText => Draw::Text,
            Self::DrawLine => Draw::Line,
            _ => return None,
        })
    }
}

/// The groups of the bar, in order (a gap between groups).
const GROUPS: [&[Tool]; 6] = [
    &[Tool::Palette],
    &[
        Tool::Select,
        Tool::DrawRect,
        Tool::DrawEllipse,
        Tool::DrawRhombus,
        Tool::DrawText,
        Tool::DrawLine,
    ],
    &[Tool::Undo, Tool::Redo],
    &[Tool::Delete, Tool::Front, Tool::Back],
    &[Tool::ZoomOut, Tool::Fit, Tool::ZoomIn],
    &[Tool::Properties],
];

impl DiagramView {
    /// The bar's area (the view's top).
    pub(super) fn toolbar_area(&self) -> Rect {
        Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.bounds.size.width,
            HEIGHT * self.scale,
        )
    }

    /// Whether `tool` can act now.
    fn tool_enabled(&self, tool: Tool) -> bool {
        let editable = !self.locked && self.diagram.is_some();
        let chosen = !self.selection.is_empty();
        let history = self.diagram.as_ref().map(|d| &d.history);
        match tool {
            Tool::Undo => editable && history.is_some_and(diagram_model::History::can_undo),
            Tool::Redo => editable && history.is_some_and(diagram_model::History::can_redo),
            Tool::Delete | Tool::Front | Tool::Back => editable && chosen,
            Tool::ZoomOut | Tool::Fit | Tool::ZoomIn => self.diagram.is_some(),
            Tool::Palette | Tool::Properties | Tool::Select => true,
            Tool::DrawRect
            | Tool::DrawEllipse
            | Tool::DrawRhombus
            | Tool::DrawText
            | Tool::DrawLine => editable,
        }
    }

    /// A press on the bar (`clicks`: a double click keeps a drawing tool). `true`: it was on
    /// it.
    pub(super) fn press_toolbar(&mut self, p: Point, clicks: u32) -> bool {
        if !self.toolbar_area().contains(p) {
            return false;
        }
        let Some(&(_, tool)) = self.tools.iter().find(|(r, _)| r.contains(p)) else { return true };
        if !self.tool_enabled(tool) {
            return true;
        }
        if let Some(draw) = tool.draw() {
            self.choose_draw(draw, clicks >= 2);
            return true;
        }
        let center = self.sheet().center();
        match tool {
            Tool::Palette => self.show_palette = !self.show_palette,
            Tool::Properties => self.show_properties = !self.show_properties,
            Tool::Undo => {
                self.command("undo");
            }
            Tool::Redo => {
                self.command("redo");
            }
            Tool::Delete => {
                self.delete_chosen();
            }
            Tool::Front => {
                self.command("to-front");
            }
            Tool::Back => {
                self.command("to-back");
            }
            Tool::ZoomOut => {
                self.camera.zoom_at(1.0 / 1.25, center, self.sheet(), self.scale);
            }
            Tool::ZoomIn => {
                self.camera.zoom_at(1.25, center, self.sheet(), self.scale);
            }
            Tool::Fit => self.fitted = false,
            Tool::Select
            | Tool::DrawRect
            | Tool::DrawEllipse
            | Tool::DrawRhombus
            | Tool::DrawText
            | Tool::DrawLine => {}
        }
        true
    }

    /// A button's tooltip.
    pub(super) fn toolbar_tip(&self, p: Point) -> Option<(Rect, String)> {
        self.tools.iter().find(|(r, _)| r.contains(p)).map(|(r, tool)| (*r, t(tool.tip())))
    }

    /// The bar drawn.
    pub(super) fn paint_toolbar(&mut self, ctx: &mut ui_components::ViewerPaint<'_>) {
        let area = self.toolbar_area();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let layer = ctx.scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.tab_bar_background));
        let line = ctx.scale.max(1.0);
        layer.quad(Quad::filled(
            Rect::new(area.min_x(), area.max_y() - line, area.size.width, line),
            p.border,
        ));
        self.tools.clear();
        let size = ds.px(28.0);
        let mut x = area.min_x() + ds.px(8.0);
        let y = (area.center().y - size / 2.0).round();
        for (g, group) in GROUPS.iter().enumerate() {
            // The properties' button at the bar's right end.
            if g == GROUPS.len() - 1 {
                x = area.max_x() - ds.px(8.0) - size;
            }
            for &tool in *group {
                let r = Rect::new(x, y, size, size);
                let on = match tool {
                    Tool::Palette => self.show_palette,
                    Tool::Properties => self.show_properties,
                    _ => tool.draw().is_some_and(|d| d == self.draw),
                };
                if on {
                    layer.quad(Quad::filled(r, p.tab_active_background).rounded(ds.px(6.0)));
                }
                let color =
                    if self.tool_enabled(tool) { p.text } else { p.text_muted.with_alpha(0.45) };
                if let Some(icon) = ctx.icons.id(tool.icon()) {
                    let inner = Rect::new(
                        r.min_x() + ds.px(6.0),
                        r.min_y() + ds.px(6.0),
                        size - ds.px(12.0),
                        size - ds.px(12.0),
                    );
                    ui_kit::draw_icon(layer, ctx.icons, icon, inner, color);
                }
                self.tools.push((r, tool));
                x = r.max_x() + ds.px(2.0);
                // The zoom's value after −.
                if tool == Tool::ZoomOut {
                    let font = ds.ui_font(-1.0);
                    let label = format!("{}%", (self.camera.zoom * 100.0).round());
                    let shaped = ctx.text.shape_line(&label, &font);
                    let w = shaped.width.max(ds.px(36.0));
                    draw_text(
                        layer,
                        &shaped,
                        (x + (w - shaped.width) / 2.0).round(),
                        ds::optical_baseline(r.min_y(), r.size.height, &font),
                        p.text,
                    );
                    x += w + ds.px(2.0);
                }
            }
            x += ds.px(10.0);
        }
    }
}

#[cfg(test)]
impl DiagramView {
    /// Where `tool`'s button is (its middle).
    pub(super) fn tool_for_tests(&self, tool: Tool) -> Option<Point> {
        self.tools.iter().find(|(_, t)| *t == tool).map(|(r, _)| r.center())
    }
}
