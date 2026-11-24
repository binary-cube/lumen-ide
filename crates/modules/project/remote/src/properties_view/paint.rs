//! The dialog painted: its tabs, the icon and the name, the rows (location, size, date,
//! owner, group), the permissions' grid and octal, the recursive choice, the failures.

use super::*;

/// The dialog's size (logical pixels).
pub(super) const WIDTH: f32 = 560.0;
pub(super) const HEIGHT: f32 = 600.0;
/// A label's column.
pub(super) const LABEL_W: f32 = 112.0;
pub(super) const ROW_H: f32 = 30.0;

impl PropertiesView {
    pub(super) fn paint_body(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        b: Rect,
        focused: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let pad = px(20.0);
        let x0 = b.min_x() + pad;
        let x1 = b.max_x() - pad;
        self.footer_top = b.max_y();
        let body = b;
        if let Phase::Failed(failed) = &self.phase {
            paint_failures(layer, text, ds, body, failed);
            return;
        }
        if let Some(why) = &self.failed {
            ds::empty_state(layer, text, ds, body, &t("remote.props_failed"), Some(why));
            return;
        }
        if self.loaded.is_none() {
            let reading = t("remote.props_reading");
            let loader = ds::Loader::with_text(&reading);
            let w = loader.width(text, ds);
            let mid = body.min_y() + body.size.height / 2.0;
            ds::loader(layer, text, ds, body.min_x() + (body.size.width - w) / 2.0, mid, &loader);
            return;
        }
        let font = ds.ui_font(-0.5);
        let strong = ds.ui_font(0.5).with_weight(Font::SEMIBOLD);
        let muted = p.text_muted;
        // The header: the icon, the name, how many.
        let mut y = b.min_y() + px(12.0);
        let s = px(28.0);
        if let Some(id) = icons.id(self.icon()) {
            ui_kit::draw_icon(layer, icons, id, Rect::new(x0, y, s, s), p.accent);
        }
        // On the right: the tabs (the Checksum one only for files).
        let mut name_room = x1 - x0 - s - px(12.0);
        if !self.sums.rows.is_empty() {
            let labels = [t("remote.props_tab_general"), t("remote.props_tab_checksum")];
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let hovered = match self.hover {
                Some(Hit::Tab(Tab::General)) => Some(0),
                Some(Hit::Tab(Tab::Checksum)) => Some(1),
                _ => None,
            };
            let selected = usize::from(self.tab == Tab::Checksum);
            let mid = y + s / 2.0;
            let spec = ds::SegmentedSpec { right: x1, mid, labels: &labels, selected, hovered };
            let tabs = ds::segmented(layer, text, ds, spec);
            for (r, tab) in tabs.iter().zip([Tab::General, Tab::Checksum]) {
                self.hits.push((*r, Hit::Tab(tab)));
            }
            name_room =
                tabs.first().map_or(name_room, |r| r.min_x() - px(16.0) - x0 - s - px(12.0));
        }
        let name = ds::ellipsize_file(text, &self.subject(), &strong, name_room.max(0.0));
        let shaped = text.shape_line(&name, &strong);
        draw_text(layer, &shaped, x0 + s + px(12.0), ds::optical_baseline(y, s, &strong), p.text);
        y += s + px(14.0);
        ds::divider(layer, ds, x0, x1, y);
        y += px(8.0);
        if self.tab == Tab::Checksum {
            self.paint_checksums(
                &mut ui_components::LayerPainter::new(layer, text, icons, *ds),
                (x0, x1),
                y,
            );
            return;
        }
        let vx = x0 + px(LABEL_W);
        let row = |layer: &mut render_api::Layer,
                   text: &mut text_layout::TextSystem,
                   y: f32,
                   label: &str| {
            let shaped = text.shape_line(label, &font);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(ROW_H), &font), muted);
        };
        let value = |layer: &mut render_api::Layer,
                     text: &mut text_layout::TextSystem,
                     y: f32,
                     v: &str,
                     right: f32| {
            let shown = ds::ellipsize_middle(text, v, &font, right - vx);
            let shaped = text.shape_line(&shown, &font);
            draw_text(layer, &shaped, vx, ds::optical_baseline(y, px(ROW_H), &font), p.text);
        };
        // Where, how big, when.
        row(layer, text, y, &t("remote.props_location"));
        value(layer, text, y, &self.location(), x1);
        y += px(ROW_H);
        row(layer, text, y, &t("remote.props_size"));
        let has_folders = self.props().iter().any(|p| p.is_dir);
        let mut right = x1;
        if has_folders {
            let label = if self.measuring.is_some() {
                t("remote.props_measure_stop")
            } else {
                t("remote.props_measure")
            };
            let w = ds::primary_width(text, ds, &[&label]);
            let h = px(26.0);
            let r = Rect::new(x1 - w, y + (px(ROW_H) - h) / 2.0, w, h);
            ds::secondary_button(
                layer,
                text,
                ds,
                r,
                &label,
                true,
                self.hover == Some(Hit::Measure),
            );
            self.hits.push((r, Hit::Measure));
            right = r.min_x() - px(8.0);
            if self.measuring.is_some() {
                let loader = ds::Loader { size: 12.0, ..ds::Loader::default() };
                ds::loader(layer, text, ds, right - px(12.0), y + px(ROW_H) / 2.0, &loader);
                right -= px(20.0);
            }
        }
        value(layer, text, y, &self.size_text(), right);
        y += px(ROW_H);
        row(layer, text, y, &t("remote.props_modified"));
        value(layer, text, y, &self.modified_text(), x1);
        y += px(ROW_H) + px(6.0);
        ds::divider(layer, ds, x0, x1, y);
        y += px(10.0);
        // The owner and the group.
        for choice in [Choice::Owner, Choice::Group] {
            let key = match choice {
                Choice::Owner => "remote.props_owner",
                Choice::Group => "remote.props_group",
            };
            row(layer, text, y, &t(key));
            // As wide as the row: its list opens as wide.
            let r = Rect::new(vx, y, x1 - vx, px(ROW_H));
            let enabled = self.may_change(choice);
            let not_allowed = t("remote.props_not_allowed");
            let spec = ds::DropdownSpec {
                value: &self.label_of(choice),
                open: self.picking == Some(Pick::Account(choice)),
                hovered: self.hover == Some(Hit::Choice(choice)),
                enabled,
                // Why it can't be chosen.
                note: (!self.allowed(choice)).then_some(not_allowed.as_str()),
                placeholder: None,
            };
            ds::dropdown(layer, text, icons, ds, r, spec);
            if enabled {
                self.hits.push((r, Hit::Choice(choice)));
            }
            self.anchors.push((Pick::Account(choice), r));
            y += px(ROW_H) + px(8.0);
        }
        y += px(4.0);
        ds::divider(layer, ds, x0, x1, y);
        y += px(12.0);
        // The permissions: the grid, the special bits, the octal.
        let Some(edit) = self.edit else {
            let note = t("remote.props_no_mode");
            let note = ds::ellipsize(text, &note, &font, x1 - x0);
            let shaped = text.shape_line(&note, &font);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(ROW_H), &font), muted);
            self.paint_recursive(layer, text, ds, x0, y + px(ROW_H) + px(10.0));
            return;
        };
        let col = px(72.0);
        let heads = ["remote.props_read", "remote.props_write", "remote.props_execute"];
        for (i, key) in heads.iter().enumerate() {
            let shaped = text.shape_line(&t(key), &font);
            let cx = vx + col * i as f32;
            draw_text(layer, &shaped, cx, ds::optical_baseline(y, px(22.0), &font), muted);
        }
        y += px(24.0);
        let classes = [
            (Class::Owner, "remote.props_class_owner"),
            (Class::Group, "remote.props_class_group"),
            (Class::Others, "remote.props_class_others"),
        ];
        let check = |state: Tri| match state {
            Tri::On => Check::On,
            Tri::Off => Check::Off,
            Tri::Mixed => Check::Mixed,
        };
        let enabled = self.loaded.as_ref().is_some_and(|l| l.caps.chmod);
        // Each class's row, its special bit on its right (as WinSCP lays them).
        let specials = [
            (Special::SetUid, "remote.props_setuid"),
            (Special::SetGid, "remote.props_setgid"),
            (Special::Sticky, "remote.props_sticky"),
        ];
        let sx = vx + col * 3.0 + px(16.0);
        for ((class, key), (special, special_key)) in classes.into_iter().zip(specials) {
            row(layer, text, y, &t(key));
            let mid = y + px(ROW_H) / 2.0;
            for (i, access) in Access::ALL.into_iter().enumerate() {
                let bit = Bit::Access(class, access);
                let x = vx + col * i as f32;
                let r = ds::checkbox(layer, text, ds, x, mid, check(edit.state(bit)), enabled);
                let area = Rect::new(r.min_x() - px(6.0), y, px(36.0), px(ROW_H));
                if enabled {
                    self.hits.push((area, Hit::Bit(bit)));
                }
            }
            let bit = Bit::Special(special);
            let r = ds::checkbox(layer, text, ds, sx, mid, check(edit.state(bit)), enabled);
            let shaped = text.shape_line(&t(special_key), &font);
            let lx = r.max_x() + px(8.0);
            draw_text(layer, &shaped, lx, ds::optical_baseline(y, px(ROW_H), &font), p.text);
            if enabled {
                self.hits
                    .push((Rect::new(sx, y, lx + shaped.width - sx, px(ROW_H)), Hit::Bit(bit)));
            }
            y += px(ROW_H);
        }
        y += px(8.0);
        row(layer, text, y, &t("remote.props_octal"));
        let field = Rect::new(vx, y, px(90.0), px(ROW_H));
        let style = ds::field_style(ds, ds.code_font(-0.5), self.octal_invalid());
        let placeholder =
            if edit.octal().is_none() { t("remote.props_mixed") } else { String::new() };
        self.octal.paint(layer, text, field, self.typing() && focused, &placeholder, &style);
        if enabled {
            self.hits.push((field, Hit::Octal));
            self.octal_area = Some(field);
        }
        // Beside it: as `ls -l` writes it, or why the octal isn't taken.
        let (note, color) = if self.octal_invalid() {
            (t("remote.props_octal_bad"), p.danger)
        } else {
            (edit.whole().map(|m| m.symbolic()).unwrap_or_default(), muted)
        };
        let small = ds.code_font(-0.5);
        let nx = field.max_x() + px(12.0);
        let note = ds::ellipsize(text, &note, &small, x1 - nx);
        let shaped = text.shape_line(&note, &small);
        draw_text(layer, &shaped, nx, ds::optical_baseline(y, px(ROW_H), &small), color);
        self.paint_recursive(layer, text, ds, x0, y + px(ROW_H) + px(12.0));
    }

    /// With a folder chosen: "Apply recursively" and "Add X to folders" (only then).
    fn paint_recursive(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        x0: f32,
        y: f32,
    ) {
        if !self.has_folders() {
            return;
        }
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let mut x = x0;
        let boxes = [
            (Hit::Recursive, self.recursive, true, "remote.props_recursive"),
            (Hit::XOnFolders, self.x_on_folders, self.recursive, "remote.props_x_on_folders"),
        ];
        for (hit, on, enabled, key) in boxes {
            let mid = y + px(ROW_H) / 2.0;
            let state = if on && enabled { Check::On } else { Check::Off };
            let r = ds::checkbox(layer, text, ds, x, mid, state, enabled);
            let shaped = text.shape_line(&t(key), &font);
            let lx = r.max_x() + px(8.0);
            let color = if enabled { ds.palette.text } else { ds.palette.text_muted };
            draw_text(layer, &shaped, lx, ds::optical_baseline(y, px(ROW_H), &font), color);
            let area = Rect::new(x, y, lx + shaped.width - x, px(ROW_H));
            if enabled {
                self.hits.push((area, hit));
            }
            x = area.max_x() + px(24.0);
        }
    }
}

/// What the server refused, each and why (a lost connection: why only).
fn paint_failures(
    layer: &mut render_api::Layer,
    text: &mut text_layout::TextSystem,
    ds: &Ctx<'_>,
    body: Rect,
    failed: &[(String, String)],
) {
    let px = |v: f32| ds.px(v);
    let p = ds.palette;
    let x0 = body.min_x() + px(20.0);
    let width = body.size.width - px(40.0);
    let title = tr!("remote.props_refused", count = failed.len());
    let mut y = ds::section_title(layer, text, ds, x0, body.min_y() + px(16.0), &title, None);
    y += px(10.0);
    let mono = ds.code_font(-1.0);
    let small = ds.ui_font(-1.0);
    let line_h = px(40.0);
    let fits = ((body.max_y() - y) / line_h).floor().max(1.0) as usize;
    let shown = if failed.len() > fits { fits - 1 } else { failed.len() };
    for (path, why) in &failed[..shown] {
        if !path.is_empty() {
            let path = ds::ellipsize_path(text, &format!("/{path}"), &mono, width);
            let shaped = text.shape_line(&path, &mono);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &mono), p.text);
        }
        let why = ds::ellipsize(text, why, &small, width);
        let shaped = text.shape_line(&why, &small);
        draw_text(
            layer,
            &shaped,
            x0,
            ds::optical_baseline(y + px(18.0), px(20.0), &small),
            p.danger,
        );
        y += line_h;
    }
    if shown < failed.len() {
        let more = tr!("remote.props_more", count = failed.len() - shown);
        let shaped = text.shape_line(&more, &small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &small), p.text_muted);
    }
}
