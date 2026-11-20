//! Saving and closing: tabs, window, quit — with the "Save changes?" dialog.

use super::*;

impl EditorApp {
    /// Saves the tab; for new documents (or Save As) asks for the path.
    pub(super) fn save(&mut self, i: usize, save_as: bool) -> bool {
        let Some(window) = self.window.clone() else { return false };
        // A view (the canvas, a preview) isn't a file: no save dialog for it.
        if self.workbench.tab_is_view(i) {
            return false;
        }
        let path = if save_as || self.workbench.tab_path(i).is_none() {
            let dir = self.workbench.dialog_dir();
            match crate::watchdog::waiting_for_user(|| {
                window.pick_save_path(dir.as_deref(), &self.workbench.tab_title(i))
            }) {
                Some(path) => Some(path),
                None => return false,
            }
        } else {
            None
        };
        let result = self.workbench.save_tab(i, path);
        let name = self.workbench.tab_title(i);
        self.succeeded(result, Doing::Saving(&name))
    }

    /// Closes the tab (asks about changes, through a dialog). `true` = needs a redraw.
    pub(super) fn close_tab(&mut self, i: usize) -> bool {
        // A module's bar asks first (a console's transaction left open), and closes the tab
        // itself once answered.
        if !self.workbench.toolbar_may_close(i) {
            self.with_text(|wb, text| wb.take_toolbar_action(text));
            return true;
        }
        let id = self.workbench.tab_id(i);
        self.start_closing(vec![id], Final::Nothing);
        true
    }

    /// All tabs, then `then`. `true` if it finished without waiting for a dialog.
    pub(super) fn close_all_tabs(&mut self, then: Final) -> bool {
        // The session (tabs, splits, terminal) is kept as it is now; then all editor groups
        // into one, so the dialogs see every tab.
        self.workbench.freeze_session();
        self.workbench.merge_groups();
        let ids = (0..self.workbench.tab_count()).rev().map(|i| self.workbench.tab_id(i)).collect();
        self.start_closing(ids, then)
    }

    /// Starts closing the given tabs (from right to left). `true` if it finished
    /// immediately (no modified tab).
    pub(super) fn start_closing(&mut self, tabs: Vec<kernel::DocumentId>, then: Final) -> bool {
        if self.workbench.dialog_open() {
            return false;
        }
        self.pending.closing = Some(Closing::new(tabs, then));
        self.continue_closing()
    }

    /// Closes the tabs in the queue up to the first modified one (which opens the dialog).
    /// `true` if it finished.
    pub(super) fn continue_closing(&mut self) -> bool {
        use document::CloseStep;
        loop {
            let Some(closing) = &mut self.pending.closing else { return true };
            match closing.next(&OpenTabs(&self.workbench)) {
                CloseStep::Done => {
                    let then =
                        self.pending.closing.take().map_or(Final::Nothing, Closing::into_then);
                    self.finish(then);
                    return true;
                }
                CloseStep::Ask(id) => {
                    if let Some(i) = self.workbench.tab_index(id) {
                        self.ask_save(id, i);
                    }
                    return false;
                }
                CloseStep::Close(id) => {
                    if let Some(i) = self.workbench.tab_index(id) {
                        self.workbench.close_tab(i);
                    }
                }
            }
        }
    }

    /// What follows after the tabs are closed.
    pub(super) fn finish(&mut self, then: Final) {
        match then {
            Final::Nothing => {}
            Final::Activate(id) => {
                if let Some(i) = self.workbench.tab_index(id) {
                    self.workbench.activate(i);
                }
            }
            Final::CloseWindow => {
                tracing::info!("the tabs are closed; closing the window");
                if !self.begin_ending(Control::Close) {
                    self.pending.exit_request = Some(Control::Close);
                }
            }
            Final::Quit => {
                if !self.begin_ending(Control::Exit) {
                    self.pending.exit_request = Some(Control::Exit);
                }
            }
            Final::CloseProject => {
                self.workbench.close_project();
                // Its own theme (if it had one) gives way to the global one.
                self.apply_theme();
            }
            Final::OpenProject(path) => {
                let opened = self.workbench.open_project(&path);
                if self.succeeded(opened, Doing::Opening(&path)) {
                    self.workbench.restore_session();
                    self.apply_theme();
                }
            }
        }
        self.relayout();
    }

    /// The "Save changes?" dialog for tab `i`.
    pub(super) fn ask_save(&mut self, id: kernel::DocumentId, i: usize) {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        self.workbench.activate(i);
        let name = self.workbench.tab_title(i);
        self.workbench.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "file-pen",
            title: i18n::t("dialog.unsaved_title"),
            message: i18n::tr!("dialog.unsaved_message", name = format!("**{name}**")),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("discard", i18n::t("dialog.discard"), ButtonKind::Destructive),
                DialogButton::new("save", i18n::t("dialog.save"), ButtonKind::Primary),
            ],
            cancel: "cancel",
            input: None,
            check: None,
            list: Vec::new(),
        });
        self.pending.asking = Some(Ask::Save(id));
    }

    /// The dialog's answer: continues the action that requested it.
    pub(super) fn on_dialog(&mut self, answer: &'static str) {
        tracing::debug!(answer, asking = ?self.pending.asking, "dialog answer");
        match self.pending.asking.take() {
            Some(Ask::Save(id)) => {
                let index = self.workbench.tab_index(id);
                match (answer, index) {
                    ("save", Some(i)) => {
                        if !self.save(i, false) {
                            self.pending.closing = None;
                            self.pending.update = None;
                            return;
                        }
                        self.workbench.close_tab(i);
                    }
                    ("discard", Some(i)) => self.workbench.close_tab(i),
                    _ => {
                        // Cancelled: the whole closing stops (the window / the quit / the
                        // project remain), its session followed again; an update waiting on it,
                        // not now.
                        self.pending.closing = None;
                        self.pending.update = None;
                        self.workbench.thaw_session();
                        self.relayout();
                        return;
                    }
                }
                if let Some(closing) = &mut self.pending.closing {
                    closing.resolved();
                }
                self.continue_closing();
                self.relayout();
            }
            Some(Ask::Rename(id)) => {
                let value = self.workbench.take_dialog_value();
                // A class's file: the class is renamed with it (a module's refactoring, with
                // its preview); any other file is just renamed.
                if let (Some(i), "rename", Some(name)) =
                    (self.workbench.tab_index(id), answer, value)
                    && !self.workbench.rename_file(i, &name)
                    && let Err(e) = self.workbench.rename_tab(i, &name)
                {
                    tracing::warn!(error = %e, "the rename failed");
                }
                self.relayout();
            }
            Some(Ask::GotoLine) => {
                let value = self.workbench.take_dialog_value();
                if answer == "go"
                    && let Some((line, column)) = value.as_deref().and_then(parse_line_column)
                {
                    self.with_text(|wb, text| wb.goto_position(text, line, column));
                }
                self.relayout();
            }
            Some(Ask::DeleteProject(path, open)) => {
                let to_trash = self.workbench.take_dialog_checked() && !open;
                if answer == "remove" {
                    self.delete_project(&path, to_trash);
                    self.relayout();
                }
            }
            Some(Ask::AlreadyOpen(path, window)) => match answer {
                "go" => {
                    if let Some(own) = &self.window {
                        own.focus_window(window);
                    }
                }
                // A second window of it (this one, holding something, stays as it is).
                "again" if self.workbench.has_content() => self.open_window(Some(&path)),
                "again" => {
                    self.open_project_here_or_new(&path);
                    self.relayout();
                }
                _ => {}
            },
            Some(Ask::OpenTarget(path)) => match answer {
                "new" => self.open_window(Some(&path)),
                "this" => {
                    // Another project here: this one closes (its work still running asked
                    // about first).
                    if let Some(id) = self.workbench.project_identity().map(|(id, _)| id)
                        && self.ask_running(
                            workbench_model::CloseScope::project(id),
                            workbench_model::running::Closing::Project,
                            Final::OpenProject(path.clone()),
                        )
                    {
                        return;
                    }
                    self.close_all_tabs(Final::OpenProject(path));
                }
                _ => {}
            },
            Some(Ask::Running) => {
                let answer = match answer {
                    "stop" => workbench_model::CloseAnswer::StopAndClose,
                    _ => workbench_model::CloseAnswer::Cancel,
                };
                self.on_running_answer(answer);
                self.relayout();
            }
            Some(Ask::LeftBehind) => self.on_left_behind(answer),
            None => {}
        }
    }

    /// Quitting the app (⌘Q): the work still running anywhere asked about first (here,
    /// once for every window), then this window's files; `AppWindows` asks the others about
    /// theirs.
    pub(super) fn quit(&mut self) -> Control {
        use workbench_model::running::Closing;
        // Already ending: it quits by itself once what it started has ended.
        if self.ending() {
            return Control::Continue;
        }
        if self.ask_running(workbench_model::CloseScope::quit(), Closing::App, Final::Quit) {
            return Control::Continue;
        }
        self.close_all_tabs(Final::Quit);
        self.pending.exit_request.take().unwrap_or(Control::Continue)
    }

    /// The update this window asked to put in place once the app has quit (its staged
    /// folder), taken by `AppWindows` as the app exits.
    pub fn take_update(&mut self) -> Option<PathBuf> {
        self.pending.update.take()
    }

    /// Requested by `AppWindows` on quit, for the other windows: `true` if the window was
    /// emptied without waiting for a dialog.
    pub fn prepare_quit(&mut self) -> bool {
        self.close_all_tabs(Final::Nothing) && self.workbench.tab_count() == 0
    }

    /// Closing the window (its button): only this one, the others remain. Its project's
    /// work still running is asked about first (the app's too, when it's the last window).
    pub(super) fn close_window(&mut self) -> Control {
        use workbench_model::running::Closing;
        // Already ending: the window closes by itself once what the app started has ended.
        if self.ending() {
            return Control::Continue;
        }
        let project = self.workbench.project_identity().map(|(id, _)| id);
        let scope = workbench_model::CloseScope::window(project, self.shared.windows.get() <= 1);
        if self.ask_running(scope, Closing::Window, Final::CloseWindow) {
            return Control::Continue;
        }
        self.close_all_tabs(Final::CloseWindow);
        self.pending.exit_request.take().unwrap_or(Control::Continue)
    }

    /// File › Close Project: as closing the window, the window kept — its unsaved files
    /// asked about (Cancel keeps everything as it is), its session saved for next time;
    /// then its data goes and the start screen shows. `false` without a project.
    pub(super) fn close_project(&mut self) -> bool {
        use workbench_model::running::Closing;
        let Some((id, _)) = self.workbench.project_identity() else { return false };
        // Its work still running asked about first (only its own).
        let scope = workbench_model::CloseScope::project(id);
        if !self.ask_running(scope, Closing::Project, Final::CloseProject) {
            self.close_all_tabs(Final::CloseProject);
        }
        true
    }

    /// Before closing `scope`: the work still running there asked about, once — `true` if
    /// it was (the closing then waits on the answer: [`Self::on_running_answer`]), `false`
    /// with nothing to ask (the closing goes on). Asked before the unsaved files: Cancel
    /// here leaves the window exactly as it was.
    pub(super) fn ask_running(
        &mut self,
        scope: workbench_model::CloseScope,
        closing: workbench_model::running::Closing,
        then: Final,
    ) -> bool {
        // Already asking (the window's button again): the question stands.
        if self.pending.running.is_some() {
            return true;
        }
        self.ask_running_about(scope, closing, None, super::state::AfterRunning::Close(then))
    }

    /// A module's restart (a reconnect) asked through its host: its own work still running
    /// for its project asked about first, as before closing; what it asked to run runs once
    /// that's let go — at once when nothing runs, never on Cancel.
    pub(super) fn ask_running_request(&mut self, request: crate::workbench::RunningRequest) {
        use workbench_model::running::Closing;
        let crate::workbench::RunningRequest { project, owner, then } = request;
        // Already asking (a closing): this restart isn't asked over it.
        if self.pending.running.is_some() {
            tracing::info!(%owner, "a restart asked while another question stands: dropped");
            return;
        }
        let scope = workbench_model::CloseScope::project(project);
        let after = super::state::AfterRunning::Run(then);
        if !self.ask_running_about(scope, Closing::Restart, Some(owner), after) {
            // Nothing runs: `then` already ran.
            self.relayout();
        }
    }

    /// Asks about what still runs in `scope` (only `owner`'s, when given) — `true` if it
    /// did; else `then` goes on at once (a closing: `false`, its caller closes the tabs; a
    /// module's: run here).
    fn ask_running_about(
        &mut self,
        scope: workbench_model::CloseScope,
        closing: workbench_model::running::Closing,
        owner: Option<String>,
        then: super::state::AfterRunning,
    ) -> bool {
        let items = if self.workbench.dialog_open() {
            Vec::new()
        } else {
            self.workbench.running_work_of(&scope, owner.as_deref())
        };
        if items.is_empty() {
            if let super::state::AfterRunning::Run(run) = then {
                run(&mut self.workbench);
            }
            return false;
        }
        tracing::info!(count = items.len(), ?closing, "work still running: asked before closing");
        let count = items.len();
        let others = self.shared.project_names.borrow().clone();
        let prompt = self.workbench.running_prompt(closing, items, false, &others);
        let modal = self.workbench.open_running_prompt(&prompt);
        if !modal {
            self.ask_running_plainly(closing, count);
        }
        self.pending.running = Some(super::state::RunningAsk {
            scope,
            closing,
            owner,
            then,
            waiting: false,
            modal,
            looked: Instant::now(),
        });
        self.redraw();
        true
    }

    /// Without the module's modal: the app's plain question (Cancel, Stop and close).
    fn ask_running_plainly(&mut self, closing: workbench_model::running::Closing, count: usize) {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        use workbench_model::running::Closing;
        self.workbench.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "clock",
            title: i18n::t(match closing {
                Closing::Window => "running.title_window",
                Closing::Project => "running.title_project",
                Closing::App => "running.title_app",
                Closing::Restart => "running.title_restart",
            }),
            message: i18n::tr!("running.dialog_message", count = count),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("running.cancel"), ButtonKind::Secondary),
                DialogButton::new(
                    "stop",
                    i18n::t("running.stop_and_close"),
                    ButtonKind::Destructive,
                ),
            ],
            cancel: "cancel",
            input: None,
            check: None,
            list: Vec::new(),
        });
        self.pending.asking = Some(Ask::Running);
    }

    /// The answer to the question about the work still running: stopped and the closing
    /// goes on; waited for (the closing goes on by itself once it ends,
    /// [`Self::follow_running`]); or nothing closes.
    pub(super) fn on_running_answer(&mut self, answer: workbench_model::CloseAnswer) {
        use workbench_model::CloseAnswer;
        tracing::info!(?answer, "running work: answered");
        match answer {
            CloseAnswer::StopAndClose => {
                let Some(ask) = self.pending.running.take() else { return };
                self.workbench.close_running_prompt();
                self.workbench.stop_running_of(&ask.scope, ask.owner.as_deref());
                self.close_after_running(ask.then);
            }
            CloseAnswer::Wait => {
                if let Some(ask) = &mut self.pending.running {
                    ask.waiting = true;
                    ask.looked = Instant::now();
                }
            }
            CloseAnswer::Cancel => {
                self.pending.running = None;
                // An update waiting on this closing: not now.
                self.pending.update = None;
                self.workbench.close_running_prompt();
            }
        }
        self.redraw();
    }

    /// The closing let go: its tabs (asking about the unsaved ones), then `then`; a module's
    /// restart: what it asked to run.
    fn close_after_running(&mut self, then: super::state::AfterRunning) {
        match then {
            super::state::AfterRunning::Close(then) => {
                self.close_all_tabs(then);
            }
            super::state::AfterRunning::Run(run) => {
                run(&mut self.workbench);
                self.relayout();
            }
        }
    }

    /// A closing held by the work still running, at `now`: its question closed without an
    /// answer (×) is a Cancel; while waiting, the work is looked at again (twice a second)
    /// — once it's all done the question closes and the closing goes on, else the question
    /// follows it.
    pub(super) fn follow_running(&mut self, now: Instant) {
        const EVERY: std::time::Duration = std::time::Duration::from_millis(500);
        let Some(ask) = &self.pending.running else { return };
        if ask.modal && !self.workbench.running_prompt_open() {
            tracing::info!("running work: the question closed, nothing closes");
            self.pending.running = None;
            return;
        }
        if !ask.waiting || now.saturating_duration_since(ask.looked) < EVERY {
            return;
        }
        let (scope, closing, owner) = (ask.scope, ask.closing, ask.owner.clone());
        if let Some(ask) = &mut self.pending.running {
            ask.looked = now;
        }
        let items = self.workbench.running_work_of(&scope, owner.as_deref());
        if items.is_empty() {
            tracing::info!("running work: done; the closing goes on");
            let Some(ask) = self.pending.running.take() else { return };
            self.workbench.close_running_prompt();
            self.close_after_running(ask.then);
            self.redraw();
            return;
        }
        let others = self.shared.project_names.borrow().clone();
        let prompt = self.workbench.running_prompt(closing, items, true, &others);
        self.workbench.update_running_prompt(&prompt);
    }

    /// Opens a project here or in a new window (if something is already open here, asks
    /// through a dialog).
    pub(super) fn open_project(&mut self, path: &Path) -> bool {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        // Open in another window already: the user decides — its window, or a second one.
        let here = self.window.as_ref().map(|w| w.id());
        let elsewhere = self
            .shared
            .open_projects
            .borrow()
            .iter()
            .find(|(w, p)| p == path && Some(*w) != here)
            .map(|(w, _)| *w);
        if let Some(window) = elsewhere {
            let name = path
                .file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            self.workbench.open_dialog(DialogSpec {
                tone: Tone::Accent,
                icon: "folder-open",
                title: i18n::tr!("dialog.already_open_title", name = name),
                message: i18n::t("dialog.already_open_message"),
                buttons: vec![
                    DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                    DialogButton::new("again", i18n::t("dialog.open_again"), ButtonKind::Secondary),
                    DialogButton::new("go", i18n::t("dialog.go_to_window"), ButtonKind::Primary),
                ],
                cancel: "cancel",
                input: None,
                check: None,
                list: Vec::new(),
            });
            self.pending.asking = Some(Ask::AlreadyOpen(path.to_path_buf(), window));
            return true;
        }
        self.open_project_here_or_new(path)
    }

    /// `path` opened: asked where when this window holds something, else here.
    fn open_project_here_or_new(&mut self, path: &Path) -> bool {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        if self.workbench.has_content() {
            let name = path
                .file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            self.workbench.open_dialog(DialogSpec {
                tone: Tone::Accent,
                icon: "folder-open",
                title: i18n::tr!("dialog.open_title", name = name),
                message: i18n::t("dialog.open_message"),
                buttons: vec![
                    DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                    DialogButton::new("this", i18n::t("dialog.this_window"), ButtonKind::Secondary),
                    DialogButton::new("new", i18n::t("dialog.new_window"), ButtonKind::Primary),
                ],
                cancel: "cancel",
                input: None,
                check: None,
                list: Vec::new(),
            });
            self.pending.asking = Some(Ask::OpenTarget(path.to_path_buf()));
            return true;
        }
        let result = self.workbench.open_project(path);
        let opened = self.succeeded(result, Doing::Opening(path));
        if opened {
            self.workbench.restore_session();
            self.apply_theme();
        }
        opened
    }

    // ─── Keyboard ───
}

/// `12`, `12:5` or `12,5` (1-based, as shown) → 0-based line and column.
pub(super) fn parse_line_column(value: &str) -> Option<(usize, usize)> {
    let mut parts = value.trim().split([':', ',', ' ']).filter(|p| !p.is_empty());
    let line: usize = parts.next()?.parse().ok()?;
    let column: usize = parts.next().map_or(Some(1), |c| c.parse().ok())?;
    Some((line.saturating_sub(1), column.saturating_sub(1)))
}

#[cfg(test)]
#[path = "../../tests/unit/app/closing.rs"]
mod tests;
