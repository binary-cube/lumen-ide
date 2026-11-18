//! The list's rows as drawn: a missing favorite's (dimmed, "no longer exists"), a folder's
//! (its chevron, its icon, its name, how many sessions it holds; lit as a drop's target), a
//! session's (its agent's icon, its title, its marks, its star, its age or its "⋮"; under a
//! folder, set in), and "Show more…" after the last.

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ViewerPaint;
use ui_components::ds::{self, Ctx, radius};
use ui_kit::draw_text;

use super::{AGE_COLUMN, DropOn, ROW, Renamed, Row, SessionsPanel, Star, around, now};
use i18n::t;

/// How far a folder's sessions are set in (logical pixels).
const INDENT: f32 = 18.0;

/// What every row is drawn with: the layer, the text, the fonts, the time now.
struct Pen<'a, 'b> {
    layer: &'a mut Layer,
    text: &'a mut TextSystem,
    icons: &'a icons::Icons,
    ds: &'a Ctx<'b>,
    font: Font,
    small: Font,
    now: i64,
}

impl SessionsPanel {
    pub(super) fn paint_rows(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        self.row_h = px(ROW);
        // "Show more…" only for the list as read (a search looks through them all).
        let more = self.more && self.query.text().trim().is_empty();
        let count = self.rows.len() + usize::from(more);
        self.max_scroll = (count as f32 * self.row_h + px(8.0) - self.list.size.height).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        let scroll = self.scroll.round();
        let layer = ctx.scene.push_layer(Some(self.list));
        let mut pen = Pen {
            layer,
            text: ctx.text,
            icons: ctx.icons,
            ds,
            font: ds.ui_font(-0.5),
            small: ds.ui_font(-1.0),
            now: now(),
        };
        let first = (scroll / self.row_h) as usize;
        let visible = (self.list.size.height / self.row_h) as usize + 2;
        self.stars.clear();
        self.more_button = None;
        let rows: Vec<(usize, Row)> =
            self.rows.iter().copied().enumerate().skip(first).take(visible).collect();
        for (n, row) in rows {
            let top = self.list.min_y() + n as f32 * self.row_h - scroll;
            match row {
                Row::Missing(m) => self.paint_missing(&mut pen, m, top),
                Row::Folder(f) => self.paint_folder(&mut pen, f, top),
                Row::Session { i, nested } => self.paint_session(&mut pen, i, nested, top),
            }
        }
        self.more_rect = None;
        if more {
            let top = self.list.min_y() + self.rows.len() as f32 * self.row_h - scroll;
            let label = pen.text.shape_line(&t("agents.show_more"), &pen.font);
            let x = self.list.min_x() + px(40.0);
            let r = Rect::new(x - px(6.0), top, label.width + px(12.0), self.row_h);
            let base = ds::optical_baseline(top, self.row_h, &pen.font);
            draw_text(pen.layer, &label, x, base, p.text_muted);
            self.more_rect = Some(r);
        }
        let content = self.max_scroll + self.list.size.height;
        self.bar.paint(ctx.scene, p, ctx.scale, self.list, content, scroll);
    }

    /// A row's area (inside the list's margins).
    fn row_rect(&self, ds: &Ctx<'_>, top: f32) -> Rect {
        let px = |v: f32| ds.px(v);
        Rect::new(self.list.min_x() + px(8.0), top, self.list.size.width - px(16.0), self.row_h)
    }

    /// A favorite whose session is gone, dimmed: "no longer exists" (its star takes it off
    /// the list).
    fn paint_missing(&mut self, pen: &mut Pen<'_, '_>, m: usize, top: f32) {
        let (ds, p) = (pen.ds, pen.ds.palette);
        let px = |v: f32| ds.px(v);
        let Some(gone) = self.missing.get(m) else { return };
        let s = px(14.0);
        if let Some(id) = pen.icons.id(crate::FAVORITE_ICON) {
            let at = Rect::new(self.list.min_x() + px(18.0), top + (self.row_h - s) / 2.0, s, s);
            let lit = self.hover_star == Some(Star::Missing(m));
            let color = if lit { p.accent } else { p.text_muted.with_alpha(0.5) };
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, color);
            self.stars.push((around(at, px(5.0)), Star::Missing(m)));
        }
        let x = self.list.min_x() + px(40.0);
        let note = pen.text.shape_line(&t("agents.favorite_gone"), &pen.small);
        let right = self.list.max_x() - px(18.0);
        let base = ds::optical_baseline(top, self.row_h, &pen.small);
        draw_text(pen.layer, &note, right - note.width, base, p.warning);
        let room = (right - note.width - px(12.0) - x).max(0.0);
        let title = ds::ellipsize(pen.text, &gone.title, &pen.font, room);
        let title = pen.text.shape_line(&title, &pen.font);
        let base = ds::optical_baseline(top, self.row_h, &pen.font);
        draw_text(pen.layer, &title, x, base, p.text_muted);
    }

    /// A folder: its chevron, its icon (open or closed), its name (or the field renaming
    /// it), how many sessions it holds; lit while a session dragged would drop into it.
    fn paint_folder(&mut self, pen: &mut Pen<'_, '_>, f: usize, top: f32) {
        let (ds, p) = (pen.ds, pen.ds.palette);
        let px = |v: f32| ds.px(v);
        let Some(folder) = self.folders.get(f) else { return };
        let r = self.row_rect(ds, top);
        let target = matches!(&self.drop_on, Some(DropOn::Folder(id)) if *id == folder.id);
        if target {
            pen.layer.quad(
                Quad::filled(r, p.accent.with_alpha(0.14))
                    .rounded(px(radius::MD))
                    .bordered(ds.line(), p.accent.with_alpha(0.6)),
            );
        } else if self.hovered_folder == Some(f) {
            pen.layer.quad(Quad::filled(r, p.text.with_alpha(0.06)).rounded(px(radius::MD)));
        }
        let open = folder.open || self.searched();
        let s = px(14.0);
        let mid = top + (self.row_h - s) / 2.0;
        let chevron = if open { "chevron-down" } else { "chevron-right" };
        if let Some(id) = pen.icons.id(chevron) {
            let at = Rect::new(r.min_x() + px(4.0), mid, s, s);
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted);
        }
        let icon = if open { "folder-open" } else { "folder" };
        if let Some(id) = pen.icons.id(icon) {
            let at = Rect::new(r.min_x() + px(22.0), mid, s, s);
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted);
        }
        let count = self.folder_counts.get(f).copied().unwrap_or(0).to_string();
        let count = pen.text.shape_line(&count, &pen.small);
        let right = r.max_x() - px(10.0);
        let base = ds::optical_baseline(top, self.row_h, &pen.small);
        draw_text(pen.layer, &count, right - count.width, base, p.text_muted);
        let x = r.min_x() + px(44.0);
        let room = (right - count.width - px(12.0) - x).max(0.0);
        let id = folder.id.clone();
        if self.paint_rename_field(pen, &Renamed::Folder(id), (x, top, room)) {
            return;
        }
        let Some(folder) = self.folders.get(f) else { return };
        let name = ds::ellipsize(pen.text, &folder.name, &pen.font, room);
        let name = pen.text.shape_line(&name, &pen.font);
        let base = ds::optical_baseline(top, self.row_h, &pen.font);
        draw_text(pen.layer, &name, x, base, p.text);
    }

    /// The field renaming `target`, if it's renamed now, in its title's place (`x`, `top`,
    /// `room` wide): `true` if it was drawn.
    fn paint_rename_field(
        &mut self,
        pen: &mut Pen<'_, '_>,
        target: &Renamed,
        (x, top, room): (f32, f32, f32),
    ) -> bool {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(renaming) = self.renaming.as_mut().filter(|n| n.target == *target) else {
            return false;
        };
        let field = Rect::new(x - px(6.0), top + px(3.0), room + px(6.0), self.row_h - px(6.0));
        let mut style = ds::field_style(ds, pen.font.clone(), false);
        style.padding = px(6.0);
        renaming.input.paint(pen.layer, pen.text, field, true, "", &style);
        renaming.field = field;
        true
    }

    /// A session: its label's tint, its unread dot, its agent's icon, its title (dimmed when
    /// done), its pin, its star, its age — or, under the mouse, its "⋮".
    fn paint_session(&mut self, pen: &mut Pen<'_, '_>, i: usize, nested: bool, top: f32) {
        let (ds, p) = (pen.ds, pen.ds.palette);
        let px = |v: f32| ds.px(v);
        let Some(session) = self.sessions.get(i) else { return };
        let (agent, active) = (session.agent, session.active);
        let r = self.row_rect(ds, top);
        let indent = if nested { px(INDENT) } else { 0.0 };
        let mark = self.marked.get(i).cloned().flatten();
        let done = mark.as_ref().is_some_and(|m| m.done);
        // Its color label: the row tinted.
        if let Some(label) = mark.as_ref().and_then(|m| m.label) {
            let tint = Rgba::rgb(label.rgb()).with_alpha(0.18);
            pen.layer.quad(Quad::filled(r, tint).rounded(px(radius::MD)));
        }
        let hovered = self.hovered == Some(i);
        let dragged = self.dragging == Some(i);
        if hovered || dragged {
            let shade = if dragged { 0.1 } else { 0.06 };
            pen.layer.quad(Quad::filled(r, p.text.with_alpha(shade)).rounded(px(radius::MD)));
        }
        // Unread: a dot at its start.
        if mark.as_ref().is_some_and(|m| m.unread) {
            let d = px(6.0);
            let dot = Rect::new(r.min_x() + indent + px(1.0), top + (self.row_h - d) / 2.0, d, d);
            pen.layer.quad(Quad::filled(dot, p.accent).rounded(d));
        }
        let s = px(14.0);
        let faded = |c: Rgba| if done { c.with_alpha(c.a * 0.55) } else { c };
        let icon = self.icons.iter().find(|(id, _)| *id == agent).map(|(_, i)| *i);
        if let Some(id) = icon.and_then(|i| pen.icons.id(i)) {
            let at = Rect::new(r.min_x() + indent + px(10.0), top + (self.row_h - s) / 2.0, s, s);
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, faded(p.text_muted));
        }
        let age = crate::age::short(pen.now.saturating_sub(active));
        let age = pen.text.shape_line(&age, &pen.small);
        // Its star in one column for every row, left of the ages' (its place always kept:
        // titles don't move): a favorite's lit; on the row under the mouse, the others' too,
        // to click.
        let column = r.max_x() - px(10.0) - px(AGE_COLUMN).max(age.width);
        let mut right = column - px(26.0);
        // Pinned: a pin left of its star.
        if mark.as_ref().is_some_and(|m| m.pinned)
            && let Some(id) = pen.icons.id("pin")
        {
            let at = Rect::new(column - px(42.0), top + (self.row_h - s) / 2.0, s, s);
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted);
            right -= px(22.0);
        }
        if let Some(id) = pen.icons.id(crate::FAVORITE_ICON) {
            let at = Rect::new(column - px(20.0), top + (self.row_h - s) / 2.0, s, s);
            let favorite = self.favorite.get(i).copied().unwrap_or(false);
            let lit = self.hover_star == Some(Star::Session(i));
            if favorite || lit {
                ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.accent);
            } else if hovered {
                ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted.with_alpha(0.6));
            }
            self.stars.push((around(at, px(5.0)), Star::Session(i)));
        }
        // Under the mouse: its "⋮" in the age's place.
        if hovered && let Some(id) = pen.icons.id("more-vertical") {
            let b = px(22.0);
            let at = Rect::new(r.max_x() - px(4.0) - b, top + (self.row_h - b) / 2.0, b, b);
            if self.hover_more {
                let lit = Quad::filled(at, p.text.with_alpha(0.08)).rounded(px(radius::SM));
                pen.layer.quad(lit);
            }
            let icon = Rect::new(at.min_x() + (b - s) / 2.0, at.min_y() + (b - s) / 2.0, s, s);
            ui_kit::draw_icon(pen.layer, pen.icons, id, icon, p.text_muted);
            self.more_button = Some((at, i));
        } else {
            let base = ds::optical_baseline(top, self.row_h, &pen.small);
            draw_text(pen.layer, &age, r.max_x() - px(10.0) - age.width, base, p.text_muted);
        }
        let x = r.min_x() + indent + px(32.0);
        let room = (right - x).max(0.0);
        let Some(session) = self.sessions.get(i) else { return };
        let target = Renamed::Session(session.agent, session.id.clone());
        // Renamed here: a field in the title's place.
        if self.paint_rename_field(pen, &target, (x, top, room)) {
            return;
        }
        let Some(session) = self.sessions.get(i) else { return };
        let title = ds::ellipsize(pen.text, &session.title, &pen.font, room);
        let title = pen.text.shape_line(&title, &pen.font);
        let color = if done { p.text_muted } else { p.text };
        let base = ds::optical_baseline(top, self.row_h, &pen.font);
        draw_text(pen.layer, &title, x, base, color);
    }
}
