//! Painting the picker: the modal, the home screen, the list's rows (actions, files, projects,
//! groups) and what a drag shows.

use super::*;

impl ProjectsPicker {
    /// The modal opened from the top bar (the `Modal` frame, like settings).
    pub fn paint_modal(
        &mut self,
        current: Option<&RecentProject>,
        recent: &RecentList,
        painter: Painter<'_>,
    ) {
        let Painter { text, scene, palette, icons } = painter;
        if !self.modal.is_open() {
            return;
        }
        let title = i18n::t("home.title");
        let subtitle = i18n::t("home.subtitle");
        let [mv, open, close] = hints();
        let hints: [KeyHint<'_>; 3] = [(&["↑", "↓"], &mv), (&["↵"], &open), (&["esc"], &close)];
        let spec = ModalSpec {
            title: &title,
            icon: Some("folder-open"),
            subtitle: Some(&subtitle),
            size: ModalSize::Medium,
            sections: &[],
            nav_header: 0.0,
            header_field: 0.0,
            active: None,
            hints: &hints,
            buttons: &[],
            status: None,
            fit: None,
            fit_width: None,
            title_widens: true,
            closable: true,
            zoom: None,
        };
        let _dialog = self.modal.told(&spec);
        let Some((content, ctx)) = self.modal.paint(&spec, icons, text, scene, palette) else {
            return;
        };
        let pad = ctx.px(18.0);
        // Room above the footer too: the last row never touches its line.
        let inner = Rect::new(
            content.min_x() + pad,
            content.min_y() + ctx.px(14.0),
            content.size.width - 2.0 * pad,
            content.size.height - ctx.px(14.0 + 12.0),
        );
        self.rebuild(current, recent);
        self.paint_content(inner, Painter { text, scene, palette, icons }, &ctx);
    }

    /// On the editor's empty screen: a centered card with the same content and the key
    /// hints below.
    pub fn paint_home(
        &mut self,
        area: Rect,
        scale: f32,
        content: HomeContent<'_>,
        current: Option<&RecentProject>,
        recent: &RecentList,
        painter: Painter<'_>,
    ) {
        let Painter { text, scene, palette, icons } = painter;
        let ctx = Ctx::new(palette, scale);
        let px = |v: f32| ctx.px(v);
        let w = px(560.0).min(area.size.width - px(48.0));
        let h = px(520.0).min(area.size.height - px(48.0));
        if w <= px(160.0) || h <= px(160.0) {
            return;
        }
        let card = Rect::new(
            (area.min_x() + (area.size.width - w) / 2.0).round(),
            (area.min_y() + (area.size.height - h) / 2.0 - px(12.0)).round(),
            w,
            h,
        );
        let layer = scene.push_layer(Some(area));
        // The title, above the list.
        let title_font = ctx.ui_font(6.0).with_weight(Font::SEMIBOLD);
        let (title, subtitle) = match content {
            HomeContent::Projects => (i18n::t("home.welcome"), i18n::t("home.subtitle")),
            HomeContent::Project { name, .. } => {
                (name.to_owned(), i18n::t("home.project_subtitle"))
            }
        };
        let shaped = text.shape_line(&title, &title_font);
        draw_text(
            layer,
            &shaped,
            card.min_x() + px(4.0),
            card.min_y() + title_font.size,
            palette.text,
        );
        let sub = ctx.ui_font(-0.5);
        let shaped = text.shape_line(&subtitle, &sub);
        draw_text(
            layer,
            &shaped,
            card.min_x() + px(4.0),
            card.min_y() + title_font.size + px(24.0),
            palette.text_muted,
        );
        let top = card.min_y() + title_font.size + px(44.0);
        let footer_h = px(36.0);
        let inner = Rect::new(card.min_x(), top, card.size.width, card.max_y() - top - footer_h);
        match content {
            HomeContent::Projects => self.rebuild(current, recent),
            HomeContent::Project { files, searching, .. } => self.rebuild_project(files, searching),
        }
        self.paint_content(inner, Painter { text, scene, palette, icons }, &ctx);
        // The key hints.
        let layer = scene.push_layer(Some(area));
        let [mv, open, _] = hints();
        let close = i18n::t("home.hint_clear");
        let remove = i18n::t("home.hint_remove");
        let mut hints: Vec<KeyHint<'_>> = vec![(&["↑", "↓"], &mv), (&["↵"], &open)];
        // A recent project selected: ⌦ takes it off the list (as its menu's row).
        if matches!(self.confirm(), Some(PickerAction::OpenRecent(_))) {
            hints.push((&["⌦"], &remove));
        }
        hints.push((&["esc"], &close));
        let row = Rect::new(
            card.min_x() + px(4.0),
            card.max_y() - footer_h + px(6.0),
            card.size.width,
            footer_h - px(6.0),
        );
        ui_kit::paint_key_hints(layer, text, &hints, row, &palette.key_hints(scale));
    }

    pub(super) fn paint_content(&mut self, area: Rect, painter: Painter<'_>, ctx: &Ctx<'_>) {
        let Painter { text, scene, icons, .. } = painter;
        let p = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let field = self.paint_filter(area, icons, text, scene, ctx);

        // The list.
        self.list = Rect::new(
            area.min_x() - px(6.0),
            field.max_y() + px(10.0),
            area.size.width + px(12.0),
            (area.max_y() - field.max_y() - px(10.0)).max(0.0),
        );
        let layer = scene.push_layer(Some(self.list));
        self.rows.clear();
        self.scale = ctx.scale;
        self.history = None;
        self.new_group = None;
        self.restore_buttons.clear();
        self.scroll_at_edge();
        let mark = self.drag.as_ref().and_then(|d| d.drop.as_ref()).map(|d| d.mark);
        let mut list = ListArea {
            layer,
            text,
            icons,
            ctx,
            font: ctx.ui_font(0.0),
            small: ctx.ui_font(-1.5),
            x0: self.list.min_x(),
        };
        let header_font = ctx.ui_font(-2.5).with_weight(Font::SEMIBOLD);
        // Whole pixels, so text and boxes scroll in step (no jitter).
        self.scroll.set_viewport(self.list.size.height);
        let top = self.scroll.origin(self.list.min_y());
        let mut y = top;
        let w = self.list.size.width;
        if self.items.is_empty() {
            let shaped = list.text.shape_line(&i18n::t("home.nothing"), &list.small);
            draw_text(
                list.layer,
                &shaped,
                list.x0 + px(12.0),
                y + px(24.0),
                ctx.fade(p.text_muted),
            );
        }
        for (i, item) in self.items.iter().enumerate() {
            let (height, depth) = match item {
                Item::Header(title) => {
                    y = paint_section_title(&mut list, title, &header_font, y);
                    continue;
                }
                Item::HistoryHeader(title) => {
                    let top = y;
                    y = paint_section_title(&mut list, title, &header_font, y);
                    let header = Rect::new(list.x0, top + px(4.0), w, y - top - px(2.0));
                    if mark == Some(Mark::Header) {
                        paint_drop_target(list.layer, ctx, header);
                    }
                    self.new_group = Some(paint_new_group(&mut list, header, self.new_group_hover));
                    self.history = Some(header);
                    continue;
                }
                Item::Project { depth, .. } => (px(50.0), *depth),
                Item::Group { depth, .. } => (px(36.0), *depth),
                Item::Action { .. } | Item::File(_) => (px(38.0), 0),
                Item::Session { .. } => (px(44.0), 0),
                Item::SessionWindow { .. } => (px(34.0), 1),
                Item::SessionFile(_) => (px(32.0), 2),
            };
            let indent = depth as f32 * px(INDENT);
            let r = Rect::new(list.x0 + indent, y, w - indent, height);
            let selected = i == self.selected;
            paint_row_bg(list.layer, ctx, r, selected, self.hover == Some(i));
            match item {
                Item::Action { label, icon, shortcut, .. } => {
                    paint_action(&mut list, r, (label.as_str(), icon, shortcut.as_str()), selected);
                }
                Item::File(file) => paint_file(&mut list, r, file),
                Item::Project { project, current, place, .. } => {
                    paint_project(&mut list, r, project, *current, place.as_deref())
                }
                Item::Group { name, collapsed, count, .. } => {
                    if mark == Some(Mark::Row(i)) {
                        paint_drop_target(list.layer, ctx, r);
                    }
                    paint_group(&mut list, r, name, *collapsed, *count);
                }
                Item::Session { id, when, detail, crashed, expanded } => {
                    let hovered = self.restore_hover == Some(*id);
                    let row = sessions::SessionRow {
                        when,
                        detail,
                        crashed: *crashed,
                        expanded: *expanded,
                    };
                    let button = sessions::paint_session(&mut list, r, &row, hovered);
                    self.restore_buttons.push((button, *id));
                }
                Item::SessionWindow { name, detail, project, count, expanded, .. } => {
                    let row = sessions::WindowRow {
                        name,
                        detail,
                        project: *project,
                        count: *count,
                        expanded: *expanded,
                    };
                    sessions::paint_window(&mut list, r, &row);
                }
                Item::SessionFile(file) => paint_file(&mut list, r, file),
                Item::Header(_) | Item::HistoryHeader(_) => {}
            }
            self.rows.push((r, i));
            y += r.size.height + px(2.0);
        }
        paint_guides(list.layer, ctx, &self.items, &self.rows);
        if let Some(Mark::Line { x, y }) = mark {
            paint_drop_line(list.layer, ctx, x, y, self.list.max_x() - px(6.0));
        }
        // Scrolling doesn't go past the end.
        self.scroll.measure(top, y);
        // Where the drag would drop, over the rows as they are now (they scroll under a
        // still mouse).
        if let Some(drag) = &self.drag {
            let (item, at) = (drag.item, drag.at);
            let drop = self.drop_target(item, at);
            if let Some(drag) = &mut self.drag {
                drag.drop = drop;
            }
        }
        // What's dragged follows the mouse.
        if let Some(drag) = self.drag.clone()
            && let Some(name) = match self.items.get(drag.item) {
                Some(Item::Project { project, .. }) => Some(project.name.as_str()),
                Some(Item::Group { name, .. }) => Some(name.as_str()),
                _ => None,
            }
        {
            let layer = scene.push_layer(None);
            paint_dragged(layer, text, icons, ctx, name, drag.at);
        }
    }
}

/// Where the list's rows are painted: the layer, the text, the icons, the drawing context,
/// the rows' fonts and their left edge.
pub(super) struct ListArea<'a, 'c> {
    pub(super) layer: &'a mut Layer,
    pub(super) text: &'a mut TextSystem,
    pub(super) icons: &'a Icons,
    pub(super) ctx: &'a Ctx<'c>,
    pub(super) font: Font,
    pub(super) small: Font,
    pub(super) x0: f32,
}

/// A group's title (uppercase) at `y`; returns the y below it.
fn paint_section_title(list: &mut ListArea<'_, '_>, title: &str, font: &Font, y: f32) -> f32 {
    let ctx = list.ctx;
    let px = |v: f32| ctx.px(v);
    let y = y + px(8.0);
    let shaped = list.text.shape_line(&title.to_uppercase(), font);
    draw_text(
        list.layer,
        &shaped,
        list.x0 + px(12.0),
        ds::optical_baseline(y, px(24.0), font),
        ctx.fade(ctx.palette.text_muted),
    );
    y + px(26.0)
}

/// An action's row `r`: (label, icon, shortcut).
fn paint_action(
    list: &mut ListArea<'_, '_>,
    r: Rect,
    (label, icon, shortcut): (&str, &str, &str),
    selected: bool,
) {
    let ctx = list.ctx;
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let icons = list.icons;
    if let Some(id) = icons.id(icon) {
        let s = px(16.0);
        draw_icon(
            list.layer,
            icons,
            id,
            Rect::new(r.min_x() + px(12.0), r.min_y() + (r.size.height - s) / 2.0, s, s),
            ctx.fade(if selected { p.accent } else { p.text_muted }),
        );
    }
    let shaped = list.text.shape_line(label, &list.font);
    draw_text(
        list.layer,
        &shaped,
        r.min_x() + px(40.0),
        ds::optical_baseline(r.min_y(), r.size.height, &list.font),
        ctx.fade(p.text),
    );
    let keys = list.text.shape_line(shortcut, &list.small);
    draw_text(
        list.layer,
        &keys,
        r.max_x() - px(12.0) - keys.width,
        ds::optical_baseline(r.min_y(), r.size.height, &list.small),
        ctx.fade(p.text_muted),
    );
}

/// A file's row `r`: its icon, name and detail (when there's room).
pub(super) fn paint_file(list: &mut ListArea<'_, '_>, r: Rect, file: &HomeFile) {
    let ctx = list.ctx;
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    if let Some((id, color)) = file.icon {
        let s = px(16.0);
        let ir = Rect::new(r.min_x() + px(12.0), r.min_y() + (r.size.height - s) / 2.0, s, s);
        draw_icon(list.layer, list.icons, id, ir, ctx.fade(color));
    }
    let nx = r.min_x() + px(40.0);
    // A long name cut in the middle, its extension kept (its detail then gets no room).
    let shown = ds::ellipsize_file(list.text, &file.name, &list.font, r.max_x() - px(12.0) - nx);
    let name = list.text.shape_line(&shown, &list.font);
    let base = ds::optical_baseline(r.min_y(), r.size.height, &list.font);
    draw_text(list.layer, &name, nx, base, ctx.fade(p.text));
    let dx = nx + name.width + px(10.0);
    let room = r.max_x() - px(12.0) - dx;
    if !file.detail.is_empty() && room > px(30.0) {
        let detail = ds::ellipsize(list.text, &file.detail, &list.small, room);
        let shaped = list.text.shape_line(&detail, &list.small);
        draw_text(list.layer, &shaped, dx, base, ctx.fade(p.text_muted));
    }
}

/// A project's row `r`: its badge (with what it is on the corner), the name with a chip,
/// the path, and "open now" for the current one.
fn paint_project(
    list: &mut ListArea<'_, '_>,
    r: Rect,
    project: &RecentProject,
    current: bool,
    place: Option<&str>,
) {
    let ctx = list.ctx;
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let (layer, text, icons) = (&mut *list.layer, &mut *list.text, list.icons);
    let badge = px(26.0);
    ds::Avatar::new(&project.name, ui_components::tag_colors::project(&project.name))
        .solid()
        .paint(
            layer,
            text,
            ctx,
            Rect::new(
                r.min_x() + px(10.0),
                r.min_y() + (r.size.height - badge) / 2.0,
                badge,
                badge,
            ),
        );
    // What it is: a small mark on the badge's corner, and a chip after the name.
    let (icon, label, tone) = match &project.kind {
        Some(kind) => (kind.icon, kind.label.clone(), ds::Tone::Accent),
        None => ("folder", i18n::t("home.project_local"), ds::Tone::Neutral),
    };
    let mark = px(15.0);
    let corner = Rect::new(
        r.min_x() + px(10.0) + badge - mark + px(6.0),
        r.min_y() + (r.size.height + badge) / 2.0 - mark + px(6.0),
        mark,
        mark,
    );
    let mark_bg = if project.kind.is_some() { p.accent } else { p.popup_background };
    layer.quad(
        Quad::filled(corner, ctx.fade(mark_bg))
            .rounded(mark / 2.0)
            .bordered(ctx.line(), ctx.fade(p.popup_background)),
    );
    if let Some(id) = icons.id(icon) {
        let s = mark - px(5.0);
        let ir =
            Rect::new(corner.min_x() + (mark - s) / 2.0, corner.min_y() + (mark - s) / 2.0, s, s);
        let color = if project.kind.is_some() { Rgba::WHITE } else { p.text_muted };
        draw_icon(layer, icons, id, ir, ctx.fade(color));
    }
    let x = r.min_x() + px(48.0);
    let name = text.shape_line(&project.name, &list.font);
    draw_text(layer, &name, x, r.min_y() + px(22.0), ctx.fade(p.text));
    let chip_room = r.max_x() - px(100.0) - (x + name.width + px(10.0));
    if chip_room > px(40.0) {
        let label = ds::ellipsize(text, &label, &ctx.ui_font(-1.5), chip_room);
        ds::badge(
            layer,
            text,
            ctx,
            ds::BadgeSpec {
                label: &label,
                x: x + name.width + px(10.0),
                mid: r.min_y() + px(17.0),
                align_right: false,
                tone,
            },
        );
    }
    let mut shown = match project.kind.as_ref().and_then(|k| k.detail.as_deref()) {
        Some(from) => format!("{}  ·  {from}", project.display),
        None => project.display.clone(),
    };
    if let Some(place) = place {
        shown = format!("{place}  ·  {shown}");
    }
    let display = ds::ellipsize(text, &shown, &list.small, r.max_x() - x - px(90.0));
    let path = text.shape_line(&display, &list.small);
    draw_text(layer, &path, x, r.min_y() + px(40.0), ctx.fade(p.text_muted));
    if current {
        ds::badge(
            layer,
            text,
            ctx,
            ds::BadgeSpec {
                label: &i18n::t("home.open_now"),
                x: r.max_x() - px(12.0),
                mid: r.min_y() + r.size.height / 2.0,
                align_right: true,
                tone: ds::Tone::Accent,
            },
        );
    }
}

/// A group's row `r`: the chevron, the folder, the name and how many projects it holds.
fn paint_group(list: &mut ListArea<'_, '_>, r: Rect, name: &str, collapsed: bool, count: usize) {
    let ctx = list.ctx;
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let icons = list.icons;
    let mid = r.min_y() + r.size.height / 2.0;
    let s = px(12.0);
    let ir = Rect::new(r.min_x() + px(12.0), mid - s / 2.0, s, s);
    ui_components::tree::chevron(list.layer, icons, ir, !collapsed, ctx.fade(p.text_muted));
    if let Some(id) = icons.id(if collapsed { "folder" } else { "folder-open" }) {
        let s = px(16.0);
        let ir = Rect::new(r.min_x() + px(30.0), mid - s / 2.0, s, s);
        draw_icon(list.layer, icons, id, ir, ctx.fade(p.accent));
    }
    let font = list.font.clone().with_weight(Font::SEMIBOLD);
    let room = r.max_x() - px(56.0) - (r.min_x() + px(54.0));
    let shown = ds::ellipsize(list.text, name, &font, room);
    let shaped = list.text.shape_line(&shown, &font);
    let base = ds::optical_baseline(r.min_y(), r.size.height, &font);
    draw_text(list.layer, &shaped, r.min_x() + px(54.0), base, ctx.fade(p.text));
    let count = list.text.shape_line(&count.to_string(), &list.small);
    let base = ds::optical_baseline(r.min_y(), r.size.height, &list.small);
    draw_text(list.layer, &count, r.max_x() - px(14.0) - count.width, base, ctx.fade(p.text_muted));
}

/// The "New group" button at the right of the history's title `header`; returns it.
fn paint_new_group(list: &mut ListArea<'_, '_>, header: Rect, hovered: bool) -> Rect {
    let px = |v: f32| list.ctx.px(v);
    let label = i18n::t("home.new_group");
    paint_text_button(
        list,
        &label,
        "plus",
        header.max_x() - px(6.0),
        header.min_y() + px(2.0),
        hovered,
    )
}

/// A small text button with an icon ending at `right`, from `top` (the history's "New
/// group", a session's "Reopen"); returns it.
pub(super) fn paint_text_button(
    list: &mut ListArea<'_, '_>,
    label: &str,
    icon_name: &str,
    right: f32,
    top: f32,
    hovered: bool,
) -> Rect {
    let ctx = list.ctx;
    let h = ctx.px(TEXT_BUTTON_H);
    let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
        .icon(icon_name)
        .label(label)
        .rest_tone(ds::Tone::Neutral)
        .hovered(hovered);
    let w = ds::button_width(list.text, ctx, &spec, h);
    let r = Rect::new(right - w, top, w, h);
    ds::button(list.layer, list.text, list.icons, ctx, r, spec);
    r
}

/// The height of a [`paint_text_button`] (logical pixels).
const TEXT_BUTTON_H: f32 = 24.0;

/// Where a drop would go: the row (or title) outlined in the accent color.
fn paint_drop_target(layer: &mut Layer, ctx: &Ctx<'_>, r: Rect) {
    let p = ctx.palette;
    layer.quad(
        Quad::filled(r, ctx.fade(p.accent.with_alpha(0.08)))
            .rounded(ctx.px(ds::radius::MD))
            .bordered(ctx.line().max(1.0), ctx.fade(p.accent.with_alpha(0.7))),
    );
}

/// The dragged project or group: a small card with its name, beside the mouse.
fn paint_dragged(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ctx: &Ctx<'_>,
    name: &str,
    at: Point,
) {
    let lead = ui_components::drag_card::Lead::Icon(icons.id_or_file("folder"), ctx.palette.accent);
    ui_components::drag_card::paint(layer, ctx, text, icons, lead, name, at);
}

/// The guide lines of the expanded groups, faint: from below a group's row, under its
/// chevron, down to its last row's middle, with a branch to each of its rows (stopping
/// short of the row: its hover and selection stay clear).
fn paint_guides(layer: &mut Layer, ctx: &Ctx<'_>, items: &[Item], rows: &[(Rect, usize)]) {
    let color = ctx.fade(ctx.palette.text.with_alpha(0.12));
    let width = ctx.line().max(1.0);
    let depth_of = |i: usize| match items.get(i) {
        Some(Item::Group { depth, .. } | Item::Project { depth, group: Some(_), .. }) => {
            Some(*depth)
        }
        Some(Item::Session { .. }) => Some(0),
        Some(Item::SessionWindow { .. }) => Some(1),
        Some(Item::SessionFile(_)) => Some(2),
        _ => None,
    };
    for (n, &(r, i)) in rows.iter().enumerate() {
        let depth = match items.get(i) {
            Some(Item::Group { depth, collapsed: false, .. }) => depth,
            Some(Item::Session { expanded: true, .. }) => &0,
            Some(Item::SessionWindow { expanded: true, .. }) => &1,
            _ => continue,
        };
        // Under the chevron's middle (`paint_group`: 12 + 12 / 2).
        let x = (r.min_x() + ctx.px(18.0)).round();
        let mut last = None;
        for &(child, j) in &rows[n + 1..] {
            match depth_of(j) {
                Some(d) if d == depth + 1 => {
                    let mid = (child.min_y() + child.size.height / 2.0).round();
                    let end = child.min_x() - ctx.px(3.0);
                    if end > x + width {
                        layer.quad(Quad::filled(Rect::new(x, mid, end - x, width), color));
                    }
                    last = Some(mid);
                }
                Some(d) if d > depth + 1 => {}
                _ => break,
            }
        }
        if let Some(bottom) = last.filter(|b| *b > r.max_y()) {
            layer.quad(Quad::filled(
                Rect::new(x, r.max_y(), width, bottom - r.max_y() + width),
                color,
            ));
        }
    }
}

/// Where a drop between two rows goes: an accent line from `x` to `right` at `y`, with a
/// small ring at its start.
fn paint_drop_line(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, y: f32, right: f32) {
    let p = ctx.palette;
    let h = ctx.px(2.0).max(1.0);
    let ring = ctx.px(8.0);
    layer.quad(
        Quad::filled(
            Rect::new(x - ring / 2.0, y - ring / 2.0, ring, ring),
            ctx.fade(p.popup_background),
        )
        .rounded(ring / 2.0)
        .bordered(h, ctx.fade(p.accent)),
    );
    layer.quad(
        Quad::filled(
            Rect::new(x + ring / 2.0, (y - h / 2.0).round(), right - x - ring / 2.0, h),
            ctx.fade(p.accent),
        )
        .rounded(h / 2.0),
    );
}

fn paint_row_bg(layer: &mut Layer, ctx: &Ctx<'_>, r: Rect, selected: bool, hovered: bool) {
    ds::row_highlight(layer, ctx, r, selected, hovered);
}

fn hints() -> [String; 3] {
    [i18n::t("home.hint_move"), i18n::t("home.hint_open"), i18n::t("home.hint_close")]
}
