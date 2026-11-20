//! The app commands (menu, shortcuts) and new windows.

use super::*;

impl EditorApp {
    /// Opens a new window in the same process (optionally with a project).
    pub(super) fn open_window(&mut self, path: Option<&Path>) {
        self.open_window_with(path.map(Path::to_path_buf).into_iter().collect());
    }

    /// A new window that opens the given paths, in order (project, files).
    pub(crate) fn open_window_with(&mut self, paths: Vec<PathBuf>) {
        let Some(window) = &self.window else { return };
        self.shared.pending.borrow_mut().push_back(paths);
        window.open_window();
    }

    /// Opens the paths in the newly created window (the project, then the files).
    pub fn open_initial_paths(&mut self, paths: &[PathBuf]) {
        for path in paths {
            let opened = self.workbench.open_path(path);
            self.succeeded(opened, Doing::Opening(path));
        }
        // Only a project, with no files given: its tabs from last time.
        if !paths.is_empty() && paths.iter().all(|p| p.is_dir()) {
            self.restore_session();
        }
        self.relayout();
    }

    /// The window's project's tabs from last time (and its own theme). Only the active tab
    /// is read for parsing now (sent to the parsing thread at once, beside the first frame's
    /// work); the others when shown.
    pub fn restore_session(&mut self) {
        if self.workbench.restore_session() {
            crate::startup::mark("session restored");
        }
        self.apply_theme();
        self.workbench.sync_syntax();
        self.relayout();
    }

    /// The rename dialog for the tab's file (the name without extension is selected).
    pub(super) fn ask_rename(&mut self, tab: usize) {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        // A view that can be renamed (a session's terminal): its tab's name, not a file's.
        if self.workbench.tab_renamable(tab) {
            self.workbench.ask_view_rename(tab);
            return;
        }
        let Some(name) = self
            .workbench
            .tab_path(tab)
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            return;
        };
        let stem = name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len());
        self.workbench.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "file-pen",
            title: i18n::t("dialog.rename_title"),
            message: i18n::tr!("dialog.rename_message", name = format!("**{name}**")),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("rename", i18n::t("dialog.rename"), ButtonKind::Primary),
            ],
            cancel: "cancel",
            input: Some((name, 0..stem)),
            check: None,
            list: Vec::new(),
        });
        self.pending.asking = Some(Ask::Rename(self.workbench.tab_id(tab)));
    }

    /// "Go to Line": the line (and column) to go to, `12` or `12:5`, starting from where
    /// the cursor is.
    fn ask_goto_line(&mut self) -> bool {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        let Some((line, column, lines)) = self.workbench.cursor_position() else { return false };
        let value = format!("{}:{}", line + 1, column + 1);
        let all = 0..value.len();
        self.workbench.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "hash",
            title: i18n::t("dialog.goto_line_title"),
            message: i18n::tr!("dialog.goto_line_message", lines = lines),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("go", i18n::t("dialog.goto_line"), ButtonKind::Primary),
            ],
            cancel: "cancel",
            input: Some((value, all)),
            check: None,
            list: Vec::new(),
        });
        self.pending.asking = Some(Ask::GotoLine);
        true
    }

    // ─── Commands ───

    /// A settings block's button that runs a command (the settings stay open): the log's
    /// folder shown or picked here (a modal's open, so not through [`Self::command`]), a
    /// module's through its handler.
    pub(super) fn settings_command(&mut self, id: CommandId) {
        let Some(window) = self.window.clone() else { return };
        match id {
            cmd::LOG_REVEAL_FOLDER => self.reveal_log_folder(&window),
            cmd::LOG_CHOOSE_FOLDER => self.choose_log_folder(&window),
            other => {
                self.workbench.run_command(other);
            }
        }
        self.redraw();
    }

    /// The log's folder shown in the file manager (made first: nothing logged there yet).
    fn reveal_log_folder(&self, window: &Arc<dyn Window>) {
        let Some(dir) = crate::logs::folder(crate::config::settings()) else { return };
        let _ = std::fs::create_dir_all(&dir);
        window.reveal_in_file_manager(&dir);
    }

    /// Another folder for the log, picked: saved and used from now, the settings shown again.
    fn choose_log_folder(&mut self, window: &Arc<dyn Window>) {
        let settings = crate::config::settings();
        let start = crate::logs::folder(settings);
        let picked = crate::watchdog::waiting_for_user(|| window.pick_folder(start.as_deref()));
        let Some(dir) = picked else { return };
        match settings.set(&crate::settings::LOG_DIR, dir.display().to_string()) {
            Ok(_) => crate::logs::apply(settings),
            Err(e) => tracing::warn!(error = %e, "can't save the log's folder"),
        }
        let model = self.settings_model();
        self.workbench.set_settings_model(model);
    }

    pub(super) fn command(&mut self, id: CommandId) -> Control {
        let Some(window) = self.window.clone() else { return Control::Continue };
        // A modal (settings, a dialog, About…) has the keyboard: only window-level commands,
        // plus the clipboard ones and undo / redo in its text field.
        if self.workbench.modal_open() {
            let clipboard = matches!(
                id,
                cmd::CUT | cmd::COPY | cmd::PASTE | cmd::SELECT_ALL | cmd::UNDO | cmd::REDO
            );
            // The menu drawer's own key closes it.
            // The zoom's, for a view over everything that zooms (a diagram's).
            let zoom = matches!(id, cmd::ZOOM_IN | cmd::ZOOM_OUT | cmd::ZOOM_RESET)
                && self.workbench.modal_view_zooms();
            let allowed = matches!(id, cmd::QUIT | cmd::NEW_WINDOW | cmd::ABOUT)
                || zoom
                || (clipboard && self.workbench.overlay_has_input())
                || (id == cmd::MENU && self.workbench.menu_drawer_open());
            if !allowed {
                return Control::Continue;
            }
        }
        // A side panel having the keyboard takes the editing commands it handles (the
        // tree's ⌘C / ⌘X / ⌘V / ⌘⌫ on files; ⇧F6 — the menu bar's Rename, wherever it was
        // pressed — renaming its entry instead of the editor's symbol).
        let editing = match id {
            cmd::COPY => Some("copy"),
            cmd::CUT => Some("cut"),
            cmd::PASTE => Some("paste"),
            cmd::DELETE_LINE => Some("delete"),
            cmd::TAB_RENAME | workbench_model::RENAME_SYMBOL => Some("rename"),
            cmd::SELECT_ALL => Some("select_all"),
            _ => None,
        };
        if let Some(name) = editing
            && self.with_text(|wb, text| wb.side_command(text, name)).unwrap_or(false)
        {
            self.redraw();
            return Control::Continue;
        }
        // What doesn't apply now (greyed out in the menus) does nothing from its keys either;
        // ⌘⇧T in the terminal makes another shell, whatever was closed.
        let in_panel = id == cmd::REOPEN_CLOSED_TAB && self.workbench.new_in_panel();
        if !in_panel && !self.workbench.command_applies(id) {
            return Control::Continue;
        }
        let changed = match id {
            cmd::QUIT => {
                // The user's own Quit: no update put in place behind their back.
                self.pending.update = None;
                return self.quit();
            }
            // Without a project, the search asks for one to open first.
            cmd::SEARCH_PROJECT if self.workbench.project_root().is_none() => {
                return self.command(cmd::OPEN_PROJECT);
            }
            // Each id belongs to one area: at most one of them takes it.
            _ => self
                .file_command(id, &window)
                .or_else(|| self.tabs_command(id, &window))
                .or_else(|| self.navigation_command(id))
                .or_else(|| self.editing_command(id, &window))
                .or_else(|| self.view_command(id, &window))
                .unwrap_or_else(|| self.other_command(id)),
        };
        if changed {
            self.relayout();
        }
        Control::Continue
    }

    /// Files and projects: new, open, save. `None` if `id` isn't one of them.
    fn file_command(&mut self, id: CommandId, window: &Arc<dyn Window>) -> Option<bool> {
        Some(match id {
            cmd::NEW_FILE => {
                self.workbench.new_file();
                true
            }
            cmd::OPEN_FILE => match crate::watchdog::waiting_for_user(|| {
                window.pick_file(self.workbench.dialog_dir().as_deref())
            }) {
                Some(path) => {
                    let opened = self.workbench.open_file(&path);
                    self.succeeded(opened, Doing::Opening(&path))
                }
                None => false,
            },
            cmd::OPEN_PROJECT => match crate::watchdog::waiting_for_user(|| {
                window.pick_folder(self.workbench.dialog_dir().as_deref())
            }) {
                Some(path) => self.open_project(&path),
                None => false,
            },
            cmd::CLOSE_PROJECT => self.close_project(),
            cmd::STOP_REOPEN => {
                self.shared.reopen_stop.set(true);
                true
            }
            cmd::CLEAR_RECENT => {
                crate::recent::update(|list| list.clear_recent());
                self.workbench.clear_recent_files();
                true
            }
            cmd::SAVE | cmd::SAVE_AS => match self.workbench.active_tab() {
                Some(i) => self.save(i, id == cmd::SAVE_AS),
                None => false,
            },
            _ => return None,
        })
    }

    /// The tabs and the project tree's menu. `None` if `id` isn't one of them.
    fn tabs_command(&mut self, id: CommandId, window: &Arc<dyn Window>) -> Option<bool> {
        Some(match id {
            // ⌘⇧T: in a bottom window that makes views (the terminal), another one;
            // otherwise, the last closed tab.
            cmd::REOPEN_CLOSED_TAB if self.workbench.new_in_panel() => true,
            cmd::REOPEN_CLOSED_TAB => self.workbench.reopen_closed_tab(),
            cmd::COPY_REFERENCE => {
                if let Some(reference) =
                    self.workbench.active_tab().and_then(|i| self.workbench.tab_reference(i))
                {
                    window.set_clipboard_text(&reference);
                }
                false
            }
            // In the editor, renaming means the symbol at the cursor (a refactoring).
            cmd::TAB_RENAME if self.workbench.editor_focused() => {
                self.workbench.run_command(workbench_model::RENAME_SYMBOL).unwrap_or(false)
            }
            cmd::TAB_RENAME => match self.workbench.active_tab() {
                Some(i) => {
                    self.ask_rename(i);
                    true
                }
                None => false,
            },
            cmd::CLOSE_TAB => match self.workbench.active_tab() {
                Some(i) => self.close_tab(i),
                None => false,
            },
            // New / Cut / Copy / Paste / Delete / Copy Path / Reveal: the project tree
            // module's commands, on what the menu was opened on.
            cmd::TREE_COPY_MENU => match self.pointer.tree_menu_path.take() {
                Some(path) => {
                    let anchor = self.pointer.menu_position.take();
                    self.workbench.open_copy_menu(&path, None, anchor)
                }
                None => false,
            },
            // From the keyboard: centered (not where an old context menu was).
            cmd::COPY_PATH_MENU => match self.workbench.active_tab() {
                Some(i) => {
                    self.pointer.menu_position = None;
                    self.tab_command(id, i)
                }
                None => false,
            },
            cmd::TAB_CLOSE
            | cmd::TAB_CLOSE_OTHERS
            | cmd::TAB_CLOSE_LEFT
            | cmd::TAB_CLOSE_RIGHT
            | cmd::TAB_CLOSE_SAVED
            | cmd::TAB_CLOSE_ALL
            | cmd::COPY_PATH
            | cmd::COPY_RELATIVE_PATH
            | cmd::REVEAL_IN_FILE_MANAGER
            | cmd::REVEAL_IN_PROJECT => match self.workbench.active_tab() {
                Some(i) => self.tab_command(id, i),
                None => false,
            },
            // The status bar's padlock, the menu bar, the palette: the active tab.
            cmd::TOGGLE_READ_ONLY => self.workbench.toggle_active_lock(),
            cmd::CHOOSE_LANGUAGE => self.workbench.choose_language(),
            cmd::CHOOSE_ENCODING => self.workbench.choose_encoding(),
            cmd::CHOOSE_LINE_ENDING => self.workbench.choose_line_ending(),
            cmd::CHOOSE_INDENT => self.workbench.choose_indent(),
            cmd::NEXT_TAB | cmd::PREV_TAB => {
                self.workbench.cycle_tab(id == cmd::NEXT_TAB);
                true
            }
            cmd::NEXT_GROUP | cmd::PREV_GROUP => self.workbench.cycle_group(id == cmd::NEXT_GROUP),
            cmd::CLOSE_GROUP => self.workbench.close_group(),
            _ => return None,
        })
    }

    /// Going somewhere: a line, a definition, a search, back / forward. `None` if `id`
    /// isn't one of them.
    fn navigation_command(&mut self, id: CommandId) -> Option<bool> {
        Some(match id {
            cmd::GOTO_LINE => self.ask_goto_line(),
            cmd::GOTO_DEFINITION => {
                self.with_text(|wb, text| wb.goto_definition(text)).unwrap_or(false)
            }
            cmd::GOTO_IMPLEMENTATION | cmd::GOTO_TYPE | cmd::TYPE_HIERARCHY => {
                use crate::workbench::Related;
                let kind = match id {
                    cmd::GOTO_IMPLEMENTATION => Related::Implementations,
                    cmd::GOTO_TYPE => Related::TypeDefinition,
                    _ => Related::Hierarchy,
                };
                self.with_text(|wb, text| wb.go_to_related(text, kind)).unwrap_or(false)
            }
            cmd::SEARCH_FILES => self.workbench.focus_file_search(),
            cmd::SEARCH_PROJECT => self.workbench.open_project_search(),
            cmd::NAVIGATE_BACK | cmd::NAVIGATE_FORWARD => {
                self.workbench.navigate(id == cmd::NAVIGATE_FORWARD)
            }
            _ => return None,
        })
    }

    /// The editor's text: completion, clipboard, find, comments, moving code, folding.
    /// `None` if `id` isn't one of them.
    fn editing_command(&mut self, id: CommandId, window: &Arc<dyn Window>) -> Option<bool> {
        Some(match id {
            cmd::COMPLETE_CODE => {
                self.with_text(|wb, text| wb.update_completion(text)).unwrap_or(false)
            }
            cmd::COPY => {
                if let Some(text) = self.workbench.copy() {
                    window.set_clipboard_text(&text);
                }
                false
            }
            cmd::CUT => {
                let cut = self.with_text(|wb, text| wb.cut(text)).flatten();
                if let Some(text) = &cut {
                    window.set_clipboard_text(text);
                }
                cut.is_some()
            }
            cmd::PASTE => match window.clipboard_text() {
                Some(clip) => self.with_text(|wb, text| wb.paste(text, &clip)).unwrap_or(false),
                None => false,
            },
            cmd::SELECT_ALL => self.with_text(|wb, text| wb.select_all(text)).unwrap_or(false),
            cmd::FIND | cmd::REPLACE => {
                let replace = id == cmd::REPLACE;
                self.with_text(|wb, text| wb.open_find(text, replace)).unwrap_or(false)
            }
            cmd::FIND_NEXT | cmd::FIND_PREV => {
                let forward = id == cmd::FIND_NEXT;
                self.with_text(|wb, text| wb.find_step(text, forward)).unwrap_or(false)
            }
            cmd::COMMENT => self.with_text(|wb, text| wb.toggle_comment(text)).unwrap_or(false),
            cmd::MOVE_STATEMENT_UP
            | cmd::MOVE_STATEMENT_DOWN
            | cmd::MOVE_LINE_UP
            | cmd::MOVE_LINE_DOWN => {
                let up = matches!(id, cmd::MOVE_STATEMENT_UP | cmd::MOVE_LINE_UP);
                let statements = matches!(id, cmd::MOVE_STATEMENT_UP | cmd::MOVE_STATEMENT_DOWN);
                self.with_text(|wb, text| wb.move_code(text, up, statements)).unwrap_or(false)
            }
            cmd::SHOW_ACTIONS => {
                self.with_text(|wb, text| wb.show_context_actions(text)).unwrap_or(false)
            }
            cmd::EXTEND_SELECTION | cmd::SHRINK_SELECTION => {
                let grow = id == cmd::EXTEND_SELECTION;
                self.with_text(|wb, text| wb.extend_selection(text, grow)).unwrap_or(false)
            }
            cmd::FOLD => self.workbench.fold(FoldCommand::Fold),
            cmd::UNFOLD => self.workbench.fold(FoldCommand::Unfold),
            cmd::FOLD_ALL => self.workbench.fold(FoldCommand::FoldAll),
            cmd::UNFOLD_ALL => self.workbench.fold(FoldCommand::UnfoldAll),
            _ => return None,
        })
    }

    /// The windows and what they show: settings, panels, themes, zoom, About. `None` if
    /// `id` isn't one of them.
    fn view_command(&mut self, id: CommandId, window: &Arc<dyn Window>) -> Option<bool> {
        Some(match id {
            cmd::OPEN_SETTINGS => {
                let model = self.settings_model();
                self.workbench.open_settings(model);
                true
            }
            cmd::KEYBOARD_SHORTCUTS => {
                let model = self.settings_model();
                self.workbench.open_settings(model);
                self.workbench.settings_show_page("keymap");
                true
            }
            cmd::NEW_WINDOW => {
                self.open_window(None);
                false
            }
            cmd::TOGGLE_PROJECT => self.workbench.toggle_project_panel(),
            cmd::CLEAR_CACHES => {
                self.clear_caches();
                true
            }
            cmd::ZOOM_IN => self.workbench.zoom_step(true),
            cmd::ZOOM_OUT => self.workbench.zoom_step(false),
            cmd::ZOOM_RESET => self.workbench.zoom_reset(),
            cmd::EDITOR_FULL => self.workbench.toggle_editor_full(true, true),
            cmd::EDITOR_FULL_WIDTH => self.workbench.toggle_editor_full(true, false),
            cmd::EDITOR_FULL_HEIGHT => self.workbench.toggle_editor_full(false, true),
            cmd::FULL_SCREEN => {
                window.toggle_fullscreen();
                true
            }
            cmd::ABOUT => {
                self.workbench.open_about();
                true
            }
            cmd::LOG_REVEAL_FOLDER => {
                self.reveal_log_folder(window);
                false
            }
            cmd::LOG_CHOOSE_FOLDER => {
                self.choose_log_folder(window);
                true
            }
            cmd::LANGUAGE_LEVEL => self.workbench.open_level_picker(),
            cmd::SWITCH_THEME => {
                self.shared.themes.borrow_mut().next();
                self.theme_changed();
                true
            }
            cmd::TOGGLE_DEBUG => {
                self.frames.overlay.toggle();
                true
            }
            cmd::MENU => self.workbench.toggle_menu_drawer(),
            cmd::CLOSE_NOTIFICATIONS => self.workbench.dismiss_notifications(),
            cmd::RECENT_NOTIFICATIONS => self.workbench.show_recent_notifications(),
            _ => return None,
        })
    }

    /// The rest: the document's edits (undo, duplicate, cursors…), then a module's
    /// command.
    fn other_command(&mut self, id: CommandId) -> bool {
        if matches!(id, cmd::UNDO | cmd::REDO)
            && let Some(done) = self.workbench.viewer_undo(id == cmd::REDO)
        {
            return done;
        }
        if id == cmd::DUPLICATE
            && let Some(done) = self.workbench.viewer_duplicate()
        {
            return done;
        }
        let op: Option<fn(&mut document::Document)> = match id {
            cmd::UNDO => Some(|d| {
                d.undo();
            }),
            cmd::REDO => Some(|d| {
                d.redo();
            }),
            cmd::DUPLICATE => Some(edit::duplicate),
            cmd::DELETE_LINE => Some(edit::delete_lines),
            cmd::SELECT_NEXT => Some(edit::select_next_occurrence),
            cmd::SELECT_ALL_OCCURRENCES => Some(edit::select_all_occurrences),
            cmd::UNSELECT_OCCURRENCE => Some(edit::unselect_occurrence),
            cmd::ADD_CURSOR_UP => Some(|d| edit::add_cursor_vertical(d, false)),
            cmd::ADD_CURSOR_DOWN => Some(|d| edit::add_cursor_vertical(d, true)),
            _ => None,
        };
        match op {
            Some(_) if self.workbench.view_keys_beside_text() => false,
            Some(op) => self.with_text(|wb, text| wb.edit(text, op)).unwrap_or(false),
            // A module's command.
            None => match self.workbench.run_command(id) {
                Some(changed) => changed,
                None => {
                    tracing::warn!(%id, "unhandled command");
                    false
                }
            },
        }
    }

    /// A command from the context menu of tab `tab`. `true` if a redraw is needed.
    pub(super) fn tab_command(&mut self, id: CommandId, tab: usize) -> bool {
        let Some(window) = self.window.clone() else { return false };
        if tab >= self.workbench.tab_count() {
            return false;
        }
        match id {
            cmd::TAB_CLOSE => self.close_tab(tab),
            cmd::TAB_CLOSE_OTHERS => self.close_tabs_where(tab, |i, _| i != tab),
            cmd::TAB_CLOSE_LEFT => self.close_tabs_where(tab, |i, _| i < tab),
            cmd::TAB_CLOSE_RIGHT => self.close_tabs_where(tab, |i, _| i > tab),
            cmd::TAB_CLOSE_SAVED => self.close_tabs_where(tab, |_, dirty| !dirty),
            cmd::TAB_CLOSE_ALL => self.close_tabs_where(tab, |_, _| true),
            cmd::COPY_PATH | cmd::COPY_RELATIVE_PATH => {
                let path = if id == cmd::COPY_PATH {
                    self.workbench.tab_path(tab)
                } else {
                    self.workbench.tab_relative_path(tab)
                };
                if let Some(path) = path {
                    window.set_clipboard_text(&path.to_string_lossy());
                }
                false
            }
            cmd::COPY_PATH_MENU => match self.workbench.tab_path(tab) {
                Some(path) => {
                    let anchor = self.pointer.menu_position.take();
                    self.workbench.open_copy_menu(&path, Some(tab), anchor)
                }
                None => false,
            },
            cmd::REVEAL_IN_FILE_MANAGER => {
                if let Some(path) = self.workbench.tab_path(tab) {
                    window.reveal_in_file_manager(&path);
                }
                false
            }
            cmd::REVEAL_IN_PROJECT => self.workbench.reveal_in_project(tab),
            cmd::TAB_PIN => self.workbench.toggle_pin(tab),
            cmd::TOGGLE_READ_ONLY => self.workbench.toggle_lock(tab),
            cmd::SPLIT_RIGHT => self.workbench.split_tab(tab, Split::Right, false),
            cmd::SPLIT_DOWN => self.workbench.split_tab(tab, Split::Down, false),
            cmd::SPLIT_MOVE_RIGHT => self.workbench.split_tab(tab, Split::Right, true),
            cmd::SPLIT_MOVE_DOWN => self.workbench.split_tab(tab, Split::Down, true),
            cmd::COPY_REFERENCE => {
                if let Some(reference) = self.workbench.tab_reference(tab) {
                    window.set_clipboard_text(&reference);
                }
                false
            }
            cmd::TAB_RENAME => {
                self.ask_rename(tab);
                true
            }
            cmd::TAB_NEW_WINDOW => {
                // The same project, with the file, in a new window; here the tab is closed.
                if let Some(path) = self.workbench.tab_path(tab) {
                    let mut paths = Vec::new();
                    if let Some(root) = self.workbench.project_root() {
                        paths.push(root);
                    }
                    paths.push(path);
                    self.open_window_with(paths);
                    self.close_tab(tab);
                }
                true
            }
            _ => false,
        }
    }

    /// Closes the tabs for which `close(index, modified)` is true, from right to left
    /// (the indices of the remaining ones don't change). Stops at "Cancel".
    /// Tab `keep` (if it stays open) becomes active.
    pub(super) fn close_tabs_where(
        &mut self,
        keep: usize,
        close: impl Fn(usize, bool) -> bool,
    ) -> bool {
        let keep_id = self.workbench.tab_id(keep);
        // Pinned tabs are not closed as a group.
        let ids: Vec<_> = (0..self.workbench.tab_count())
            .rev()
            .filter(|&i| !self.workbench.tab_pinned(i) && close(i, self.workbench.tab_dirty(i)))
            .map(|i| self.workbench.tab_id(i))
            .collect();
        let then = if ids.contains(&keep_id) { Final::Nothing } else { Final::Activate(keep_id) };
        self.start_closing(ids, then);
        true
    }
}
