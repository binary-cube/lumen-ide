//! The bar as an editor's toolbar (`EditorToolbar`): its tab's title, icon and crumbs, the
//! mouse, the commands, the statements followed in the editor.

use super::*;

impl EditorToolbar for ConsoleBar {
    fn height(&self) -> f32 {
        HEIGHT
    }

    fn paint(&mut self, ctx: ToolbarPaint<'_>) {
        self.refresh();
        self.paint_bar(ctx);
    }

    fn pointer_down(&mut self, p: Point) -> Option<ToolbarAction> {
        let tool = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t)?;
        if !self.enabled(tool) {
            return None;
        }
        self.press(tool)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let tool = p.and_then(|p| self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t));
        let changed = tool != self.hover;
        self.hover = tool;
        // On Run: the statement it runs highlighted; leaving it: the highlight goes.
        if tool == Some(Tool::Run) {
            self.highlight = self.current.clone();
        } else if self.highlight.take().is_some() {
            return true;
        }
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let tool = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t)?;
        (self.enabled(tool)
            && !matches!(tool, Tool::Status | Tool::Limited | Tool::ReadOnly | Tool::Production))
        .then_some(ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (rect, tool) = self.hits.iter().find(|(r, _)| r.contains(p))?;
        let (label, keys) = self.tooltip_of(*tool)?;
        Some(Tip::new(*rect, label).with_shortcut(keys))
    }

    fn poll(&mut self) -> bool {
        self.refresh()
    }

    fn is_loading(&self) -> bool {
        self.snapshot.running.is_some()
    }

    fn title(&self) -> Option<String> {
        // An attached file keeps its own name.
        (!self.spec.is_attached()).then(|| self.title.clone())
    }

    /// Its server's icon; the generic database one is the `.sql` file's own (as the file
    /// icon set draws it), so the file's is kept then.
    fn icon(&self) -> Option<&'static str> {
        if self.spec.is_attached() {
            return None;
        }
        self.brief.as_ref().map(|b| b.icon).filter(|icon| *icon != GENERIC_ICON)
    }

    fn accent(&self, palette: &Palette) -> Option<Rgba> {
        let color = self.brief.as_ref()?.color?;
        ui_components::tag_colors::named(&color.name(), palette)
    }

    fn tab_badge(&self, palette: &Palette) -> Option<(String, Rgba)> {
        let tx = self.snapshot.tx;
        tx.is_open().then(|| {
            let color = if tx.is_failed() { palette.danger } else { palette.warning };
            (i18n::t("console.tab_tx"), color)
        })
    }

    fn crumbs(&self) -> Vec<String> {
        if self.spec.is_attached() {
            return Vec::new();
        }
        let source = self.brief.as_ref().map(|b| b.name.clone()).unwrap_or_default();
        vec![i18n::t("console.crumb_consoles"), source, self.spec.name.clone()]
    }

    fn may_close(&mut self) -> bool {
        self.refresh();
        // Its transaction answered for (committing, or rolled back): it closes.
        if self.consoles.take_allowed_close(self.console) || !self.snapshot.tx.is_open() {
            return true;
        }
        let console = self.console;
        self.with_host(move |host| {
            crate::commands::close(host, console);
        });
        false
    }

    fn moved(&mut self, path: &std::path::Path) -> bool {
        // Still this console's file, of the same source: the same bar under the new name.
        // Still a console (or still an attached file): another kind is another bar.
        match self.consoles.follow_file(self.console, path) {
            Some(spec) if spec.source == self.source && spec.kind == self.spec.kind => {
                let old = self.spec.path(self.consoles.root());
                self.consoles.checks().forget(&self.consoles, &old);
                self.spec = spec;
                self.layout = Memo::default();
                self.refresh();
                true
            }
            _ => false,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        let update = |bar: &mut Self, change: &dyn Fn(&mut ConsoleSpec)| {
            if let Some(spec) = bar.consoles.update(bar.console, |s| change(s)) {
                bar.spec = spec;
            }
        };
        match id {
            "run" => self.run_command(crate::RUN.0),
            "run-script" => self.run_command(crate::RUN_SCRIPT.0),
            "explain" => self.run_command(crate::EXPLAIN.0),
            "explain-analyze" => self.run_command(crate::EXPLAIN_ANALYZE.0),
            "tx:auto" => update(self, &|s| s.tx = TxMode::Auto),
            "tx:manual" => update(self, &|s| s.tx = TxMode::Manual),
            "isolation:default" => update(self, &|s| s.isolation = None),
            id if id.starts_with("isolation:") => {
                let level = Isolation::from_id(&id["isolation:".len()..]);
                update(self, &|s| s.isolation = level);
            }
            "mode:playground" => update(self, &|s| s.mode = RunMode::Playground),
            "mode:script" => update(self, &|s| s.mode = RunMode::Script),
            id if id.starts_with("fetch:") => {
                let Ok(n) = id["fetch:".len()..].parse::<i64>() else { return false };
                self.save_setting(crate::commands::SettingChange::FetchLimit(n));
            }
            id if id.starts_with("profile-option:") => {
                let Some(switch) =
                    crate::settings::ProfileSwitch::from_id(&id["profile-option:".len()..])
                else {
                    return false;
                };
                let on = !crate::settings::profile_switch(switch);
                self.save_setting(crate::commands::SettingChange::Profile(switch, on));
            }
            "stop-on-error" => {
                let on = !crate::settings::stop_on_error();
                self.save_setting(crate::commands::SettingChange::StopOnError(on));
            }
            id if id.starts_with("schema:") => {
                let schema = id["schema:".len()..].to_owned();
                update(self, &|s| s.schema = Some(schema.clone()));
            }
            id if id.starts_with("db:") => {
                if self.snapshot.in_transaction {
                    self.notifications
                        .push(Notification::warning(i18n::t("console.tx_open_switch")));
                    return false;
                }
                let database = id["db:".len()..].to_owned();
                update(self, &|s| {
                    s.database = Some(database.clone());
                    s.schema = None;
                });
                self.runner.disconnect();
            }
            id if id.starts_with("source:") => {
                let Some(source) = DataSourceId::parse(&id["source:".len()..]) else {
                    return false;
                };
                return self.attach_to(Some(source));
            }
            "detach" => return self.attach_to(None),
            "more:history" => self.run_command(crate::HISTORY.0),
            "more:mode" => self.action = self.menu(Tool::More, self.mode_menu()),
            "more:settings" => self.action = self.menu(Tool::More, self.settings_menu()),
            "more:services" => self.run_command(workbench_model::TOGGLE_SERVICES.0),
            "none" => {}
            _ => return false,
        }
        self.layout = Memo::default();
        true
    }

    fn take_action(&mut self) -> Option<ToolbarAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn follows_editor(&self) -> bool {
        // Always: the statement the cursor is in is marked beside its lines (Run hovered
        // highlights it too); an error underlined is gone once its statement is edited; the
        // text is checked against the schema as it changes.
        true
    }

    fn sync_editor(&mut self, state: &EditorState) {
        if self.brief.as_ref().is_some_and(|b| b.dialect) {
            let path = self.spec.path(self.consoles.root());
            self.consoles.checks().schedule(
                &self.consoles,
                &path,
                Arc::clone(&state.text),
                state.version,
            );
        }
        if let Some((range, hash)) = &self.snapshot.error_mark {
            let now = state.text.get(range.clone()).map(ui_components::editor_toolbar::text_hash);
            if now != Some(*hash) {
                self.snapshot.error_mark = None;
                self.runner.clear_diagnostics();
            }
        }
        self.editor = Some(state.clone());
        self.current = self.planned_range();
        self.highlight = if self.hover == Some(Tool::Run) { self.current.clone() } else { None };
        self.focused = self.focus_range();
        // The statement profiled was edited: what its finding pointed at isn't there.
        if self.snapshot.focus.is_some() && self.focused.is_none() {
            self.snapshot.focus = None;
            self.runner.set_focus(None);
        }
    }

    fn marks(&self) -> (&[StatementMark], u64) {
        (&self.snapshot.marks, self.snapshot.marks_generation)
    }

    fn highlight(&self) -> Option<Range<usize>> {
        self.highlight.clone()
    }

    fn current(&self) -> Option<Range<usize>> {
        self.current.clone()
    }

    fn focus(&self) -> Option<(Range<usize>, ui_components::Level)> {
        let level = self.snapshot.focus.as_ref()?.level;
        Some((self.focused.clone()?, level))
    }

    fn purge(&mut self) {
        self.layout = Memo::default();
        self.runner.purge();
    }

    fn grammar_hints(&self) -> bool {
        crate::settings::grammar_hints()
    }
}
