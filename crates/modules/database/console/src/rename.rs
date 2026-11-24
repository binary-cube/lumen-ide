//! A console renamed or deleted from the UI ("Database Consoles", plan 39i: the consoles'
//! popover, the Services window). Renaming asks the new name in a small modal of the
//! module's own (`view-console-rename`: closed only by ×, Esc or its buttons; the name
//! checked as it's typed — empty, a character a file can't have, a name its source's other
//! consoles have); the file is renamed and the rename announced on the project's bus, so it
//! goes the way a rename from the tab or the tree does ([`crate::Consoles::rename`]): the
//! same console — its id, settings, runner — under the new name, its tabs following.
//! Deleting asks first (`host.confirm`), then removes the file and says so on the bus (its
//! saved tab closes, a transaction left open is rolled back and said).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use database::console::ConsoleId;
use i18n::t;
use kernel::{Point, ProjectId, Rect};
use ui_components::control::{Control, Controls};
use ui_components::ds::{self, Ctx};
use ui_components::{
    KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::{TextInput, TextInputStyle, draw_icon, draw_text};
use workbench_model::{Notification, Question, WorkbenchHost};

use crate::field::KeyField;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-console-rename";

/// What the modal shows: the console, its source, the names its source's others have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameRequest {
    /// Set by [`ask_name`]: a name is only taken for the question it was given to.
    pub id: u64,
    pub project: ProjectId,
    pub console: ConsoleId,
    pub name: String,
    pub source_name: String,
    pub taken: Vec<String>,
}

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Console `id`'s new name asked in the modal. `false`: no such console (said), or no modals.
pub fn ask_name(host: &mut dyn WorkbenchHost, id: ConsoleId) -> bool {
    let Some(project) = host.project_id() else { return false };
    let Some(consoles) = crate::consoles::consoles(project) else { return false };
    let Some(spec) = consoles.spec(id) else {
        host.notify(Notification::warning(t("console.gone")));
        return false;
    };
    let source_name =
        consoles.access().and_then(|a| a.source(spec.source)).map(|b| b.name).unwrap_or_default();
    // The names taken as on disk, looked at in the background; the modal opens with them.
    host.spawn(Box::new(move || {
        let taken: Vec<String> = consoles
            .consoles_of(spec.source)
            .into_iter()
            .filter(|s| s.id != id)
            .map(|s| s.name)
            .collect();
        Box::new(move |host: &mut dyn WorkbenchHost| {
            let request = RenameRequest {
                id: NEXT.fetch_add(1, Ordering::Relaxed),
                project,
                console: id,
                name: spec.name,
                source_name,
                taken,
            };
            host.open_modal(&t("console.rename_title"), EXTENSION, &request);
        }) as workbench_model::Completion
    }));
    true
}

/// Console `id` renamed `name` (its file, then everything following it). `true` if it was.
pub fn apply(host: &mut dyn WorkbenchHost, id: ConsoleId, name: &str) -> bool {
    let Some(consoles) = host.project_id().and_then(crate::consoles::consoles) else {
        return false;
    };
    match consoles.rename(id, name) {
        Ok(_) => true,
        Err(e) => {
            host.notify(Notification::warning(e.text()));
            false
        }
    }
}

/// Console `id` deleted once the user said so: its file removed (a transaction left open
/// rolled back, said).
pub fn confirm_delete(host: &mut dyn WorkbenchHost, id: ConsoleId) -> bool {
    let Some(consoles) = host.project_id().and_then(crate::consoles::consoles) else {
        return false;
    };
    let Some(spec) = consoles.spec(id) else {
        host.notify(Notification::warning(t("console.gone")));
        return false;
    };
    let open = consoles.runner_of(id).is_some_and(|r| r.in_transaction());
    let mut message = i18n::tr!("console.delete_message", name = spec.name.as_str());
    if open {
        message.push(' ');
        message.push_str(&t("console.delete_tx"));
    }
    let question = Question {
        title: i18n::tr!("console.delete_title", name = spec.name.as_str()),
        message,
        icon: "trash",
        yes: t("console.delete"),
        danger: true,
        list: Vec::new(),
    };
    host.confirm(
        question,
        Box::new(move |host| {
            let Some(consoles) = host.project_id().and_then(crate::consoles::consoles) else {
                return;
            };
            if let Err(e) = consoles.delete(id) {
                host.notify(Notification::warning(e.text()));
            }
        }),
    )
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct RenameProvider;

impl ViewerProvider for RenameProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(RenameView::default()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Go,
    Cancel,
}

/// The modal's view.
#[derive(Debug, Default)]
pub struct RenameView {
    request: Option<RenameRequest>,
    input: TextInput,
    /// Where the name's field was drawn last.
    field: Rect,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    done: bool,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
}

/// The name's field's place among the modal's parts.
const FIELD: usize = 0;

/// The modal's content (logical pixels).
const HEIGHT: f32 = 150.0;

impl RenameView {
    /// Why the name typed can't be given (`None`: it can, or it's the same).
    pub fn problem(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        database::console::check_name(self.input.text(), r.taken.iter().map(String::as_str))
            .err()
            .map(|e| crate::consoles::RenameError::Name(e).text())
    }

    pub fn set_name(&mut self, name: &str) {
        self.input.set_text(name);
    }

    /// The name given (when it can be): the window closes, the console is renamed.
    pub fn give(&mut self) -> bool {
        if self.problem().is_some() {
            return false;
        }
        let Some(r) = &self.request else { return false };
        let (console, name) = (r.console, self.input.text().trim().to_owned());
        self.done = true;
        self.action = Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            apply(host, console, &name);
        })));
        true
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// Its parts (the name's field, which has the keys the whole time it's open), with their
    /// state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut edited = false;
        let mut name = KeyField {
            input: &mut self.input,
            area: self.field,
            selects: true,
            edited: &mut edited,
        };
        let mut parts: [&mut dyn Control; 1] = [&mut name];
        self.controls.focus(&mut parts, Some(FIELD));
        f(&mut self.controls, &mut parts)
    }
}

impl Viewer for RenameView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<RenameRequest>() {
            self.input = TextInput::new(&request.name);
            self.input.select_all();
            self.request = Some(request.clone());
            // The name's field has the keys from now (its selection the copy's).
            self.controls = Controls::sticky();
            self.with_controls(|_, _| ());
        }
    }

    fn subtitle(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        Some(i18n::tr!(
            "console.confirm_subtitle",
            console = r.name.as_str(),
            source = r.source_name.as_str()
        ))
    }

    fn modal_height(&self) -> Option<f32> {
        Some(HEIGHT)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("file-pen")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], t("console.rename"))]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed));
        true
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let pad = px(20.0);
        let layer = scene.push_layer(Some(b));
        let field =
            Rect::new(b.min_x() + pad, b.min_y() + px(10.0), b.size.width - 2.0 * pad, px(36.0));
        let problem = self.problem();
        let style = TextInputStyle {
            font: ds.code_font(-0.5),
            background: p.input_background,
            border: if problem.is_some() { p.danger } else { p.input_border },
            border_focused: if problem.is_some() { p.danger } else { p.accent },
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: px(ds::radius::MD),
            border_width: ds.line(),
            padding: px(10.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
            trail: 0.0,
        };
        self.input.paint(layer, text, field, true, &t("console.rename_placeholder"), &style);
        // `.sql` after the name, as the file will be.
        let suffix = text.shape_line(".sql", &style.font);
        draw_text(
            layer,
            &suffix,
            (field.max_x() - px(10.0) - suffix.width).round(),
            ds::optical_baseline(field.min_y(), field.size.height, &style.font),
            p.text_muted,
        );
        self.field = field;
        let small = ds.ui_font(-1.0);
        let note_y = field.max_y() + px(8.0);
        let (note, color) = match &problem {
            Some(why) => (why.clone(), p.danger),
            None => (t("console.rename_note"), p.text_muted),
        };
        let mut x = b.min_x() + pad;
        if problem.is_some()
            && let Some(icon) = icons.id("circle-alert")
        {
            let s = px(13.0);
            draw_icon(
                layer,
                icons,
                icon,
                Rect::new(x.round(), (note_y + px(10.0) - s / 2.0).round(), s, s),
                p.danger,
            );
            x += px(20.0);
        }
        let shown = ds::ellipsize(text, &note, &small, b.max_x() - pad - x);
        let shaped = text.shape_line(&shown, &small);
        draw_text(layer, &shaped, x.round(), ds::optical_baseline(note_y, px(20.0), &small), color);
        // The buttons.
        let bh = px(ds::BUTTON_H);
        let by = b.max_y() - px(56.0) + (px(56.0) - bh) / 2.0;
        let go = t("console.rename");
        let w = ds::primary_width(text, &ds, &[&go]);
        let g = Rect::new(b.max_x() - pad - w, by, w, bh);
        ds::primary_button(
            layer,
            text,
            &ds,
            g,
            &go,
            problem.is_none(),
            self.hover == Some(Hit::Go),
        );
        self.hits.push((g, Hit::Go));
        let cancel = t("console.confirm_cancel");
        let cw = ds::primary_width(text, &ds, &[&cancel]);
        let c = Rect::new(g.min_x() - px(10.0) - cw, by, cw, bh);
        ds::secondary_button(layer, text, &ds, c, &cancel, true, self.hover == Some(Hit::Cancel));
        self.hits.push((c, Hit::Cancel));
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // The field: its caret put, a drag selecting.
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        match self.hit(p) {
            Some(Hit::Go) => {
                self.give();
            }
            Some(Hit::Cancel) => self.action = Some(ViewerAction::Close),
            None => {}
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        self.hit(p).map(|_| ViewerCursor::Pointer)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter => {
                self.give();
            }
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Tab => return false,
            // The field's editing keys (⌦ at its end too: nothing to delete).
            ViewerKey::Delete => {
                self.with_controls(|c, parts| c.key(parts, key, reach));
            }
            key => return self.with_controls(|c, parts| c.key(parts, key, reach)).is_some(),
        }
        true
    }

    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| (i == FIELD).then_some(&self.input as &dyn Control))
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "go" => self.give(),
            id if id.starts_with("name:") => {
                self.set_name(&id["name:".len()..]);
                true
            }
            // The field's edit commands (⌘A, ⌘Z, ⇧⌘Z, ⌘X).
            id => self.with_controls(|c, parts| c.command(parts, id)).is_some(),
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/rename.rs"]
mod tests;
