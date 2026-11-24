//! The sources' list painted: the filter and the toolbar, the rows by list (the project's,
//! then every project's), the one dragged and where it drops, the production badge.

use super::*;

impl SourcesView {
    /// The filter and the toolbar; the rows' area between them.
    pub(super) fn paint_list(&mut self, pen: &mut Pen<'_, '_>, r: Rect) -> Rect {
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
    pub(super) fn paint_rows(&mut self, pen: &mut Pen<'_, '_>, list: Rect) {
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
                    // To a screen reader: a folder of the tree (its name, depth, open or not).
                    let _item = ui_components::tree::item(ui_components::tree::Item {
                        name: &folder.name,
                        bounds: row_rect,
                        level: usize::from(depth),
                        expanded: Some(open),
                        selected,
                    });
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
                        let center = kernel::Point::new(
                            x + shaped.width + px(6.0 + CHANGED_DOT / 2.0),
                            row_rect.min_y() + h / 2.0,
                        );
                        ds::dot(pen.layer, ds, center, CHANGED_DOT, p.accent);
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
}
