//! Drawing the data sources' modal: the sources on the left (filtered, by list: the
//! project's, then every project's), the selected one's name, comment and tabs on the
//! right, the footer's test and buttons (or the question before changes are let go), and
//! the open list over all.

use database::{FieldKind, FieldSpec, FieldTab, Scope, SourceColor};
use i18n::{t, tr};
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ViewerPaint;
use ui_components::ds::{self, ButtonKind, ButtonSpec, Check, Ctx, button_size, radius};
use ui_kit::{TextInputStyle, draw_text};

use super::form::{Field, Item, ListRow, Place, Tab, policy_key};
use super::modal::{Choice, Hit, SourcesView, Tool, field_error_text, url_error_text};

mod general;
mod list;
mod parts;
mod tabs;

struct Pen<'a, 'b> {
    layer: &'a mut Layer,
    text: &'a mut TextSystem,
    icons: &'a icons::Icons,
    ds: &'a Ctx<'b>,
}

const PAD: f32 = 24.0;
const LIST_W: f32 = 268.0;
const ROW_H: f32 = 30.0;
const SECTION_H: f32 = 28.0;
const FIELD_H: f32 = 34.0;
/// A labeled field: its label, the field, the gap after it.
const GROUP_H: f32 = 68.0;
const TOOLBAR_H: f32 = 40.0;
/// The list's rows: the chosen one's accent alpha; a drop target's fill and outline (alphas
/// of the accent).
const ROW_CHOSEN: f32 = 0.14;
const DROP_FILL: f32 = 0.10;
const DROP_OUTLINE: f32 = 0.6;

/// A row of the list (a source, a folder, a choice): the chosen one in the accent, the one
/// under the mouse in the hover's background.
fn list_row(selected: bool, hovered: bool) -> ds::RowHighlight {
    ds::RowHighlight::new(selected, hovered)
        .selected_alpha(ROW_CHOSEN)
        .hover(ds::HoverFill::Background)
        .radius(radius::SM)
}

/// Where a dragged row would land: in the accent, outlined.
fn drop_target(p: &ui_components::Palette) -> ds::RowHighlight {
    ds::RowHighlight::new(true, false)
        .selected_alpha(DROP_FILL)
        .outline(p.accent.with_alpha(DROP_OUTLINE))
        .radius(radius::SM)
}

/// A named color's round swatch's side (logical pixels).
const SWATCH: f32 = 20.0;

impl SourcesView {
    fn style(ds: &Ctx<'_>, enabled: bool) -> TextInputStyle {
        let p = ds.palette;
        TextInputStyle {
            font: ds.ui_font(-0.5),
            background: if enabled { p.input_background } else { p.text.with_alpha(0.03) },
            border: p.input_border,
            border_focused: p.accent,
            text: if enabled { p.text } else { p.text_muted },
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: ds.px(radius::MD),
            border_width: ds.line(),
            padding: ds.px(10.0),
            caret_width: ds.px(1.5).max(1.0),
            lead: 0.0,
            trail: 0.0,
        }
    }

    pub(super) fn paint_modal(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.anchors.clear();
        self.bounds = b;
        // The footer (its buttons, the test's outcome) is the window's.
        let body = b;
        if self.editor.is_none() {
            let layer = scene.push_layer(Some(b));
            let font = ds.ui_font(0.0);
            let shaped = text.shape_line(&t("datasources.loading"), &font);
            draw_text(
                layer,
                &shaped,
                b.min_x() + (b.size.width - shaped.width) / 2.0,
                ds::optical_baseline(b.min_y(), b.size.height, &font),
                p.text_muted,
            );
            return;
        }
        // The list, on the left.
        let left = Rect::new(
            b.min_x() + px(PAD),
            body.min_y() + px(16.0),
            px(LIST_W),
            body.size.height - px(16.0),
        );
        let list = {
            let layer = scene.push_layer(Some(b));
            let mut pen = Pen { layer, text, icons, ds: &ds };
            self.paint_list(&mut pen, left)
        };
        {
            let layer = scene.push_layer(Some(list));
            let mut pen = Pen { layer, text, icons, ds: &ds };
            self.paint_rows(&mut pen, list);
        }
        {
            let layer = scene.push_layer(Some(b));
            let x = left.max_x() + px(PAD) - px(1.0);
            ds::divider_vertical(layer, &ds, x, body.min_y(), body.max_y());
        }
        // The selected source, on the right.
        let x0 = left.max_x() + px(2.0 * PAD);
        let width = b.max_x() - px(PAD) - x0;
        let right = Rect::new(x0, body.min_y() + px(16.0), width, body.size.height - px(16.0));
        if self.editor.as_ref().is_some_and(|e| e.current().is_some()) {
            let header_h = px(GROUP_H + 46.0);
            {
                let layer = scene.push_layer(Some(b));
                let mut pen = Pen { layer, text, icons, ds: &ds };
                self.paint_header(&mut pen, Rect::new(x0, right.min_y(), width, header_h));
            }
            let area = Rect::new(
                b.min_x(),
                right.min_y() + header_h,
                b.size.width,
                right.max_y() - right.min_y() - header_h,
            );
            let form = Rect::new(x0 - px(4.0), area.min_y(), width + px(8.0), area.size.height);
            self.form_area = form;
            let before = self.hits.len();
            let layer = scene.push_layer(Some(form));
            let mut pen = Pen { layer, text, icons, ds: &ds };
            let mut y = form.min_y() + px(4.0) - self.form_scroll.round();
            self.paint_tab(&mut pen, x0, width, &mut y);
            let used = y + self.form_scroll.round() - form.min_y();
            self.form_max = (used - form.size.height).max(0.0);
            self.form_scroll = self.form_scroll.min(self.form_max);
            for (r, _) in &mut self.hits[before..] {
                *r = r.intersect(&form).unwrap_or_default();
            }
            let content = self.form_max + form.size.height;
            let offset = self.form_scroll.round();
            self.form_bar.paint(
                scene,
                p,
                scale,
                Rect::new(
                    form.min_x(),
                    form.min_y(),
                    form.size.width + px(PAD) - px(6.0),
                    form.size.height,
                ),
                content,
                offset,
            );
        } else if self.editor.as_ref().is_some_and(|e| e.current_folder().is_some()) {
            let layer = scene.push_layer(Some(b));
            let mut pen = Pen { layer, text, icons, ds: &ds };
            self.paint_folder(&mut pen, x0, right.min_y(), width);
        } else {
            let layer = scene.push_layer(Some(b));
            let font = ds.ui_font(-0.5);
            let shaped = text.shape_line(&t("datasources.none_selected"), &font);
            draw_text(
                layer,
                &shaped,
                x0 + (width - shaped.width) / 2.0,
                ds::optical_baseline(right.min_y(), right.size.height, &font),
                p.text_muted,
            );
        }
        // The open list (the shared picker), over all.
        if let Some(anchor) = self.popup.as_ref().and_then(|c| self.anchors.get(c)).copied() {
            let search = t("datasources.find_option");
            self.picker.paint(scene, text, icons, &ds, (anchor, b), &search);
        }
        // What's dragged follows the mouse, over everything.
        if let Some((icon, color, name)) = self.dragged_card(p) {
            let layer = scene.push_layer(Some(b));
            let lead = ui_components::drag_card::Lead::Icon(icons.id_or_file(icon), color);
            ui_components::drag_card::paint(layer, &ds, text, icons, lead, &name, self.drag_at);
        }
    }

    /// The card of what's dragged: its icon, the icon's color and its name.
    pub(super) fn dragged_card(
        &self,
        p: &ui_components::Palette,
    ) -> Option<(&'static str, Rgba, String)> {
        let editor = self.editor.as_ref()?;
        let named = |name: &str| {
            if name.is_empty() { t("datasources.unnamed") } else { name.to_owned() }
        };
        match self.dragging? {
            Item::Source(id) => {
                let e = editor.entries.iter().find(|e| e.source.id == id)?;
                let (icon, tone) = editor
                    .driver(&e.source.driver)
                    .map_or(("database", database::IconTone::Neutral), |d| (d.icon, d.tone));
                let color = crate::engine_icon::color(icon, tone, p, p.accent);
                Some((icon, color, named(&e.source.name)))
            }
            Item::Folder(id) => {
                let folder = editor.folder(id)?;
                let color = folder
                    .color
                    .and_then(|c| ui_components::tag_colors::named(&c.name(), p))
                    .unwrap_or(p.folder_icon);
                Some(("folder", color, named(&folder.name)))
            }
        }
    }
}

/// The dot after a source's name while its changes aren't applied (logical pixels).
const CHANGED_DOT: f32 = 6.0;
