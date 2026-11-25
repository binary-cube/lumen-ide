//! The Push… dialog's view: its form drawn, what's clicked and typed.

use std::sync::Arc;

use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Layer;
use text_layout::TextSystem;
use ui_components::control::{Control, Controls, OneClick, Thumb};
use ui_components::ds::{self, Check, Ctx};
use ui_components::picker::{Picker, PickerEvent, PickerItem};
use ui_components::{
    KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint, WithHost,
};
use ui_kit::draw_text;
use version_control::Repository;
use version_control::application::SyncOp;

use super::parts::NameFields;
use super::{PushForm, PushRequest};
use crate::parts::{Tune, Tuned};

/// The rows' height and how many are shown before the table scrolls (logical pixels, rows).
const ROW_H: f32 = 34.0;
/// The room between the columns' line and the first row (logical pixels).
const TABLE_TOP: f32 = 6.0;
pub(super) const ROWS_SHOWN: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    Remote,
    Push(usize),
    Name(usize),
    Track(usize),
    All,
    Tags,
    Force,
}

/// The footer's buttons (their commands).
const GO: &str = "push-go";
const CANCEL: &str = "push-cancel";

#[derive(Default)]
pub struct PushDialog {
    repo: Option<Arc<dyn Repository>>,
    form: Option<PushForm>,
    picker: Picker,
    remote_field: Rect,
    /// The row typed in.
    typing: Option<usize>,
    /// The first row shown.
    first: usize,
    /// The wheel's movement not yet a whole row; a row's height (physical pixels).
    wheel: f32,
    row_h: f32,
    /// A force push asked for once: the next "go" sends it.
    confirming: bool,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    done: bool,
    /// Where the mouse was last (the wheel goes to the part there).
    pointer: Point,
    /// The table's scrollbar (more branches than shown): the app's, a row at a time.
    bar: ui_components::EdgeScrollbar,
    /// Where it goes: along the table, in the margin right of it; and how many rows.
    bar_area: Option<(Rect, usize)>,
    /// Its parts' state of use (see [`PushDialog::with_controls`]).
    controls: Controls,
}

/// The places of its parts in [`PushDialog::with_controls`].
const REMOTES: usize = 0;
const NAMES: usize = 2;

impl std::fmt::Debug for PushDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushDialog").field("done", &self.done).finish_non_exhaustive()
    }
}

impl PushDialog {
    /// The rows scrolled to `offset` (the scrollbar's, a row being `row_h`): the nearest
    /// row first, within the ends. `true` if they moved.
    fn scroll_to(&mut self, offset: f32) -> bool {
        let Some(form) = &self.form else { return false };
        let last = form.lines.len().saturating_sub(ROWS_SHOWN);
        let first =
            if self.row_h > 0.0 { (offset / self.row_h).round().max(0.0) as usize } else { 0 };
        let first = first.min(last);
        let moved = first != self.first;
        self.first = first;
        moved
    }

    /// Its parts used as controls, the topmost first, with their state of use: the remotes'
    /// list (over everything while open, a press on it a single click), the table's scrollbar
    /// (its offset in rows: where the thumb goes, the nearest row comes first), the rows'
    /// names to type ([`NameFields`]).
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let shown = self.row_h * self.first as f32;
        let mut offset = shown;
        let tune = Tune { keeps_wheel: true, ..Tune::default() };
        let mut remotes = Tuned { part: &mut self.picker, tune };
        let mut remotes = OneClick(&mut remotes);
        let mut table = Thumb { bar: &mut self.bar, offset: &mut offset };
        let lines = self.form.as_mut().map_or(&mut [][..], |f| &mut f.lines[..]);
        let mut names = NameFields {
            lines,
            hits: &self.hits,
            typing: &mut self.typing,
            first: &mut self.first,
            confirming: &mut self.confirming,
        };
        let mut parts: [&mut dyn Control; 3] = [&mut remotes, &mut table, &mut names];
        let answer = f(&mut self.controls, &mut parts);
        if offset != shown {
            self.scroll_to(offset);
        }
        answer
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        // On the table's scrollbar: no row under it.
        if self.bar.contains(p) {
            return None;
        }
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The push sent (a force push only the second time): the dialog closes, the status bar
    /// tells how it went.
    fn give(&mut self) {
        let Some(form) = &self.form else { return };
        if form.problem().is_some() {
            return;
        }
        if form.force && !self.confirming {
            self.confirming = true;
            return;
        }
        let plan = form.plan();
        let Some(repo) = self.repo.clone() else { return };
        self.action = Some(ViewerAction::WithHost(WithHost::once(move |host| {
            crate::sync::start(host, repo, SyncOp::Push, move |repo| repo.push_plan(&plan));
        })));
        self.done = true;
    }

    fn open_remotes(&mut self) {
        let Some(form) = &self.form else { return };
        let items = form
            .remotes
            .iter()
            .map(|r| PickerItem { label: r.name.as_str().into(), ..PickerItem::default() })
            .collect();
        self.picker.open(items, Some(form.remote));
    }

    fn picked(&mut self, event: Option<PickerEvent>) {
        if let (Some(PickerEvent::Chosen(at)), Some(form)) = (event, self.form.as_mut()) {
            form.choose_remote(at);
            self.confirming = false;
        }
    }

    /// What a press on `hit` does to the form (a name's field is a part of its own:
    /// [`NameFields`]). The remotes' list opened has the keys.
    fn apply(&mut self, hit: Hit) {
        let Some(form) = self.form.as_mut() else { return };
        match hit {
            Hit::Remote => {
                self.open_remotes();
                self.with_controls(|c, parts| c.focus(parts, Some(REMOTES)));
            }
            Hit::Push(i) => form.lines[i].push = !form.lines[i].push,
            Hit::Track(i) => form.lines[i].track = !form.lines[i].track,
            Hit::Name(_) => {}
            Hit::All => form.toggle_all(),
            Hit::Tags => form.tags = !form.tags,
            Hit::Force => form.force = !form.force,
        }
        self.confirming = false;
    }

    fn paint_body(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        b: Rect,
        focused: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let pad = px(20.0);
        let (left, right) = (b.min_x() + pad, b.max_x() - pad);
        self.row_h = px(ROW_H);
        self.hits.clear();
        let Some(mut form) = self.form.take() else { return };
        // The remote and where it leads.
        let (font, small) = (ds.ui_font(-0.5), ds.ui_font(-1.0));
        let mut y = b.min_y() + px(10.0);
        let label = |layer: &mut Layer,
                     text: &mut TextSystem,
                     value: &str,
                     x: f32,
                     y: f32,
                     h: f32,
                     color| {
            let shaped = text.shape_line(value, &font);
            draw_text(layer, &shaped, x, ds::optical_baseline(y, h, &font), color);
        };
        label(layer, text, &t("git.push_to_repository"), left, y, px(32.0), p.text);
        let field = Rect::new(left + px(130.0), y, px(150.0), px(32.0));
        let spec = ds::DropdownSpec {
            value: form.remote_name(),
            open: self.picker.is_open(),
            hovered: self.hover == Some(Hit::Remote),
            enabled: true,
            note: None,
            placeholder: None,
        };
        ds::dropdown(layer, text, icons, ds, field, spec);
        self.remote_field = field;
        self.hits.push((field, Hit::Remote));
        let url = form.remotes.get(form.remote).map_or("", |r| r.url.as_str());
        let url_x = field.max_x() + px(12.0);
        let shown = ds::ellipsize(text, url, &small, (right - url_x).max(0.0));
        let shaped = text.shape_line(&shown, &small);
        draw_text(layer, &shaped, url_x, ds::optical_baseline(y, px(32.0), &small), p.text_muted);
        y += px(44.0);
        // The table: a tick, the local branch, its name there, a tick.
        let width = right - left;
        let (push_w, track_w) = (px(52.0), px(52.0));
        let name_w = ((width - push_w - track_w) / 2.0 - px(8.0)).floor();
        let local_x = left + push_w;
        let name_x = local_x + name_w + px(8.0);
        let track_x = right - track_w;
        let heads = [
            (t("git.push_col_push"), left),
            (t("git.push_col_local"), local_x),
            (t("git.push_col_remote"), name_x),
            (t("git.push_col_track"), track_x),
        ];
        for (head, x) in &heads {
            let shaped = text.shape_line(head, &small);
            draw_text(layer, &shaped, *x, ds::optical_baseline(y, px(24.0), &small), p.text_muted);
        }
        y += px(24.0);
        ds::divider(layer, ds, left, left + width, y);
        // The rows a little below the line (a field never touches it).
        y += px(TABLE_TOP);
        let table = Rect::new(left, y, width, self.row_h * ROWS_SHOWN as f32);
        let last = form.lines.len().saturating_sub(ROWS_SHOWN);
        self.first = self.first.min(last);
        let first = self.first;
        let count = form.lines.len();
        let style = ds::field_style(ds, ds.code_font(-1.0), false);
        let bad = ds::field_style(ds, ds.code_font(-1.0), true);
        for (i, line) in form.lines.iter_mut().enumerate().skip(first).take(ROWS_SHOWN) {
            let row_y = y + self.row_h * (i - first) as f32;
            let mid = row_y + self.row_h / 2.0;
            let state = |on: bool| if on { Check::On } else { Check::Off };
            ds::checkbox(layer, text, ds, left + px(4.0), mid, state(line.push), true);
            self.hits.push((Rect::new(left, row_y, push_w, self.row_h), Hit::Push(i)));
            let shown = ds::ellipsize(text, &line.local, &font, name_w - px(6.0));
            let shaped = text.shape_line(&shown, &font);
            draw_text(
                layer,
                &shaped,
                local_x,
                ds::optical_baseline(row_y, self.row_h, &font),
                p.text,
            );
            let field = Rect::new(name_x, row_y + px(3.0), name_w, self.row_h - px(6.0));
            let invalid = line.push
                && version_control::check_ref_name(line.remote_name.text().trim()).is_err();
            let active = focused && self.typing == Some(i);
            line.remote_name.paint(
                layer,
                text,
                field,
                active,
                "",
                if invalid { &bad } else { &style },
            );
            self.hits.push((field, Hit::Name(i)));
            ds::checkbox(layer, text, ds, track_x + px(4.0), mid, state(line.track), true);
            self.hits.push((Rect::new(track_x, row_y, track_w, self.row_h), Hit::Track(i)));
        }
        // How far it's scrolled, when they don't all fit: the app's scrollbar, in the margin
        // right of the table (painted over everything, after).
        self.bar_area = (count > ROWS_SHOWN).then(|| {
            let lane = Rect::new(left, table.min_y(), right + px(16.0) - left, table.size.height);
            (lane, count)
        });
        y = table.max_y() + px(8.0);
        // The boxes under the table.
        let force_warning = form.force;
        let mut x = left;
        let boxes = [
            (t("git.push_select_all"), form.all_state(), Hit::All),
            (t("git.push_all_tags"), if form.tags { Check::On } else { Check::Off }, Hit::Tags),
            (t("git.push_force"), if form.force { Check::On } else { Check::Off }, Hit::Force),
        ];
        for (name, state, hit) in boxes {
            let r = ds::checkbox(layer, text, ds, x, y + px(14.0), state, true);
            let shaped = text.shape_line(&name, &font);
            let end = r.max_x() + px(6.0) + shaped.width;
            draw_text(
                layer,
                &shaped,
                r.max_x() + px(6.0),
                ds::optical_baseline(y, px(28.0), &font),
                p.text,
            );
            self.hits.push((Rect::new(x, y, end - x, px(28.0)), hit));
            x = end + px(24.0);
        }
        y += px(30.0);
        // What stands in the way, or what a force push means.
        let problem = form.problem();
        let remote = form.remote_name().to_owned();
        let note = match (&problem, force_warning) {
            (Some(why), _) => Some((why.clone(), p.danger)),
            (None, true) => {
                Some((tr!("git.push_force_warning", remote = remote.as_str()), p.danger))
            }
            _ => None,
        };
        if let Some((note, color)) = note {
            let shown = ds::ellipsize(text, &note, &small, width);
            let shaped = text.shape_line(&shown, &small);
            draw_text(layer, &shaped, left, ds::optical_baseline(y, px(20.0), &small), color);
        }
        self.form = Some(form);
    }
}

impl Viewer for PushDialog {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(request) = data.downcast_ref::<PushRequest>() else { return };
        self.repo = Some(Arc::clone(&request.repo));
        let form = PushForm::new(request);
        // The branch pushed (the current one) in sight, about the middle of the rows.
        if let Some(at) = form.lines.iter().position(|l| l.push) {
            self.first = at.saturating_sub(ROWS_SHOWN / 2);
        }
        self.form = Some(form);
    }

    fn modal_height(&self) -> Option<f32> {
        Some(44.0 + 24.0 + TABLE_TOP + ROW_H * ROWS_SHOWN as f32 + 8.0 + 30.0 + 20.0 + 10.0)
    }

    /// Push (a force push: in the danger color, asked once more), Cancel — in the footer;
    /// pressed by a click only (a push is the user's to choose: no `↵`).
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        let ready = self.form.as_ref().is_some_and(|f| f.problem().is_none());
        let go = if self.confirming { t("git.push_force_confirm") } else { t("git.push_go") };
        vec![
            ModalButton::main(GO, go, ready).dangerous(self.confirming).keyless(),
            ModalButton::other(CANCEL, t("git.ref_cancel")),
        ]
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("arrow-up")
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        self.typing.is_some() || (self.picker.is_open() && self.picker.searches())
    }

    fn text_input(&mut self, typed: &str) -> bool {
        let took = self.with_controls(|c, parts| c.text(parts, typed)).is_some();
        // Typed with nowhere to take it: still a change of mind about a force push.
        if !took {
            self.confirming = false;
        }
        took
    }

    fn paste(&mut self, text: &str) -> bool {
        let line = text.lines().next().unwrap_or_default();
        let took = self.with_controls(|c, parts| c.paste(parts, line)).is_some();
        // Pasted with nowhere to take it: still a change of mind about a force push.
        if !took {
            self.confirming = false;
        }
        took
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds, scale, text, scene, palette, icons, focused, .. } = ctx;
        let ds = Ctx::new(palette, scale);
        let layer = scene.push_layer(Some(bounds));
        self.paint_body(layer, text, icons, &ds, bounds, focused);
        match self.bar_area {
            Some((lane, count)) => {
                let content = self.row_h * count as f32;
                let offset = self.row_h * self.first as f32;
                self.bar.paint(scene, palette, scale, lane, content, offset);
            }
            None => self.bar = ui_components::EdgeScrollbar::default(),
        }
        if self.picker.is_open() {
            self.picker.paint(scene, text, icons, &ds, (self.remote_field, bounds), "");
        }
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        // The remotes' list, the table's scrollbar (its thumb: a drag follows, or the rows
        // moved there), a row's name (the caret put). A press on nothing keeps the focus.
        let missed = self.hit(p).is_none();
        let pressed = self.with_controls(|c, parts| {
            if missed && !parts.iter().any(|part| part.hit(p)) {
                return None;
            }
            c.press(parts, p, clicks)
        });
        if pressed.is_some() {
            let event = self.picker.take_event();
            self.picked(event);
            return None;
        }
        if let Some(hit) = self.hit(p) {
            self.apply(hit);
        }
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn drag(&mut self, p: Point) -> bool {
        let first = self.first;
        let dragged = self.with_controls(|c, parts| c.drag(parts, p));
        dragged.is_some_and(|(_, moved)| moved || self.first != first)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = p;
        let parts = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        // A part under the mouse (the remotes' list open has it over everything): the
        // form's own hover stays as it was.
        if self.controls.hovered().is_some() {
            return parts;
        }
        let hover = self.hit(p);
        let changed = hover != self.hover || parts;
        self.hover = hover;
        changed
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.controls.cursor_or(p, || self.hit(p).map(|_| ViewerCursor::Pointer))
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        // The remotes' list open takes the wheel (it's over everything).
        let pointer = self.pointer;
        if self.with_controls(|c, parts| c.scroll(parts, pointer, dx, dy)).is_some() {
            return true;
        }
        let Some(form) = &self.form else { return false };
        self.wheel += dy;
        let rows = (self.wheel / self.row_h.max(1.0)).trunc();
        self.wheel -= rows * self.row_h;
        let last = form.lines.len().saturating_sub(ROWS_SHOWN) as f32;
        let first = (self.first as f32 + rows).clamp(0.0, last) as usize;
        let moved = first != self.first;
        self.first = first;
        moved
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // The remotes' list open has every key; a row's name its editing keys and Tab.
        if self.with_controls(|c, parts| c.key(parts, key, reach)).is_some() {
            let event = self.picker.take_event();
            self.picked(event);
            return true;
        }
        match key {
            // No key pushes: only its button, clicked.
            ViewerKey::Enter => false,
            ViewerKey::Escape => {
                self.action = Some(ViewerAction::Close);
                true
            }
            // No rows to type in.
            ViewerKey::Tab => true,
            _ => false,
        }
    }

    /// Read from `&self`: the names' part lends the row typed in (only it has one: the
    /// focus leaving it lets the row go).
    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| {
            let row = (i == NAMES).then_some(self.typing?)?;
            Some(&self.form.as_ref()?.lines.get(row)?.remote_name as &dyn Control)
        })
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            GO => {
                self.give();
                return true;
            }
            CANCEL => {
                self.action = Some(ViewerAction::Close);
                return true;
            }
            _ => {}
        }
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/push_dialog/view.rs"]
mod tests;
