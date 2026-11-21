//! The sources' window as a viewer: its modal frame, its keys, the mouse.

use super::*;
use fields::{FieldPart, FilterPart};
use ui_components::control::Shown;

impl Viewer for SourcesView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<SourcesRequest>() {
            self.take(request.clone());
        }
    }

    fn subtitle(&self) -> Option<String> {
        Some(t("datasources.modal_subtitle"))
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("database")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["tab"], t("datasources.hint_next")), (vec!["↵"], t("datasources.ok"))]
    }

    /// Right to left: OK, Apply, Cancel, then Test Connection apart; asking before changes
    /// are let go: Discard, Keep Editing.
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        let button = |b: Footer, key: &str| ModalButton::other(footer_id(b), t(key));
        if self.confirm_close {
            return vec![
                ModalButton::danger(footer_id(Footer::Discard), t("datasources.discard")),
                button(Footer::KeepEditing, "datasources.keep_editing"),
            ];
        }
        let dirty = self.editor.as_ref().is_some_and(Editor::dirty);
        let has = self.editor.as_ref().is_some_and(|e| e.current().is_some());
        let running = self.probe_shown() == Probe::Running;
        let test = if running { "datasources.test_cancel" } else { "datasources.test" };
        vec![
            ModalButton::main(footer_id(Footer::Ok), t("datasources.ok"), true),
            button(Footer::Apply, "datasources.apply").enabled(dirty),
            button(Footer::Cancel, "datasources.cancel"),
            button(Footer::Test, test).enabled(has).separated(),
        ]
    }

    /// The question before changes are let go; else why Apply didn't go through, or how
    /// the test went.
    fn modal_status(&self) -> Option<ui_components::ModalStatus> {
        use ui_components::ModalStatus;
        if self.confirm_close {
            return Some(ModalStatus::Warning(t("datasources.discard_question")));
        }
        if let Some(error) = &self.error {
            return Some(ModalStatus::Failed(error.clone()));
        }
        match self.probe_shown() {
            Probe::Idle => None,
            Probe::Running => Some(ModalStatus::Busy(t("datasources.testing"))),
            Probe::Connected { server, millis, .. } => Some(ModalStatus::Done(i18n::tr!(
                "datasources.test_ok",
                server = server.as_str(),
                millis = millis
            ))),
            Probe::Failed(why) => Some(ModalStatus::Failed(why)),
            Probe::Cancelled => Some(ModalStatus::Note(t("datasources.test_cancelled"))),
        }
    }

    fn may_close(&mut self) -> bool {
        self.close();
        matches!(self.action, Some(ViewerAction::Close))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let Some(waker) = &ctx.waker {
            self.waker = Some(Arc::clone(waker));
        }
        self.hits.clear();
        self.tips.clear();
        self.paint_modal(ctx);
    }

    fn poll(&mut self) -> bool {
        let made = self.make_editor();
        self.take_probe();
        let saved = self.saved.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(Err(why)) = &saved {
            let said = i18n::tr!("datasources.secret_failed", why = why.as_str());
            // Told once (it stays until closed), and in the footer while the modal is open.
            if !std::mem::replace(&mut self.failure_told, true)
                && let Some(request) = &self.request
            {
                request.sessions.notify(Notification::error(said.clone()));
            }
            self.error = Some(said);
            self.close_when_saved = false;
        }
        if self.close_when_saved && matches!(saved, Some(Ok(()))) {
            self.close_now();
        }
        made || true
    }

    fn is_loading(&self) -> bool {
        self.editor.is_none() || self.tester.probe() == Probe::Running
    }

    fn wants_text(&self) -> bool {
        self.focus.is_some() || (self.picker.is_open() && self.picker.searches())
    }

    fn text_input(&mut self, text: &str) -> bool {
        if self.picker.is_open() {
            return self.picker.text_input(text);
        }
        if self.focus.is_none() {
            return false;
        }
        self.type_text(text);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let used = self.key_pressed(key, reach);
        self.sync_focus();
        used
    }

    fn command(&mut self, id: &str) -> bool {
        let used = self.run_command(id);
        self.sync_focus();
        used
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        let action = self.pressed_at(p);
        self.sync_focus();
        action
    }

    /// The focused field's (never the password's), its part lent to be read.
    fn selected_text(&self) -> Option<String> {
        let editor = self.editor.as_ref()?;
        let fields: Vec<FieldPart<'_, &ui_kit::TextInput>> = editor
            .inputs()
            .into_iter()
            .map(|(field, input)| FieldPart { field, input, hits: &self.hits })
            .collect();
        self.controls.0.selected_text_with(|i| {
            let part = fields.get(i.checked_sub(FIELDS)?)?;
            // A place out of step with the field having the keys lends nothing.
            (self.focus.as_ref() == Some(&part.field)).then_some(part as &dyn Control)
        })
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    /// The part under the mouse (an open list over everything, the sources' list, the
    /// form), as it is now (a list may have opened under it since it last moved).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let at = self.last_hover;
        self.with_controls(|c, parts| {
            c.hover(parts, at);
            c.scroll_hovered(parts, dx, dy)
        })
        .is_some()
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some((item, from)) = self.pressed {
            let far = (p.x - from.x).abs().max((p.y - from.y).abs()) > 4.0;
            if self.dragging.is_some() || far {
                self.dragging = Some(item);
                self.drag_at = p;
                self.drop = self.place_at(p);
                return true;
            }
        }
        // A field holding the pointer is only among all the parts (made for it alone).
        let dragged = self.with_controls(|c, parts| c.drag(parts, p)).or_else(|| {
            if self.controls.0.held() {
                self.with_fields(|c, parts| c.drag(parts, p))
            } else {
                None
            }
        });
        match dragged {
            Some((FORM, _)) => {
                self.form_scroll = self.form_scroll.clamp(0.0, self.form_max);
                true
            }
            // A field's selection, when the pointer is over it.
            Some((part, changed)) => part < FIELDS || changed,
            None => false,
        }
    }

    fn release(&mut self) -> bool {
        self.pressed = None;
        let (dragged, drop) = (self.dragging.take(), self.drop.take());
        if let (Some(item), Some(place)) = (dragged, drop) {
            self.move_to(item, place);
            return true;
        }
        let parts = self.with_fields(|c, parts| c.release(parts));
        dragged.is_some() | parts.is_some_and(|(_, changed)| changed)
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.close_popup();
        self.list_menu(p)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_hover = Some(p);
        // An open list takes the mouse over everything.
        let parts = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        if self.picker.is_open() {
            return parts;
        }
        let hover = self.hit(p);
        let changed = hover != self.hover;
        // The filter's × lit under the mouse.
        let on_filter = matches!(hover, Some(Hit::Field(Field::Filter)));
        let clear = self
            .editor
            .as_mut()
            .is_some_and(|e| Control::hover(&mut e.filter, on_filter.then_some(p)));
        self.hover = hover;
        changed | parts | clear
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // A part's (an open list's anywhere: never what's under it).
        if let Some(cursor) = self.controls.0.cursor_at(p) {
            return Some(cursor);
        }
        match self.hit(p)? {
            Hit::Field(Field::Filter) => {
                self.editor.as_ref().and_then(|e| Control::cursor(&e.filter, p))
            }
            Hit::Field(_) => Some(ViewerCursor::Text),
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        // Not through an open list over them.
        if self.popup.is_some() {
            return None;
        }
        let (r, label) = self.tips.iter().find(|(r, _)| r.contains(p))?;
        Some(ui_components::tooltip::Tip::new(*r, label.clone()))
    }
}

/// The open list's and the form's places among the window's parts; the fields come after
/// them, from `FIELDS` on, in the editor's order ([`Editor::inputs`]).
const PICKER: usize = 0;
const FORM: usize = 2;
const FIELDS: usize = 3;

impl SourcesView {
    /// A key: an open list's; Tab, ↑ ↓ the view's own order; ↵ OK; esc; the field's.
    fn key_pressed(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // An open list has the keys.
        if self.picker.is_open() {
            let event = self.picker.key(key, reach);
            self.picked(event);
            return true;
        }
        match key {
            ViewerKey::Tab => self.focus_step(!reach.extend),
            ViewerKey::Down if self.focus.is_none() || self.focus == Some(Field::Filter) => {
                self.step_row(true)
            }
            ViewerKey::Up if self.focus.is_none() || self.focus == Some(Field::Filter) => {
                self.step_row(false)
            }
            ViewerKey::Down => self.focus_step(true),
            ViewerKey::Up => self.focus_step(false),
            ViewerKey::Enter => self.ok(),
            ViewerKey::Escape => {
                if self.focus == Some(Field::Filter)
                    && self.editor.as_ref().is_some_and(|e| !e.filter.text().is_empty())
                {
                    if let Some(editor) = &mut self.editor {
                        editor.filter.set_text("");
                    }
                    self.edited(&Field::Filter);
                } else {
                    self.close();
                }
            }
            // The editing keys: the focused field's.
            key => {
                let Some(field) = self.focus.clone() else { return false };
                if self.with_fields(|c, parts| c.key(parts, key, reach)).is_none() {
                    return false;
                }
                if matches!(key, ViewerKey::Backspace | ViewerKey::Delete) {
                    self.edited(&field);
                }
            }
        }
        true
    }

    /// An edit command, a footer's button, a list's row, a file picked.
    fn run_command(&mut self, id: &str) -> bool {
        // The footer's buttons (the window draws them).
        if let Some(&(_, button)) = FOOTER.iter().find(|(i, _)| *i == id) {
            self.footer(button);
            return true;
        }
        // An open list's search gets the edit commands.
        if self.picker.is_open() {
            return self.picker.command(id);
        }
        if id.starts_with("ds:") {
            return self.list_command(id);
        }
        if let Some(path) = id.strip_prefix("picked-file:") {
            let Some(field) = self.browsing.take() else { return false };
            if let Some(editor) = &mut self.editor {
                editor.picked(&field, path);
            }
            return true;
        }
        #[cfg(feature = "snapshot")]
        if let Some(sample) = id.strip_prefix("snapshot:") {
            return self.snapshot(sample);
        }
        let field = self.focus.clone();
        // The password's cut is the modal's all the same: its field takes none, and it never
        // reaches what's behind the window.
        if id == "cut" && field == Some(Field::Password) {
            return true;
        }
        let used = self.with_fields(|c, parts| c.command(parts, id)).is_some();
        if used
            && matches!(id, "undo" | "redo" | "cut")
            && let Some(field) = field
        {
            self.edited(&field);
        }
        used
    }

    /// A press at `p`: on an open list, a field, or what's drawn there.
    fn pressed_at(&mut self, p: Point) -> Option<ViewerAction> {
        let inside = self.picker.contains(p);
        let opener = self.popup.clone();
        let mut pressed = self.with_fields(|c, parts| c.press(parts, p, 1));
        if pressed.is_some_and(|(part, _)| part == FORM) {
            return None;
        }
        // An open list takes the click: a row chosen; outside it, it closes — and the click
        // goes on, but on the field (or `+`) that opened it (that only closes it).
        if pressed.is_some_and(|(part, _)| part == PICKER) {
            let event = self.picker.take_event();
            self.picked(event);
            let hit = self.hit(p);
            let same = matches!((&hit, &opener), (Some(Hit::Choice(c)), Some(o)) if c == o)
                || (matches!(hit, Some(Hit::Tool(Tool::Add))) && opener == Some(Choice::AddDriver));
            if inside || same {
                return self.action.take();
            }
            // Closed, it lets the click on to a field under it (never to a bar).
            pressed = self.with_fields(|c, parts| {
                let mut shown: Vec<Shown<'_>> = parts
                    .iter_mut()
                    .enumerate()
                    .map(|(i, part)| Shown { part: &mut **part, shown: i >= FIELDS })
                    .collect();
                let mut fields: Vec<&mut dyn Control> =
                    shown.iter_mut().map(|s| s as &mut dyn Control).collect();
                c.press(&mut fields, p, 1)
            });
        }
        // A field took it (its caret placed, the focus on it); the filter's × emptied it.
        if pressed.is_some_and(|(part, _)| part >= FIELDS) {
            if self.editor.as_mut().is_some_and(|e| e.filter.take_cleared()) {
                self.edited(&Field::Filter);
            }
            return self.action.take();
        }
        let hit = self.hit(p);
        let Some(hit) = hit else {
            self.focus = None;
            return None;
        };
        match hit {
            Hit::Row(i) => {
                self.select(i);
                let id = self.editor.as_ref().and_then(|e| e.entries.get(i)).map(|e| e.source.id);
                self.pressed = id.map(|id| (Item::Source(id), p));
            }
            Hit::Folder(id, chevron) => {
                if chevron {
                    if let Some(editor) = &mut self.editor {
                        editor.toggle_folder(id);
                    }
                } else {
                    self.select_folder(id);
                    self.pressed = Some((Item::Folder(id), p));
                }
            }
            Hit::Section(_) => {}
            Hit::SourceColor(color) => {
                if let Some(editor) = &mut self.editor {
                    editor.set_color(color);
                }
                self.confirm_close = false;
            }
            Hit::FolderColor(color) => {
                if let Some(editor) = &mut self.editor
                    && let Some(id) = editor.selected_folder
                {
                    editor.set_folder_color(id, color);
                }
                self.confirm_close = false;
            }
            // Its part took the press (a field not in the editor takes none).
            Hit::Field(_) => {}
            Hit::Tab(tab) => self.set_tab(tab),
            Hit::Tool(tool) => self.tool(tool),
            Hit::Choice(choice) => self.open_popup(choice),
            Hit::Flag(id) => {
                if let Some(editor) = &mut self.editor {
                    editor.flip(&id);
                }
            }
            Hit::ReadOnly => {
                if let Some(editor) = &mut self.editor {
                    let now = editor.current().is_some_and(|e| e.source.read_only);
                    editor.set_read_only(!now);
                }
            }
            Hit::Production => {
                if let Some(editor) = &mut self.editor {
                    // A folder's mark isn't taken away from here: the box is off then.
                    let inherited = editor.selected.and_then(|i| editor.inherited_production(i));
                    if inherited.is_none() {
                        let now = editor.current().is_some_and(|e| e.source.production);
                        editor.set_production(!now);
                    }
                }
            }
            Hit::FolderProduction => {
                if let Some(editor) = &mut self.editor
                    && let Some(folder) = editor.current_folder().cloned()
                {
                    editor.set_folder_production(folder.id, !folder.production);
                }
            }
            Hit::Schema(name) => {
                if let Some(editor) = &mut self.editor {
                    let all = editor.schemas();
                    editor.toggle_schema(&name, &all);
                }
            }
            Hit::AllSchemas => {
                if let Some(editor) = &mut self.editor {
                    editor.show_all_schemas();
                }
            }
            Hit::Database(name) => {
                if let Some(editor) = &mut self.editor {
                    let all = editor.databases();
                    editor.toggle_database(&name, &all);
                }
            }
            Hit::AllDatabases => {
                if let Some(editor) = &mut self.editor {
                    editor.show_all_databases();
                }
            }
            Hit::Browse(id) => {
                self.browsing = Some(id);
                let start = self.request.as_ref().map(|r| r.sessions.root().to_path_buf());
                return Some(ViewerAction::Request(AppRequest::PickFile(start)));
            }
            Hit::Reveal => {
                if let Some(editor) = &mut self.editor {
                    editor.revealed = !editor.revealed;
                }
            }
            Hit::AddOption => {
                if let Some(editor) = &mut self.editor {
                    self.focus = Some(editor.add_option());
                }
            }
            Hit::RemoveOption(i) => {
                if let Some(editor) = &mut self.editor {
                    editor.remove_option(i);
                }
                self.focus = None;
            }
        }
        self.action.take()
    }

    /// Its parts (an open list, over everything; the sources' list, the form and its
    /// scrollbar: the rest of the window), the topmost first, with their state of use: what
    /// the mouse moving and the wheel need, without the fields.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        self.with_parts(false, f)
    }

    /// [`Self::with_controls`] and the fields after them: a press, a key, the text, an edit
    /// command. The focus given to the parts is the field having the keys (`focus`, as the
    /// view moves it: Tab, ↑ ↓, a list's row); one a part took is the view's after.
    pub(super) fn with_fields<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        self.with_parts(true, f)
    }

    /// The parts' focus put back on the field having the keys (the editor's fields may have
    /// moved in its order since: another source selected).
    pub(super) fn sync_focus(&mut self) {
        self.with_fields(|_, _| ());
    }

    fn with_parts<R>(
        &mut self,
        fields: bool,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        // The sources' list has no scrollbar: one never drawn is never hit.
        let mut no_bar = ui_components::EdgeScrollbar::default();
        let mut list = Scrolled {
            bar: &mut no_bar,
            offset: &mut self.list_scroll,
            area: self.list_area,
            max: self.list_max,
        };
        let mut form = Scrolled {
            bar: &mut self.form_bar,
            offset: &mut self.form_scroll,
            area: self.bounds,
            max: self.form_max,
        };
        let controls = &mut self.controls.0;
        if !fields {
            let mut parts: [&mut dyn Control; FIELDS] = [&mut self.picker, &mut list, &mut form];
            return f(controls, &mut parts);
        }
        let hits = &self.hits;
        let (mut filter, plain) = match self.editor.as_mut() {
            Some(editor) => {
                let (search, plain) = editor.inputs_mut();
                (Some(FilterPart { search, hits }), plain)
            }
            None => (None, Vec::new()),
        };
        let mut inputs: Vec<FieldPart<'_, &mut ui_kit::TextInput>> =
            plain.into_iter().map(|(field, input)| FieldPart { field, input, hits }).collect();
        // The fields' order (the filter first, while there's an editor): their places from
        // `FIELDS` on.
        let lead = usize::from(filter.is_some());
        let field_at = |i: usize, inputs: &[FieldPart<'_, &mut ui_kit::TextInput>]| match i {
            0 if lead == 1 => Some(Field::Filter),
            i => inputs.get(i.checked_sub(lead)?).map(|part| part.field.clone()),
        };
        let at = self
            .focus
            .as_ref()
            .and_then(|f| match f {
                Field::Filter => filter.as_ref().map(|_| 0),
                f => inputs.iter().position(|part| part.field == *f).map(|i| i + lead),
            })
            .map(|i| FIELDS + i);
        let (out, focused) = {
            let mut parts: Vec<&mut dyn Control> = Vec::with_capacity(FIELDS + lead + inputs.len());
            parts.extend([&mut self.picker as &mut dyn Control, &mut list, &mut form]);
            if let Some(filter) = &mut filter {
                parts.push(filter);
            }
            parts.extend(inputs.iter_mut().map(|part| part as &mut dyn Control));
            controls.focus(&mut parts, at);
            let out = f(controls, &mut parts);
            (out, controls.focused())
        };
        // A field the parts' focus went to has the keys; none: none has them. The open list
        // focused leaves the field's (the keys go back to it once the list closes).
        if focused != at {
            match focused.map(|i| i.checked_sub(FIELDS).and_then(|i| field_at(i, &inputs))) {
                None => self.focus = None,
                Some(Some(field)) => self.focus = Some(field),
                Some(None) => {}
            }
        }
        out
    }
}
