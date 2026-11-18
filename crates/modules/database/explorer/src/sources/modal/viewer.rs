//! The sources' window as a viewer: its modal frame, its keys, the mouse.

use super::*;

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
            key => {
                let Some(field) = self.focus.clone() else { return false };
                let Some(input) = self.focused_input() else { return false };
                let edits = matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
                match key {
                    ViewerKey::Left => input.move_left(reach.word, reach.extend),
                    ViewerKey::Right => input.move_right(reach.word, reach.extend),
                    ViewerKey::Home => input.home(reach.extend),
                    ViewerKey::End => input.end(reach.extend),
                    ViewerKey::Backspace => input.backspace(reach.word),
                    ViewerKey::Delete => input.delete(),
                    _ => return false,
                }
                if edits {
                    self.edited(&field);
                }
            }
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
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
        let field = self.focus.clone();
        let used = match id {
            "select_all" => self.focused_input().is_some_and(|i| {
                i.select_all();
                true
            }),
            "undo" | "redo" => {
                self.focused_input().is_some_and(|i| if id == "redo" { i.redo() } else { i.undo() })
            }
            "cut" => {
                // A password is never cut out (nor copied).
                if field == Some(Field::Password) {
                    return true;
                }
                self.focused_input().is_some_and(|i| {
                    let had = !i.selected_text().is_empty();
                    if had {
                        i.insert("");
                    }
                    had
                })
            }
            #[cfg(feature = "snapshot")]
            id if id.starts_with("snapshot:") => self.snapshot(&id["snapshot:".len()..]),
            _ => return false,
        };
        if used
            && matches!(id, "undo" | "redo" | "cut")
            && let Some(field) = field
        {
            self.edited(&field);
        }
        used
    }

    /// Never the password's text.
    fn selected_text(&self) -> Option<String> {
        let field = self.focus.as_ref()?;
        if *field == Field::Password {
            return None;
        }
        let input = self.editor.as_ref()?.input(field)?;
        let selected = input.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if self.picker.is_open() {
            return self.picker.scroll(dy);
        }
        let at = self.hover_point();
        if self.list_area.contains(at) {
            self.list_scroll = (self.list_scroll + dy).clamp(0.0, self.list_max);
        } else {
            self.form_scroll = (self.form_scroll + dy).clamp(0.0, self.form_max);
        }
        true
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if let Some(offset) = self.form_bar.press(p) {
            self.form_scroll = offset;
            return None;
        }
        // An open list takes the click: a row chosen; outside it, it closes — and the click
        // goes on, but on the field (or `+`) that opened it (that only closes it).
        if self.picker.is_open() {
            let inside = self.picker.contains(p);
            let event = self.picker.press(p);
            let opener = self.popup.clone();
            self.picked(event);
            let hit = self.hit(p);
            let same = matches!((&hit, &opener), (Some(Hit::Choice(c)), Some(o)) if c == o)
                || (matches!(hit, Some(Hit::Tool(Tool::Add))) && opener == Some(Choice::AddDriver));
            if inside || same {
                return self.action.take();
            }
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
            Hit::Field(field) => {
                if let Some(editor) = &mut self.editor
                    && let Some(input) = editor.input_mut(&field)
                {
                    input.click(p.x);
                }
                self.focus = Some(field);
            }
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
        if let Some(offset) = self.form_bar.drag(p) {
            self.form_scroll = offset.clamp(0.0, self.form_max);
            return true;
        }
        match self.focused_input() {
            Some(input) if input.contains(p) => {
                input.extend_to(p.x);
                true
            }
            _ => false,
        }
    }

    fn release(&mut self) -> bool {
        self.pressed = None;
        let (dragged, drop) = (self.dragging.take(), self.drop.take());
        if let (Some(item), Some(place)) = (dragged, drop) {
            self.move_to(item, place);
            return true;
        }
        dragged.is_some() | self.form_bar.release()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.close_popup();
        self.list_menu(p)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_hover = Some(p);
        if self.picker.is_open() {
            return self.picker.hover(p);
        }
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | self.form_bar.hover(p)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // An open list: its own cursor, never what's under it.
        if self.picker.is_open() {
            return Some(self.picker.cursor_at(p).unwrap_or(ViewerCursor::Arrow));
        }
        match self.hit(p)? {
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
