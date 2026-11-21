//! The window as a tool panel.

use super::*;

impl ToolPanel for DatabaseWindow {
    fn update(&mut self, _ctx: &PanelContext<'_>) {}

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(view) = data.downcast_ref::<ProjectView>() {
            self.set_project(view.project);
        } else if let Some(reveal) = data.downcast_ref::<Reveal>() {
            self.take_changes();
            self.reveal(reveal);
        } else if let Some(crate::WindowCommand(id)) = data.downcast_ref() {
            self.run(id);
        }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.share_selection();
        if let Some(waker) = &ctx.waker {
            let mut slot = self.waker.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.is_none() {
                *slot = Some(Arc::clone(waker));
            }
        }
        // Shown again after it was hidden: what's open is read again.
        if self.loaders.is_empty() && self.sessions.is_some() {
            let needs = self.tree.open_needs();
            if !needs.is_empty() {
                self.ask(needs);
            }
        }
        self.refresh_rows();
        self.paint_window(ctx);
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.take_changes();
        changed |= self.take_replies();
        changed |= self.take_schema_answer();
        if changed {
            self.refresh_rows();
        }
        changed
    }

    fn parked(&mut self) {
        // Hidden: its reads given up (asked again when shown).
        self.loaders.clear();
        self.tree.forget_pending();
        self.stale = true;
    }

    fn purge(&mut self) {
        self.loaders.clear();
        self.tree.purge();
        // Which file each path names is looked up again (a link may point elsewhere now).
        if let Some(sessions) = &self.sessions {
            sessions.identities().clear();
        }
        self.stale = true;
    }

    fn scroll(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        let h = (ROW_HEIGHT * self.scale).round();
        let content = self.rows.len() as f32 * h + (12.0 * self.scale).round();
        let max = (content - self.list.size.height).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ToolPanelAction> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        // The filter (its caret put, or emptied by its ×): it has the keys; any other press
        // gives them back to the rows.
        if self.with_controls(|c, parts| c.press(parts, p, clicks)).is_some() {
            if self.filter.take_cleared() {
                self.filtered();
            }
            return self.action.take();
        }
        match self.hit(p) {
            Some(Hover::Field) | None => {}
            Some(Hover::Row(i)) => {
                let key = self.rows[i].key.clone();
                self.selected = Some(key.clone());
                if self.on_chevron(i, p) {
                    self.toggle(&key);
                } else if clicks == 2 {
                    self.activate(&key);
                }
            }
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            return changed;
        }
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        let field = self.with_controls(|c, parts| c.release(parts));
        self.bar.release() | field.is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let bar = self.bar.hover(p);
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        let field = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        changed || bar || field
    }

    fn clickable(&self, p: Point) -> bool {
        matches!(self.hit(p), Some(Hover::Row(_)))
            || self.cursor_at(p) == Some(ViewerCursor::Pointer)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        (self.hit(p) == Some(Hover::Field)).then(|| Control::cursor(&self.filter, p)).flatten()
    }

    /// A row's icon: what it stands for (a key, where a foreign key points, a view…).
    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        let i = self.row_at(p)?;
        let anchor = self.icon_rect(i);
        if !anchor.contains(p) {
            return None;
        }
        let label = self.icon_tip(&self.rows[i])?;
        Some(ui_components::tooltip::Tip::new(anchor, label))
    }

    fn wants_text(&self) -> bool {
        self.typing()
    }

    fn takes_keys(&self) -> bool {
        true
    }

    fn keyed(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        let taken = self.with_controls(|c, parts| c.text(parts, text)).is_some();
        if taken {
            self.filtered();
        }
        taken
    }

    /// Typing on the rows filters them (the field takes the keys).
    fn text_on_rows(&mut self, text: &str) -> bool {
        if text.chars().all(char::is_control) {
            return false;
        }
        self.set_typing(true);
        self.filter.input.select_all();
        self.text_input(text)
    }

    fn selected_text(&self) -> Option<String> {
        self.typing().then(|| Control::selected_text(&self.filter)).flatten()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if !self.typing() {
            return self.row_key(key);
        }
        // Esc empties the filter and gives the keys back to the rows.
        if key == ViewerKey::Escape {
            self.filter.set_text("");
            self.set_typing(false);
            self.filtered();
            return true;
        }
        // The filter's editing keys; any other goes back to the rows (↑ ↓ ↵ through what's
        // filtered).
        if self.with_controls(|c, parts| c.key(parts, key, reach)).is_some() {
            if matches!(key, ViewerKey::Backspace | ViewerKey::Delete) {
                self.filtered();
            }
            return true;
        }
        self.set_typing(false);
        self.row_key(key)
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "refresh" => self.run("refresh"),
            // The filter's editing commands while it has the keys (⌘A, ⌘X, ⌘Z, ⌘⇧Z).
            "select_all" | "cut" | "undo" | "redo" if self.typing() => {
                let used = self.with_controls(|c, parts| c.command(parts, id)).is_some();
                if used && id != "select_all" {
                    self.filtered();
                }
                used
            }
            "copy" if !self.typing() => self.run("copy-name"),
            // The picker closed with nothing: what it asked for is given up (the next file
            // picked, for another, isn't taken as its).
            "picked-file-cancelled" => {
                self.adding = None;
                false
            }
            id if id.starts_with("picked-file:") => {
                let Some(adding) = self.adding.take() else { return false };
                let path = PathBuf::from(&id["picked-file:".len()..]);
                match adding {
                    Adding::Driver(driver) => self.add_file(&path, Some(&driver)),
                    Adding::Any => self.add_file(&path, None),
                }
                self.stale = true;
                true
            }
            _ => false,
        }
    }

    fn header(&self) -> Option<Vec<PanelButton>> {
        let button =
            |id, icon, key: &str, group| PanelButton { id, icon, tooltip: i18n::t(key), group };
        Some(vec![
            button("add", "plus", "datasources.tooltip_add", 0),
            PanelButton {
                id: "console",
                icon: "console",
                tooltip: match ui_components::shortcuts::label(database::console::JUMP_COMMAND) {
                    Some(keys) => format!("{} ({keys})", i18n::t("datasources.tooltip_console")),
                    None => i18n::t("datasources.tooltip_console"),
                },
                group: 0,
            },
            button("refresh", "refresh", "datasources.tooltip_refresh", 0),
            button("collapse", "chevrons-collapse", "datasources.tooltip_collapse", 1),
        ])
    }

    fn header_icon(&self) -> Option<&'static str> {
        Some("database")
    }

    fn header_action(&mut self, id: &str) -> Option<ToolPanelAction> {
        match id {
            "add" => return Some(ToolPanelAction::Menu(self.add_menu())),
            "console" => {
                let key = self.selected.clone();
                self.console(key.as_ref(), false);
                return self.action.take();
            }
            "refresh" => self.refresh_asked(None),
            "collapse" => self.collapse_all(),
            _ => {}
        }
        None
    }

    fn menu_at(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if !self.bounds.contains(p) || self.field.contains(p) {
            return None;
        }
        let key = self.row_at(p).map(|i| self.rows[i].key.clone());
        if let Some(key) = &key {
            self.selected = Some(key.clone());
        }
        let entries = self.menu_of(key.as_ref());
        (!entries.is_empty()).then_some(entries)
    }

    fn menu_command(&mut self, id: &str) -> bool {
        self.run(id)
    }

    fn take_action(&mut self) -> Option<ToolPanelAction> {
        self.action.take()
    }
}
