//! The modal as a view (`Viewer`): its frame's title, buttons and status, the keys, the
//! text, the commands, the mouse.

use super::controls::LIST;
use super::*;

impl Viewer for SchemaView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<SchemaRequest>() {
            self.take(request.clone());
        }
    }

    fn subtitle(&self) -> Option<String> {
        let form = self.form.as_ref()?;
        Some(match form.table() {
            Some(table) => match &form.kind {
                Kind::AlterColumn(c) | Kind::DropColumn(c) | Kind::MoveColumn(c) => {
                    format!("{table}.{c}")
                }
                Kind::DropIndex(name) => format!("{table} · {name}"),
                _ => table.to_string(),
            },
            None if form.kind.is_catalog() => String::new(),
            None => t("schema.subtitle_new_table"),
        })
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some(if self.is_catalog() { "database" } else { "table" })
    }

    /// The submit (in the danger color when it only takes away), Cancel.
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        let drops = self.form.as_ref().is_some_and(|f| f.kind.drops());
        let submit = ModalButton::main(SUBMIT, self.submit_label(), self.can_submit());
        vec![submit.dangerous(drops), ModalButton::other(CANCEL, t("database.preview_cancel"))]
    }

    /// Running; else what runs is: SQL on the server, or a file written.
    fn modal_status(&self) -> Option<ui_components::ModalStatus> {
        use ui_components::ModalStatus;
        if self.running.is_some() {
            return Some(ModalStatus::Busy(t("schema.running")));
        }
        let sql = self.form.as_ref().is_some_and(|f| f.ctx.sql());
        let note = if sql { t("database.preview_sql_note") } else { super::super::file_note() };
        Some(ModalStatus::Note(note))
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        let fields = self.form.as_ref().is_some_and(|f| !f.texts().is_empty());
        let mut hints = Vec::new();
        if fields {
            hints.push((vec!["tab"], t("schema.hint_next")));
        }
        hints.push((vec!["↵"], self.submit_label()));
        hints
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        // Woken by the app's waker (none in a snapshot: the replies are polled anyway).
        let wake = ctx.waker.clone().unwrap_or_else(|| Arc::new(|| {}));
        self.connect(wake);
        self.hits.clear();
        self.tips.clear();
        self.paint_modal(ctx);
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        // Not through an open popup over them.
        if self.popup.is_some() {
            return None;
        }
        let (r, label) = self.tips.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(ui_components::tooltip::Tip::new(*r, label.clone()))
    }

    fn poll(&mut self) -> bool {
        self.take_replies()
    }

    fn is_loading(&self) -> bool {
        self.running.is_some() || self.asked.is_some()
    }

    fn wants_text(&self) -> bool {
        self.focus.is_some() || (self.choosing() && self.picker.searches())
    }

    fn text_input(&mut self, text: &str) -> bool {
        // A choice's list's search, else the focused field (under its suggestions too).
        let used = self.with_controls(|c, parts| c.text(parts, text)).is_some();
        self.heard_keys();
        used
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // An open list over everything first (a choice's has every key; a field's
        // suggestions are lit by the arrows, taken by ↵ or ⇥), then the focused field.
        let choosing = self.choosing();
        let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
        if choosing {
            let event = self.picker.take_event();
            self.picked(event);
            return true;
        }
        if self.heard_keys() || used {
            return true;
        }
        match key {
            ViewerKey::Tab | ViewerKey::Down => self.focus_step(true),
            ViewerKey::Up => self.focus_step(false),
            ViewerKey::Enter => {
                self.popup = None;
                self.submit();
            }
            ViewerKey::Escape => self.cancel(),
            _ => return false,
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
        // The footer's buttons (the window draws them).
        match id {
            SUBMIT => return self.submit(),
            CANCEL => {
                self.cancel();
                return true;
            }
            _ => {}
        }
        // A choice's list's search gets the edit commands.
        if self.choosing() {
            return self.picker.command(id);
        }
        let field = self.focus;
        let used = match id {
            "submit" => self.submit(),
            "select_all" => self.focused_input().is_some_and(|i| {
                i.select_all();
                true
            }),
            "undo" | "redo" => {
                self.focused_input().is_some_and(|i| if id == "redo" { i.redo() } else { i.undo() })
            }
            "cut" => self.focused_input().is_some_and(|i| {
                let had = !i.selected_text().is_empty();
                if had {
                    i.insert("");
                }
                had
            }),
            #[cfg(feature = "snapshot")]
            id if id.starts_with("snapshot:") => self.snapshot(&id["snapshot:".len()..]),
            _ => return false,
        };
        if used
            && matches!(id, "undo" | "redo" | "cut")
            && let Some(field) = field
        {
            self.edited(field);
        }
        used
    }

    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| self.part(i))
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    /// The part under the mouse (an open list over everything, the preview, the form), as
    /// it is now (a list may have opened under it since it last moved).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let at = self.last_hover;
        self.with_controls(|c, parts| {
            c.hover(parts, at);
            c.scroll_hovered(parts, dx, dy)
        })
        .is_some()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        let inside = self.picker.contains(p);
        let opener = self.popup;
        let pressed = self.with_controls(|c, parts| c.press(parts, p, 1));
        // A choice's list takes the click: a row taken; outside it, it closes — and the click
        // goes on, but on the field that opened it (that only closes it).
        if let Some((part, _)) = pressed {
            if part != LIST {
                return None;
            }
            let event = self.picker.take_event();
            self.picked(event);
            let same = matches!((&self.hit(p), opener), (Some(Hit::Choice(c)), Some(Popup::Choice(o))) if *c == o);
            if inside || same {
                return self.action.take();
            }
        }
        let hit = self.hit(p);

        match hit? {
            Hit::Field(field) => {
                self.focus = Some(field);
                if let Text::Name(i) | Text::Type(i) | Text::Default(i) = field {
                    self.selected = i;
                }
                if let Some(input) = self.focused_input() {
                    input.click(p.x);
                }
                // Clicked: every one, its value ticked (chosen once, it's offered again).
                self.suggest(field, true);
            }
            Hit::Tick(tick) => self.tick(tick),
            Hit::Choice(choice) => self.open_choice(choice),
            Hit::SelectColumn(i) => self.selected = i,
            Hit::RemoveColumn(i) => self.remove_column(i),
            Hit::AddKey => {
                if let Some(form) = &mut self.form {
                    form.keys.push(KeyRow::default());
                }
                self.replan();
            }
            Hit::RemoveKey(i) => {
                if let Some(form) = &mut self.form
                    && i < form.keys.len()
                {
                    form.keys.remove(i);
                }
                self.replan();
            }
            Hit::IndexColumn(name) => {
                if let Some(form) = &mut self.form {
                    form.toggle_index_column(&name);
                }
                self.replan();
            }
            Hit::IndexOrder(name) => {
                if let Some(form) = &mut self.form {
                    form.flip_index_order(&name);
                }
                self.replan();
            }
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.with_controls(|c, parts| c.drag(parts, p)).is_some() {
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
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_hover = Some(p);
        // An open list takes the mouse over everything.
        let parts = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        if self.choosing() || self.suggesting() {
            return parts;
        }
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | parts
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // A part's (an open list's, never what's under it).
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        match self.hit(p)? {
            Hit::Field(_) => Some(ViewerCursor::Text),
            _ => Some(ViewerCursor::Pointer),
        }
    }
}
