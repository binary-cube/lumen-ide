//! The list's rows as drawn: a missing favorite's (dimmed, "no longer exists"), a folder's
//! (its chevron, its icon, its name, how many sessions it holds; lit as a drop's target), a
//! session's (its agent's icon, its title, its marks, its star, its age or its "⋮"; under a
//! folder, set in), and "Show more…" after the last.

use kernel::{Rect, Rgba};
use render_api::Layer;
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx, Tone, button_size};
use ui_components::{ViewerPaint, tree};
use ui_kit::draw_text;

use super::{AGE_COLUMN, DropOn, ROW, Renamed, Row, SessionsPanel, Star, around, now};
use i18n::t;

/// A sleeping session's sign on its row (its terminal's tab shows the same).
pub(super) const ASLEEP_ICON: &str = "moon";
/// What a session's terminal holds on its row: the moon's size, and the room after it
/// (logical pixels).
const HELD_ICON: f32 = 13.0;
const HELD_GAP: f32 = 6.0;
/// The room between a row's memory (or moon) and its star's button (logical pixels): the
/// star's hover fill reaches right up to where the memory would otherwise end.
const BESIDE_STAR: f32 = 6.0;
/// The most memory labels kept shaped (a list's sessions with a tab open).
const HELD_LABELS: usize = 64;

/// How far a folder's sessions are set in (logical pixels).
const INDENT: f32 = 18.0;
/// A row's fills (alphas): under the mouse (of the text), dragged (of the text), tinted by
/// its color label, and a folder lit as a drop's target (of the accent, and its outline).
const ROW_HOVER: f32 = 0.06;
const DRAGGED_SHADE: f32 = 0.1;
const LABEL_TINT: f32 = 0.18;
const DROP_FILL: f32 = 0.14;
const DROP_OUTLINE: f32 = 0.6;
/// The unread dot's diameter (logical pixels).
const UNREAD_DOT: f32 = 6.0;

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
        self.holds.clear();
        // Shown: what the sessions' terminals hold is asked meanwhile.
        crate::follow::panel_drawn();
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
        // Lit as a drop's target (outlined in the accent), or under the mouse.
        let highlight = ds::RowHighlight::new(target, self.hovered_folder == Some(f))
            .selected_alpha(DROP_FILL)
            .hover(ds::HoverFill::Faint(ROW_HOVER));
        let highlight =
            if target { highlight.outline(p.accent.with_alpha(DROP_OUTLINE)) } else { highlight };
        highlight.paint(pen.layer, ds, r);
        let open = folder.open || self.searched();
        let s = px(14.0);
        let mid = top + (self.row_h - s) / 2.0;
        let at = Rect::new(r.min_x() + px(4.0), mid, s, s);
        tree::chevron(pen.layer, pen.icons, at, open, p.text_muted);
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

    /// `memory` as its row says it (`412 MB`), shaped once for the value and the font's size
    /// (kept while the value stays: a frame formats and shapes nothing for it).
    fn held_label(
        &mut self,
        pen: &mut Pen<'_, '_>,
        memory: u64,
    ) -> std::sync::Arc<text_layout::ShapedLine> {
        let key = (memory, pen.small.size.to_bits());
        if let Some((_, shaped)) = self.held_labels.iter().find(|(k, _)| *k == key) {
            return shaped.clone();
        }
        let shaped = pen.text.shape_line(&i18n::bytes(memory), &pen.small);
        if self.held_labels.len() >= HELD_LABELS {
            self.held_labels.clear();
        }
        self.held_labels.push((key, shaped.clone()));
        shaped
    }

    /// What a session's terminal holds, ending at `right` on the row at `top`: its memory
    /// (muted), or a moon while it sleeps. Where it starts.
    fn paint_held(
        &mut self,
        pen: &mut Pen<'_, '_>,
        state: contracts::terminal::TerminalState,
        (right, top): (f32, f32),
    ) -> f32 {
        let (ds, p) = (pen.ds, pen.ds.palette);
        let px = |v: f32| ds.px(v);
        let s = px(HELD_ICON);
        let rect = if state.asleep {
            let at = Rect::new(right - s, top + (self.row_h - s) / 2.0, s, s);
            if let Some(id) = pen.icons.id(ASLEEP_ICON) {
                ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted);
            }
            at
        } else {
            let Some(memory) = state.memory else { return right };
            let label = self.held_label(pen, memory);
            let base = ds::optical_baseline(top, self.row_h, &pen.small);
            draw_text(pen.layer, &label, right - label.width, base, p.text_muted);
            Rect::new(right - label.width, top, label.width, self.row_h)
        };
        self.holds.push((rect, state));
        rect.min_x()
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
            let tint = Rgba::rgb(label.rgb()).with_alpha(LABEL_TINT);
            ds::RowHighlight::new(true, false).selected_fill(tint).paint(pen.layer, ds, r);
        }
        let hovered = self.hovered == Some(i);
        let dragged = self.dragging == Some(i);
        let shade = if dragged { DRAGGED_SHADE } else { ROW_HOVER };
        ds::RowHighlight::new(false, hovered || dragged)
            .hover(ds::HoverFill::Faint(shade))
            .paint(pen.layer, ds, r);
        // Unread: a dot at its start.
        if mark.as_ref().is_some_and(|m| m.unread) {
            let center = kernel::Point::new(
                r.min_x() + indent + px(1.0 + UNREAD_DOT / 2.0),
                top + self.row_h / 2.0,
            );
            ds::dot(pen.layer, ds, center, UNREAD_DOT, p.accent);
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
        } else {
            // Room between what it holds and the star's button (its hover reaches here).
            right -= px(BESIDE_STAR);
        }
        // What its terminal holds (a tab open): its memory, or a moon while it sleeps.
        if let Some(state) = crate::follow::state_of(agent, &session.id) {
            right = self.paint_held(pen, state, (right, top)) - px(HELD_GAP);
        }
        let at = Rect::new(column - px(20.0), top + (self.row_h - s) / 2.0, s, s);
        let star = around(at, px(5.0));
        let favorite = self.favorite.get(i).copied().unwrap_or(false);
        let lit = self.hover_star == Some(Star::Session(i));
        if favorite || lit || hovered {
            let mut spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon(crate::FAVORITE_ICON)
                .rest_tone(Tone::Neutral)
                .hovered(lit);
            if favorite || lit {
                spec = spec.tone(Tone::Accent);
            }
            ds::button(pen.layer, pen.text, pen.icons, pen.ds, star, spec);
        }
        self.stars.push((star, Star::Session(i)));
        // Under the mouse: its "⋮" in the age's place.
        if hovered {
            let at = ds::square_button(
                pen.ds,
                (r.max_x() - px(4.0) - px(button_size::SMALL), top + self.row_h / 2.0),
                button_size::SMALL,
            );
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon("more-vertical")
                .rest_tone(Tone::Neutral)
                .hovered(self.hover_more);
            ds::button(pen.layer, pen.text, pen.icons, pen.ds, at, spec);
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
