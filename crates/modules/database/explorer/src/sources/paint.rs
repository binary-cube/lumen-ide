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

    // ─── The list ───

    /// The filter and the toolbar; the rows' area between them.
    fn paint_list(&mut self, pen: &mut Pen<'_, '_>, r: Rect) -> Rect {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        // The filter: the app's search field (× with text, the caret while it has the keys).
        let field = Rect::new(r.min_x(), r.min_y(), r.size.width, px(FIELD_H));
        let focused = self.focus == Some(Field::Filter);
        let Some(editor) = &mut self.editor else { return Rect::default() };
        let placeholder = t("datasources.filter_sources");
        editor.filter.paint(pen.layer, pen.text, pen.icons, ds, (field, focused), &placeholder);
        let Some(editor) = &self.editor else { return Rect::default() };
        self.hits.push((field, Hit::Field(Field::Filter)));
        // The rows, by list.
        let toolbar = Rect::new(r.min_x(), r.max_y() - px(TOOLBAR_H), r.size.width, px(TOOLBAR_H));
        let list = Rect::new(
            r.min_x(),
            field.max_y() + px(10.0),
            r.size.width,
            toolbar.min_y() - field.max_y() - px(14.0),
        );
        self.list_area = list;
        // The toolbar: add, remove, duplicate, move to the other list.
        ds::divider_card(pen.layer, ds, r.min_x(), r.max_x(), toolbar.min_y());
        let has = editor.current().is_some();
        let folder = editor.current_folder().is_some();
        let move_label = match editor.scope() {
            Some(Scope::Global) => t("datasources.tool_to_project"),
            _ => t("datasources.tool_to_global"),
        };
        let tools = [
            (Tool::Add, "plus", t("datasources.tool_add"), true),
            (Tool::Folder, "folder-plus", t("datasources.tool_folder"), true),
            (Tool::Remove, "minus", t("datasources.tool_remove"), has || folder),
            (Tool::Duplicate, "copy", t("datasources.tool_duplicate"), has),
            (
                Tool::Move,
                if editor.scope() == Some(Scope::Global) { "folder" } else { "globe" },
                move_label,
                has,
            ),
        ];
        let size = px(button_size::LARGE);
        let mut x = r.min_x();
        let mid = toolbar.min_y() + px(6.0) + size / 2.0;
        for (tool, icon, tip, enabled) in tools {
            let button = Rect::new(x, (mid - size / 2.0).round(), size, size);
            let hovered = self.hover == Some(Hit::Tool(tool))
                || (tool == Tool::Add && self.popup == Some(Choice::AddDriver));
            let spec =
                ButtonSpec::new(ButtonKind::Ghost).icon(icon).enabled(enabled).hovered(hovered);
            ds::button(pen.layer, pen.text, pen.icons, ds, button, spec);
            if enabled {
                self.hits.push((button, Hit::Tool(tool)));
            }
            self.tips.push((button, tip));
            if tool == Tool::Add {
                self.anchors.insert(Choice::AddDriver, button);
            }
            x += size + px(4.0);
        }
        list
    }

    /// The rows, by list (the project's, then every project's) and folder, scrolled in
    /// `list`; where a drag would drop outlined.
    fn paint_rows(&mut self, pen: &mut Pen<'_, '_>, list: Rect) {
        let ds = pen.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let rows = editor.list_rows();
        let height: f32 = rows
            .iter()
            .map(|r| if matches!(r, ListRow::Section(_)) { px(SECTION_H) } else { px(ROW_H) })
            .sum();
        self.list_max = (height - list.size.height).max(0.0);
        self.list_scroll = self.list_scroll.min(self.list_max);
        let font = ds.ui_font(-0.5);
        let small = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let detail_font = ds.ui_font(-1.5);
        let indent = px(16.0);
        // The trailing column (a source's driver, a folder's count): as wide as its widest,
        // each right-aligned in it, so the production badges before it line up.
        let trailing = rows
            .iter()
            .filter_map(|r| match r {
                ListRow::Folder { count, .. } => Some(count.to_string()),
                ListRow::Source { index, .. } => editor.entries.get(*index).map(|e| {
                    editor
                        .driver(&e.source.driver)
                        .map_or(e.source.driver.clone(), |d| d.name.clone())
                }),
                _ => None,
            })
            .map(|t| pen.text.shape_line(&t, &detail_font).width)
            .fold(0.0_f32, f32::max);
        let mut y = list.min_y() - self.list_scroll.round();
        let mut row_hits = Vec::new();
        let drop = self.dragging.and(self.drop);
        for row in rows {
            if let ListRow::Section(scope) = row {
                let h = px(SECTION_H);
                let r = Rect::new(list.min_x(), y, list.size.width, h);
                if y + h > list.min_y() && y < list.max_y() {
                    if drop == Some(Place::Top(scope)) {
                        drop_target(p).paint(pen.layer, ds, r);
                    }
                    let label = t(match scope {
                        Scope::Project => "datasources.section_project",
                        Scope::Global => "datasources.section_global",
                    })
                    .to_uppercase();
                    let shaped = pen.text.shape_line(&label, &small);
                    draw_text(
                        pen.layer,
                        &shaped,
                        list.min_x() + px(6.0),
                        ds::optical_baseline(y + px(4.0), h - px(4.0), &small),
                        p.text_muted,
                    );
                }
                row_hits.push((r.intersect(&list).unwrap_or_default(), Hit::Section(scope)));
                y += h;
                continue;
            }
            let h = px(ROW_H);
            let row_rect = Rect::new(list.min_x(), y, list.size.width, h);
            let visible = y + h > list.min_y() && y < list.max_y();
            y += h;
            if !visible {
                continue;
            }
            match row {
                ListRow::Section(_) => {}
                ListRow::Empty(_) => {
                    let shaped = pen.text.shape_line(&t("datasources.section_empty"), &detail_font);
                    draw_text(
                        pen.layer,
                        &shaped,
                        row_rect.min_x() + px(10.0),
                        ds::optical_baseline(row_rect.min_y(), h, &detail_font),
                        p.text_muted,
                    );
                }
                ListRow::Folder { id, depth, open, count } => {
                    let Some(folder) = editor.folder(id) else { continue };
                    let selected = editor.selected_folder == Some(id);
                    let target =
                        drop == Some(Place::Folder(id)) && self.dragging != Some(Item::Folder(id));
                    if target {
                        drop_target(p).paint(pen.layer, ds, row_rect);
                    } else {
                        let hovered = matches!(self.hover, Some(Hit::Folder(f, _)) if f == id);
                        list_row(selected, hovered).paint(pen.layer, ds, row_rect);
                    }
                    let x = row_rect.min_x() + px(6.0) + f32::from(depth) * indent;
                    let chevron = px(12.0);
                    let chev = Rect::new(
                        x.round(),
                        (row_rect.min_y() + (h - chevron) / 2.0).round(),
                        chevron,
                        chevron,
                    );
                    ui_components::tree::chevron(pen.layer, pen.icons, chev, open, p.text_muted);
                    let color = editor
                        .folder_color(id)
                        .and_then(|c| ui_components::tag_colors::named(&c.name(), p));
                    let s = px(15.0);
                    if let Some(icon) = pen.icons.id("folder") {
                        let at = Rect::new(
                            chev.max_x() + px(4.0),
                            (row_rect.min_y() + (h - s) / 2.0).round(),
                            s,
                            s,
                        );
                        ui_kit::draw_icon(
                            pen.layer,
                            pen.icons,
                            icon,
                            at,
                            color.unwrap_or(p.folder_icon),
                        );
                    }
                    let tx = chev.max_x() + px(4.0) + s + px(8.0);
                    let counted = count.to_string();
                    let cw = pen.text.shape_line(&counted, &detail_font).width;
                    let room = (row_rect.max_x() - px(10.0) - cw - px(10.0) - tx).max(px(30.0));
                    let name = if folder.name.is_empty() {
                        t("datasources.unnamed")
                    } else {
                        folder.name.clone()
                    };
                    let shown = ds::ellipsize(pen.text, &name, &font, room);
                    let shaped = pen.text.shape_line(&shown, &font);
                    draw_text(
                        pen.layer,
                        &shaped,
                        tx,
                        ds::optical_baseline(row_rect.min_y(), h, &font),
                        p.text,
                    );
                    if folder.production {
                        let right = row_rect.max_x() - px(18.0) - trailing;
                        self.production_badge(pen, row_rect, right);
                    }
                    let cs = pen.text.shape_line(&counted, &detail_font);
                    draw_text(
                        pen.layer,
                        &cs,
                        row_rect.max_x() - px(10.0) - cw,
                        ds::optical_baseline(row_rect.min_y(), h, &detail_font),
                        p.text_muted,
                    );
                    let clipped = row_rect.intersect(&list).unwrap_or_default();
                    row_hits.push((clipped, Hit::Folder(id, false)));
                    let chev_hit =
                        Rect::new(chev.min_x() - px(4.0), row_rect.min_y(), chevron + px(8.0), h);
                    row_hits.push((
                        chev_hit.intersect(&list).unwrap_or_default(),
                        Hit::Folder(id, true),
                    ));
                }
                ListRow::Source { index: i, depth } => {
                    let e = &editor.entries[i];
                    let selected = editor.selected == Some(i);
                    list_row(selected, self.hover == Some(Hit::Row(i)))
                        .paint(pen.layer, ds, row_rect);
                    let shift = f32::from(depth) * indent + if depth > 0 { px(6.0) } else { 0.0 };
                    let color = editor
                        .color_of(i)
                        .and_then(|c| ui_components::tag_colors::named(&c.name(), p));
                    if let Some(color) = color {
                        // Its color: a mark at the row's start.
                        let bar = Rect::new(
                            row_rect.min_x() + px(3.0) + shift,
                            row_rect.min_y() + px(8.0),
                            px(3.0),
                            h - px(16.0),
                        );
                        pen.layer.quad(Quad::filled(bar, color).rounded(px(1.5)));
                    }
                    let (icon, tone) = editor
                        .driver(&e.source.driver)
                        .map_or(("database", database::IconTone::Neutral), |d| (d.icon, d.tone));
                    let s = px(15.0);
                    if let Some(id) = pen.icons.id(icon) {
                        let at = Rect::new(
                            row_rect.min_x() + px(10.0) + shift,
                            (row_rect.min_y() + (h - s) / 2.0).round(),
                            s,
                            s,
                        );
                        let engine = || crate::engine_icon::color(icon, tone, p, p.text_muted);
                        let tint = if selected { p.accent } else { color.unwrap_or_else(engine) };
                        ui_kit::draw_icon(pen.layer, pen.icons, id, at, tint);
                    }
                    let x = row_rect.min_x() + px(34.0) + shift;
                    let changed = e.changed();
                    let name = if e.source.name.is_empty() {
                        t("datasources.unnamed")
                    } else {
                        e.source.name.clone()
                    };
                    let driver = editor
                        .driver(&e.source.driver)
                        .map_or(e.source.driver.clone(), |d| d.name.clone());
                    let dw = pen.text.shape_line(&driver, &detail_font).width;
                    let room = (row_rect.max_x() - px(10.0) - x - dw - px(10.0)).max(px(40.0));
                    let shown = ds::ellipsize(pen.text, &name, &font, room);
                    let shaped = pen.text.shape_line(&shown, &font);
                    let base = ds::optical_baseline(row_rect.min_y(), h, &font);
                    draw_text(pen.layer, &shaped, x, base, p.text);
                    if changed {
                        // Not applied yet: a dot after its name.
                        let d = px(6.0);
                        let at = Rect::new(
                            x + shaped.width + px(6.0),
                            (row_rect.min_y() + (h - d) / 2.0).round(),
                            d,
                            d,
                        );
                        pen.layer.quad(Quad::filled(at, p.accent).rounded(d / 2.0));
                    }
                    let ds_shaped = pen.text.shape_line(&driver, &detail_font);
                    draw_text(
                        pen.layer,
                        &ds_shaped,
                        row_rect.max_x() - px(10.0) - dw,
                        ds::optical_baseline(row_rect.min_y(), h, &detail_font),
                        p.text_muted,
                    );
                    if editor.production_of(i) {
                        let right = row_rect.max_x() - px(18.0) - trailing;
                        self.production_badge(pen, row_rect, right);
                    }
                    row_hits.push((row_rect.intersect(&list).unwrap_or_default(), Hit::Row(i)));
                }
            }
        }
        self.hits.extend(row_hits);
    }

    /// The production badge ending at `right` in a list's row.
    fn production_badge(&self, pen: &mut Pen<'_, '_>, row: Rect, right: f32) {
        let label = t("datasources.badge_production");
        let mid = row.min_y() + row.size.height / 2.0;
        let spec = ds::BadgeSpec {
            label: &label,
            x: right,
            mid,
            align_right: true,
            tone: ds::Tone::Danger,
        };
        ds::badge(pen.layer, pen.text, pen.ds, spec);
    }

    // ─── The right side ───

    fn label(
        &self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        y: f32,
        label: &str,
        error: Option<&str>,
        width: f32,
    ) {
        let ds = pen.ds;
        let font = ds.ui_font(-1.0);
        let shaped = pen.text.shape_line(label, &font);
        let base = ds::optical_baseline(y, ds.px(18.0), &font);
        draw_text(pen.layer, &shaped, x, base, ds.palette.text_muted);
        if let Some(error) = error {
            let room = (width - shaped.width - ds.px(16.0)).max(0.0);
            let shown = ds::ellipsize(pen.text, error, &font, room);
            let e = pen.text.shape_line(&shown, &font);
            draw_text(pen.layer, &e, x + width - e.width, base, ds.palette.danger);
        }
    }

    /// A text field in `r`, `placeholder` when empty.
    fn input(&mut self, pen: &mut Pen<'_, '_>, field: Field, r: Rect, placeholder: &str) {
        let style = Self::style(pen.ds, true);
        let focused = self.focus.as_ref() == Some(&field);
        if let Some(input) = self.editor.as_ref().and_then(|e| e.input(&field)) {
            input.paint(pen.layer, pen.text, r, focused, placeholder, &style);
        }
        self.hits.push((r, Hit::Field(field)));
    }

    /// A field drawn disabled (what isn't there yet: the SSH tunnel).
    fn disabled(&self, pen: &mut Pen<'_, '_>, r: Rect, placeholder: &str) {
        let style = Self::style(pen.ds, false);
        ui_kit::TextInput::new("").paint(pen.layer, pen.text, r, false, placeholder, &style);
    }

    fn header_tabs(&mut self, pen: &mut Pen<'_, '_>, x: f32, mid: f32) {
        let Some(tab) = self.editor.as_ref().map(|e| e.tab) else { return };
        let labels: Vec<String> = Tab::ALL.iter().map(|t_| t(t_.label_key())).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let w = ds::segmented_width(pen.text, pen.ds, &refs);
        let hovered = Tab::ALL.iter().position(|t_| self.hover == Some(Hit::Tab(*t_)));
        let selected = Tab::ALL.iter().position(|t_| *t_ == tab).unwrap_or(0);
        let rects = ds::segmented(
            pen.layer,
            pen.text,
            pen.ds,
            ds::SegmentedSpec { right: x + w, mid, labels: &refs, selected, hovered },
        );
        for (r, t_) in rects.into_iter().zip(Tab::ALL) {
            self.hits.push((r, Hit::Tab(t_)));
        }
    }

    /// Name and comment, then the tabs.
    fn paint_header(&mut self, pen: &mut Pen<'_, '_>, r: Rect) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let half = ((r.size.width - px(16.0)) / 2.0).floor();
        let no_name = self
            .editor
            .as_ref()
            .and_then(|e| e.current())
            .is_some_and(|e| e.source.name.trim().is_empty());
        let error = no_name.then(|| t("datasources.error_required"));
        self.label(pen, r.min_x(), r.min_y(), &t("datasources.field_name"), error.as_deref(), half);
        self.input(
            pen,
            Field::Name,
            Rect::new(r.min_x(), r.min_y() + px(22.0), half, px(FIELD_H)),
            "",
        );
        let cx = r.min_x() + half + px(16.0);
        self.label(pen, cx, r.min_y(), &t("datasources.field_comment"), None, half);
        self.input(
            pen,
            Field::Comment,
            Rect::new(cx, r.min_y() + px(22.0), half, px(FIELD_H)),
            &t("datasources.comment_hint"),
        );
        let mid = r.min_y() + px(GROUP_H) + px(18.0);
        self.header_tabs(pen, r.min_x(), mid);
        // Its driver, at the tabs' end.
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let driver = editor
            .driver(&entry.source.driver)
            .map_or(entry.source.driver.clone(), |d| d.name.clone());
        let font = ds.ui_font(-1.0);
        let label = tr!("datasources.driver_is", driver = driver.as_str());
        let shaped = pen.text.shape_line(&label, &font);
        draw_text(
            pen.layer,
            &shaped,
            r.max_x() - shaped.width,
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            ds.palette.text_muted,
        );
    }

    fn paint_tab(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let Some(tab) = self.editor.as_ref().map(|e| e.tab) else { return };
        match tab {
            Tab::General => self.paint_general(pen, x, w, y),
            Tab::Options => self.paint_options(pen, x, w, y),
            Tab::Ssl => self.paint_ssl(pen, x, w, y),
            Tab::Schemas => self.paint_schemas(pen, x, w, y),
            Tab::Advanced => self.paint_advanced(pen, x, w, y),
        }
    }

    /// A driver's field at (`x`, `y`), `w` wide (its label over it).
    fn spec_field(&mut self, pen: &mut Pen<'_, '_>, field: &FieldSpec, x: f32, y: f32, w: f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let problem = editor.field_problem(&field.id).map(|e| field_error_text(&e));
        let r = Rect::new(x, y + px(22.0), w, px(FIELD_H));
        match &field.kind {
            FieldKind::Flag => {
                let on = editor.choice_of(field) == "true";
                let mid = y + px(22.0) + px(FIELD_H) / 2.0;
                let state = if on { Check::On } else { Check::Off };
                let check = ds::checkbox(pen.layer, pen.text, ds, x, mid, state, true);
                let font = ds.ui_font(-0.5);
                let shaped = pen.text.shape_line(&t(&field.label), &font);
                let lx = check.max_x() + px(8.0);
                draw_text(
                    pen.layer,
                    &shaped,
                    lx,
                    ds::optical_baseline(mid - px(10.0), px(20.0), &font),
                    ds.palette.text,
                );
                self.hits.push((
                    Rect::new(
                        x - px(4.0),
                        mid - px(12.0),
                        lx + shaped.width - x + px(8.0),
                        px(24.0),
                    ),
                    Hit::Flag(field.id.clone()),
                ));
            }
            FieldKind::Choice(values) => {
                self.label(pen, x, y, &t(&field.label), problem.as_deref(), w);
                let now = editor.choice_of(field);
                let shown =
                    values.iter().find(|(v, _)| *v == now).map_or(now.clone(), |(_, l)| t(l));
                self.dropdown(pen, Choice::Spec(field.id.clone()), r, &shown);
            }
            FieldKind::File { .. } => {
                self.label(pen, x, y, &t(&field.label), problem.as_deref(), w);
                let browse = t("datasources.browse");
                let bw = ds::primary_width(pen.text, ds, &[&browse]);
                let input =
                    Rect::new(r.min_x(), r.min_y(), r.size.width - bw - px(8.0), r.size.height);
                self.input(pen, Field::Spec(field.id.clone()), input, &field.default);
                let button = Rect::new(input.max_x() + px(8.0), r.min_y(), bw, r.size.height);
                self.secondary(pen, button, &browse, Hit::Browse(field.id.clone()), true);
            }
            FieldKind::Secret => {}
            _ => {
                self.label(pen, x, y, &t(&field.label), problem.as_deref(), w);
                self.input(pen, Field::Spec(field.id.clone()), r, &field.default);
            }
        }
    }

    /// A list to choose from, shown as a field with its value and a chevron.
    fn dropdown(&mut self, pen: &mut Pen<'_, '_>, choice: Choice, r: Rect, value: &str) {
        let open = self.popup.as_ref() == Some(&choice);
        let hovered = self.hover == Some(Hit::Choice(choice.clone()));
        let spec =
            ds::DropdownSpec { value, open, hovered, enabled: true, note: None, placeholder: None };
        ds::dropdown(pen.layer, pen.text, pen.icons, pen.ds, r, spec);
        self.hits.push((r, Hit::Choice(choice.clone())));
        self.anchors.insert(choice, r);
    }

    /// A bordered button.
    fn secondary(&mut self, pen: &mut Pen<'_, '_>, r: Rect, label: &str, hit: Hit, enabled: bool) {
        let hovered = self.hover.as_ref() == Some(&hit);
        ds::secondary_button(pen.layer, pen.text, pen.ds, r, label, enabled, hovered);
        if enabled {
            self.hits.push((r, hit));
        }
    }

    fn title(&self, pen: &mut Pen<'_, '_>, x: f32, y: f32, title: &str) {
        let font = pen.ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let shaped = pen.text.shape_line(title, &font);
        draw_text(
            pen.layer,
            &shaped,
            x,
            ds::optical_baseline(y, pen.ds.px(22.0), &font),
            pen.ds.palette.text,
        );
    }

    /// A note in muted words, wrapped at `w`; its height.
    fn note(&self, pen: &mut Pen<'_, '_>, x: f32, y: f32, w: f32, text: &str) -> f32 {
        let ds = pen.ds;
        let font = ds.ui_font(-1.0);
        let line_h = ds.px(19.0);
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        for word in text.split(' ') {
            let candidate =
                if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if !line.is_empty() && pen.text.shape_line(&candidate, &font).width > w {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        for (i, l) in lines.iter().enumerate() {
            let shaped = pen.text.shape_line(l, &font);
            draw_text(
                pen.layer,
                &shaped,
                x,
                ds::optical_baseline(y + i as f32 * line_h, line_h, &font),
                ds.palette.text_muted,
            );
        }
        lines.len() as f32 * line_h
    }

    fn paint_general(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        if editor.current().is_none() {
            return;
        }
        *y += px(4.0);
        let mut fields = editor.fields_in(FieldTab::General);
        // A user and the database side by side (the database moved up beside it).
        if let (Some(u), Some(d)) = (
            fields.iter().position(|f| f.id == "user"),
            fields.iter().position(|f| f.id == "database"),
        ) && d > u + 1
        {
            let database = fields.remove(d);
            fields.insert(u + 1, database);
        }
        let spec = editor.spec();
        let mut i = 0;
        while i < fields.len() {
            let field = &fields[i];
            if field.is_secret() {
                self.password_row(pen, x, *y, w);
                *y += px(GROUP_H);
                i += 1;
                continue;
            }
            // A host and its port side by side.
            if field.id == "host" && fields.get(i + 1).is_some_and(|f| f.id == "port") {
                let port_w = px(120.0);
                self.spec_field(pen, field, x, *y, w - port_w - px(12.0));
                self.spec_field(pen, &fields[i + 1].clone(), x + w - port_w, *y, port_w);
                *y += px(GROUP_H);
                i += 2;
                continue;
            }
            if field.id == "user" && fields.get(i + 1).is_some_and(|f| f.id == "database") {
                let half = ((w - px(12.0)) / 2.0).floor();
                self.spec_field(pen, field, x, *y, half);
                self.spec_field(
                    pen,
                    &fields[i + 1].clone(),
                    x + half + px(12.0),
                    *y,
                    w - half - px(12.0),
                );
                *y += px(GROUP_H);
                i += 2;
                continue;
            }
            let field = field.clone();
            self.spec_field(pen, &field, x, *y, w);
            *y += px(GROUP_H);
            i += 1;
        }
        if spec.url.is_some() {
            let error = self.editor.as_ref().and_then(|e| e.url_error.as_ref()).map(url_error_text);
            self.label(pen, x, *y, &t("datasources.field_url"), error.as_deref(), w);
            self.input(pen, Field::Url, Rect::new(x, *y + px(22.0), w, px(FIELD_H)), "");
            *y += px(GROUP_H);
        }
        // Its color (its folder's unless it has one).
        let (own, inherited) = {
            let Some(editor) = &self.editor else { return };
            let Some(at) = editor.selected else { return };
            let own = editor.entries[at].source.color;
            let inherited = editor.entries[at].source.folder.and_then(|f| editor.folder_color(f));
            (own, inherited)
        };
        self.label(pen, x, *y, &t("datasources.field_color"), None, w);
        let mut sx = x;
        let mid = *y + px(22.0) + px(14.0);
        let chip =
            self.inherit_chip(pen, sx, mid, "datasources.color_inherit", inherited, own.is_none());
        self.hits.push((chip, Hit::SourceColor(None)));
        sx = chip.max_x() + px(12.0);
        self.swatches(pen, sx, mid, own, false);
        *y += px(22.0 + 28.0 + 8.0);
        *y += self.note(pen, x, *y, w, &t("datasources.color_note")) + px(12.0);
    }

    /// The "none" choice of a color row, as a chip: its label, and the color it means
    /// (what's inherited) as a dot, when there's one. Returns its area.
    fn inherit_chip(
        &mut self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        mid: f32,
        label: &str,
        means: Option<SourceColor>,
        chosen: bool,
    ) -> Rect {
        let ds = pen.ds;
        let label = t(label);
        let hovered = self.hover == Some(Hit::SourceColor(None))
            || self.hover == Some(Hit::FolderColor(None));
        let spec = ds::ChoiceSpec { x, mid, label: &label, selected: chosen, hovered };
        match means.and_then(|c| ui_components::tag_colors::named(&c.name(), ds.palette)) {
            Some(mark) => ds::choice_with_dot(pen.layer, pen.text, ds, spec, mark),
            None => ds::choice(pen.layer, pen.text, ds, spec),
        }
    }

    /// The named colors as round swatches from `x`, centered on `mid`, `now` ringed; a
    /// folder's (`folder`) or the selected source's.
    fn swatches(
        &mut self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        mid: f32,
        now: Option<SourceColor>,
        folder: bool,
    ) {
        let ds = pen.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let size = px(SWATCH);
        let mut sx = x;
        for color in SourceColor::NAMED {
            let Some(fill) = ui_components::tag_colors::named(color.key(), p) else { continue };
            let r = Rect::new(sx, (mid - size / 2.0).round(), size, size);
            let hit =
                if folder { Hit::FolderColor(Some(color)) } else { Hit::SourceColor(Some(color)) };
            ds::ColorSwatch::new(fill)
                .radius(SWATCH / 2.0)
                .ringed_around(now == Some(color))
                .hovered(self.hover.as_ref() == Some(&hit))
                .paint(pen.layer, ds, r);
            self.tips.push((r, t(&format!("datasources.color_{}", color.key()))));
            self.hits.push((r, hit));
            sx += size + px(12.0);
        }
    }

    /// The selected folder on the right: its name, its color, what it holds.
    fn paint_folder(&mut self, pen: &mut Pen<'_, '_>, x: f32, y: f32, w: f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(folder) = editor.current_folder().cloned() else { return };
        let count = editor.count_in(folder.id);
        let inherited = folder.parent.and_then(|p| editor.folder_color(p));
        let no_name = folder.name.trim().is_empty();
        let error = no_name.then(|| t("datasources.error_required"));
        let half = ((w - px(16.0)) / 2.0).floor();
        self.label(pen, x, y, &t("datasources.folder_name"), error.as_deref(), half);
        self.input(pen, Field::FolderName, Rect::new(x, y + px(22.0), half, px(FIELD_H)), "");
        let font = ds.ui_font(-1.0);
        let detail = tr!("datasources.folder_count", count = count);
        let shaped = pen.text.shape_line(&detail, &font);
        draw_text(
            pen.layer,
            &shaped,
            x + half + px(16.0),
            ds::optical_baseline(y + px(22.0), px(FIELD_H), &font),
            ds.palette.text_muted,
        );
        let mut y = y + px(GROUP_H);
        self.label(pen, x, y, &t("datasources.field_color"), None, w);
        let mid = y + px(22.0) + px(14.0);
        let key = if inherited.is_some() {
            "datasources.color_inherit"
        } else {
            "datasources.color_none"
        };
        let chip = self.inherit_chip(pen, x, mid, key, inherited, folder.color.is_none());
        self.hits.push((chip, Hit::FolderColor(None)));
        self.swatches(pen, chip.max_x() + px(12.0), mid, folder.color, true);
        y += px(22.0 + 28.0 + 16.0);
        // Production: every source in it (and its subfolders) is too.
        let parent =
            folder.parent.filter(|p| self.editor.as_ref().is_some_and(|e| e.folder_production(*p)));
        let on = folder.production || parent.is_some();
        let lx = self.check_row(
            pen,
            (x, y),
            &t("datasources.folder_production"),
            on,
            parent.is_none(),
            Hit::FolderProduction,
        );
        y += px(30.0);
        let note = if parent.is_some() {
            t("datasources.folder_production_inherited")
        } else {
            t("datasources.folder_production_note")
        };
        y += self.note(pen, lx, y, w - (lx - x), &note) + px(16.0);
        y += self.note(pen, x, y, w, &t("datasources.folder_note")) + px(8.0);
        self.note(pen, x, y, w, &t("datasources.folder_drop"));
    }

    /// The user and the password's field (masked, its eye), then where it's kept.
    fn password_row(&mut self, pen: &mut Pen<'_, '_>, x: f32, y: f32, w: f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &mut self.editor else { return };
        let revealed = editor.revealed;
        let Some(entry) = editor.current() else { return };
        let policy = crate::credentials::effective(&entry.source);
        let stored = entry.original.is_some() && !entry.password_typed;
        let keeps = matches!(
            policy,
            database::SecretPolicy::ProjectVault | database::SecretPolicy::Keychain
        );
        let placeholder =
            if stored && keeps { t("datasources.password_saved") } else { String::new() };
        let policy_w = px(230.0).min(w * 0.42);
        let pw = w - policy_w - px(12.0);
        self.label(pen, x, y, &t("datasources.field_password"), None, pw);
        self.label(pen, x + pw + px(12.0), y, &t("datasources.field_save"), None, policy_w);
        // Shown as typed only while its eye is on.
        if let Some(e) =
            self.editor.as_mut().and_then(|e| e.entries.get_mut(e.selected.unwrap_or(usize::MAX)))
        {
            e.password.set_masked(!revealed);
        }
        let r = Rect::new(x, y + px(22.0), pw, px(FIELD_H));
        self.input(pen, Field::Password, r, &placeholder);
        let eye =
            Rect::new(r.max_x() - px(32.0), r.min_y() + px(3.0), px(28.0), r.size.height - px(6.0));
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon(if revealed { "eye-off" } else { "eye" })
            .hovered(self.hover == Some(Hit::Reveal));
        ds::button(pen.layer, pen.text, pen.icons, pen.ds, eye, spec);
        self.hits.push((eye, Hit::Reveal));
        self.tips.push((
            eye,
            t(if revealed { "datasources.password_hide" } else { "datasources.password_show" }),
        ));
        let d = Rect::new(x + pw + px(12.0), y + px(22.0), policy_w, px(FIELD_H));
        self.dropdown(pen, Choice::Policy, d, &t(policy_key(policy)));
    }

    /// A checkbox and its label at `y` (its row clickable as `hit` when `enabled`); where
    /// the label starts (a note goes under it).
    fn check_row(
        &mut self,
        pen: &mut Pen<'_, '_>,
        (x, y): (f32, f32),
        label: &str,
        on: bool,
        enabled: bool,
        hit: Hit,
    ) -> f32 {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let mid = y + px(14.0);
        let state = if on { Check::On } else { Check::Off };
        let hovered = enabled && self.hover.as_ref() == Some(&hit);
        let check = ds::Checkbox::new(state)
            .enabled(enabled)
            .hovered(hovered)
            .paint(pen.layer, pen.text, ds, x, mid);
        let font = ds.ui_font(-0.5);
        let shaped = pen.text.shape_line(label, &font);
        let lx = check.max_x() + px(8.0);
        let color = if enabled { ds.palette.text } else { ds.palette.text_muted };
        draw_text(
            pen.layer,
            &shaped,
            lx,
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            color,
        );
        if enabled {
            self.hits.push((
                Rect::new(x - px(4.0), mid - px(12.0), lx + shaped.width - x + px(8.0), px(24.0)),
                hit,
            ));
        }
        lx
    }

    fn paint_options(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let file_based = editor.driver(&entry.source.driver).is_some_and(|d| d.file_based);
        let read_only = entry.source.read_only;
        let mid = *y + px(14.0);
        let state = if read_only { Check::On } else { Check::Off };
        let check = ds::checkbox(pen.layer, pen.text, ds, x, mid, state, true);
        let font = ds.ui_font(-0.5);
        let shaped = pen.text.shape_line(&t("datasources.read_only"), &font);
        let lx = check.max_x() + px(8.0);
        draw_text(
            pen.layer,
            &shaped,
            lx,
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            ds.palette.text,
        );
        self.hits.push((
            Rect::new(x - px(4.0), mid - px(12.0), lx + shaped.width - x + px(8.0), px(24.0)),
            Hit::ReadOnly,
        ));
        *y += px(30.0);
        *y += self.note(pen, lx, *y, w - (lx - x), &t("datasources.read_only_note")) + px(16.0);
        // Production: its own mark, or its folder's (shown on, the box disabled).
        let inherited =
            editor.selected.and_then(|i| editor.inherited_production(i)).map(|f| f.name.clone());
        let on = entry.source.production || inherited.is_some();
        let lx = self.check_row(
            pen,
            (x, *y),
            &t("datasources.production"),
            on,
            inherited.is_none(),
            Hit::Production,
        );
        *y += px(30.0);
        let note = match inherited {
            Some(folder) => tr!("datasources.production_inherited", folder = folder.as_str()),
            None => t("datasources.production_note"),
        };
        *y += self.note(pen, lx, *y, w - (lx - x), &note) + px(16.0);
        let Some(editor) = &self.editor else { return };
        let fields = editor.fields_in(FieldTab::Options);
        for field in fields {
            let h = if field.kind == FieldKind::Flag { px(52.0) } else { px(GROUP_H) };
            self.spec_field(pen, &field, x, *y, w);
            *y += h;
        }
        if !file_based {
            self.label(pen, x, *y, &t("datasources.field_startup"), None, w);
            self.input(
                pen,
                Field::Startup,
                Rect::new(x, *y + px(22.0), w, px(FIELD_H)),
                &t("datasources.startup_hint"),
            );
            *y += px(GROUP_H);
        }
        *y += self.note(pen, x, *y, w, &t("datasources.idle_note")) + px(8.0);
    }

    fn paint_ssl(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        if editor.driver(&entry.source.driver).is_some_and(|d| d.file_based) {
            *y += self.note(pen, x, *y, w, &t("datasources.ssl_file_note"));
            return;
        }
        self.title(pen, x, *y, &t("datasources.ssl_title"));
        *y += px(32.0);
        let fields = editor.fields_in(FieldTab::Ssl);
        if fields.is_empty() {
            *y += self.note(pen, x, *y, w, &t("datasources.ssl_none")) + px(12.0);
        }
        let mut i = 0;
        while i < fields.len() {
            // A client's certificate and its key side by side.
            let file = |f: &FieldSpec| matches!(f.kind, FieldKind::File { .. });
            if i > 0
                && file(&fields[i])
                && file(&fields[i - 1])
                && fields.get(i + 1).is_some_and(file)
            {
                let half = ((w - px(12.0)) / 2.0).floor();
                self.spec_field(pen, &fields[i], x, *y, half);
                self.spec_field(pen, &fields[i + 1], x + half + px(12.0), *y, w - half - px(12.0));
                i += 2;
            } else {
                self.spec_field(pen, &fields[i], x, *y, w);
                i += 1;
            }
            *y += px(GROUP_H);
        }
        *y += px(8.0);
        self.title(pen, x, *y, &t("datasources.ssh_title"));
        *y += px(28.0);
        *y += self.note(pen, x, *y, w, &t("datasources.ssh_note")) + px(12.0);
        let port_w = px(120.0);
        self.label(pen, x, *y, &t("datasources.field_ssh_host"), None, w);
        self.label(pen, x + w - port_w, *y, &t("datasources.field_port"), None, port_w);
        self.disabled(pen, Rect::new(x, *y + px(22.0), w - port_w - px(12.0), px(FIELD_H)), "");
        self.disabled(pen, Rect::new(x + w - port_w, *y + px(22.0), port_w, px(FIELD_H)), "22");
        *y += px(GROUP_H);
        let half = ((w - px(12.0)) / 2.0).floor();
        self.label(pen, x, *y, &t("datasources.field_user"), None, half);
        self.label(pen, x + half + px(12.0), *y, &t("datasources.field_ssh_key"), None, half);
        self.disabled(pen, Rect::new(x, *y + px(22.0), half, px(FIELD_H)), "");
        self.disabled(
            pen,
            Rect::new(x + half + px(12.0), *y + px(22.0), half, px(FIELD_H)),
            "~/.ssh/id_ed25519",
        );
        *y += px(GROUP_H);
    }

    fn paint_schemas(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let driver = editor.driver(&entry.source.driver);
        let has = driver.is_some_and(|d| d.schemas);
        let has_databases = driver.is_some_and(|d| d.databases);
        if !has {
            *y += self.note(pen, x, *y, w, &t("datasources.schemas_none"));
            return;
        }
        // A server's databases first (those the tree shows), then the schemas shown in each.
        if has_databases {
            let all = editor.databases();
            let every = entry.source.databases.is_empty();
            *y += self.note(pen, x, *y, w, &t("datasources.databases_note")) + px(12.0);
            if all.is_empty() {
                *y += self.note(pen, x, *y, w, &t("datasources.databases_test")) + px(16.0);
            } else {
                let rows: Vec<(String, bool, Hit)> =
                    std::iter::once((t("datasources.databases_all"), every, Hit::AllDatabases))
                        .chain(all.iter().map(|d| {
                            (d.clone(), editor.shows_database(d), Hit::Database(d.clone()))
                        }))
                        .collect();
                self.paint_checks(pen, x, w, y, rows);
                *y += px(16.0);
            }
        }
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let all = editor.schemas();
        let every = entry.source.schemas.is_empty();
        *y += self.note(pen, x, *y, w, &t("datasources.schemas_note")) + px(12.0);
        if all.is_empty() {
            *y += self.note(pen, x, *y, w, &t("datasources.schemas_test"));
            return;
        }
        let rows: Vec<(String, bool, Hit)> =
            std::iter::once((t("datasources.schemas_all"), every, Hit::AllSchemas))
                .chain(
                    all.iter().map(|s| (s.clone(), editor.shows_schema(s), Hit::Schema(s.clone()))),
                )
                .collect();
        self.paint_checks(pen, x, w, y, rows);
    }

    /// A list of checkboxes, the first ("all") flush, the others under it.
    fn paint_checks(
        &mut self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        w: f32,
        y: &mut f32,
        rows: Vec<(String, bool, Hit)>,
    ) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        for (i, (label, on, hit)) in rows.into_iter().enumerate() {
            let h = px(32.0);
            let row = Rect::new(x - px(4.0), *y, w + px(8.0), h);
            let hovered = self.hover.as_ref() == Some(&hit);
            list_row(false, hovered).paint(pen.layer, ds, row);
            let mid = *y + h / 2.0;
            let indent = if i == 0 { 0.0 } else { px(26.0) };
            let state = if on { Check::On } else { Check::Off };
            let check = ds::Checkbox::new(state).hovered(hovered).paint(
                pen.layer,
                pen.text,
                ds,
                x + indent,
                mid,
            );
            let shaped = pen.text.shape_line(&label, &font);
            draw_text(
                pen.layer,
                &shaped,
                check.max_x() + px(8.0),
                ds::optical_baseline(mid - px(10.0), px(20.0), &font),
                ds.palette.text,
            );
            self.hits.push((row, hit));
            *y += h;
        }
    }

    fn paint_advanced(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        *y += self.note(pen, x, *y, w, &t("datasources.advanced_note")) + px(12.0);
        let n = editor.options.len();
        let key_w = ((w - px(48.0)) * 0.4).floor();
        let value_w = w - key_w - px(48.0);
        if n > 0 {
            self.label(pen, x, *y, &t("datasources.option_name"), None, key_w);
            self.label(
                pen,
                x + key_w + px(10.0),
                *y,
                &t("datasources.option_value"),
                None,
                value_w,
            );
            *y += px(22.0);
        }
        for i in 0..n {
            let secret = self
                .editor
                .as_ref()
                .and_then(|e| e.options.get(i))
                .is_some_and(|(k, _)| database::source::is_secret_option(k.text().trim()));
            self.input(pen, Field::OptionKey(i), Rect::new(x, *y, key_w, px(FIELD_H)), "sslmode");
            self.input(
                pen,
                Field::OptionValue(i),
                Rect::new(x + key_w + px(10.0), *y, value_w, px(FIELD_H)),
                "",
            );
            let remove = ds::square_button(
                ds,
                (x + w - px(30.0), *y + px(FIELD_H) / 2.0),
                button_size::LARGE,
            );
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon("minus")
                .hovered(self.hover == Some(Hit::RemoveOption(i)));
            ds::button(pen.layer, pen.text, pen.icons, ds, remove, spec);
            self.hits.push((remove, Hit::RemoveOption(i)));
            self.tips.push((remove, t("datasources.option_remove")));
            *y += px(FIELD_H) + px(8.0);
            if secret {
                let font = ds.ui_font(-1.0);
                let shaped = pen.text.shape_line(&t("datasources.option_secret"), &font);
                draw_text(
                    pen.layer,
                    &shaped,
                    x,
                    ds::optical_baseline(*y - px(4.0), px(18.0), &font),
                    ds.palette.danger,
                );
                *y += px(18.0);
            }
        }
        let label = t("datasources.option_add");
        let bw = ds::primary_width(pen.text, ds, &[&label]);
        self.secondary(
            pen,
            Rect::new(x, *y + px(4.0), bw, px(ds::BUTTON_H)),
            &label,
            Hit::AddOption,
            true,
        );
        *y += px(ds::BUTTON_H) + px(12.0);
    }
}
