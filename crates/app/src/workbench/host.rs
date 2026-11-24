//! The workbench as modules see it: the [`WorkbenchHost`] port, through which contributed
//! commands read the active file and the repository, open choice lists, work in the
//! background and tell the user, and its capabilities (views, the editor, refactorings,
//! shared services, the desktop), each in its own block. Also: running a module's command.

use super::*;
use workbench_model::{
    ActiveFile, ChoiceList, Completion, EditorHost, Job, OnChoice, RefactoringHost, ServicesHost,
    SystemHost, ViewHost, WorkbenchHost,
};

impl WorkbenchHost for Workbench {
    fn selected_text(&self) -> Option<String> {
        Workbench::selected_text(self)
    }

    fn project_root(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|p| p.root_path().to_path_buf())
    }

    fn project_id(&self) -> Option<kernel::ProjectId> {
        self.project.as_ref().map(|_| self.data.id)
    }

    fn active_file(&self) -> Option<ActiveFile> {
        if self.menu_context.is_some() {
            return self.menu_target_file();
        }
        let tab = self.tabs.get(self.active)?;
        if tab.viewer.as_ref().is_some_and(|v| !v.editor_visible()) {
            return None;
        }
        let path = tab.doc.path().filter(|p| !is_virtual(p))?.to_path_buf();
        let primary = tab.doc.selections().primary();
        let line = tab.doc.buffer().line_of(primary.head);
        let selection = (!primary.is_empty()).then(|| primary.range());
        Some(ActiveFile { path, text: tab.text(), line, cursor: primary.head, selection })
    }

    fn active_path(&self) -> Option<PathBuf> {
        if self.menu_context.is_some() {
            return self.menu_target_text_path();
        }
        let tab = self.tabs.get(self.active)?;
        if tab.viewer.as_ref().is_some_and(|v| !v.editor_visible()) {
            return None;
        }
        tab.doc.path().filter(|p| !is_virtual(p)).map(Path::to_path_buf)
    }

    fn tabs(&self) -> Vec<workbench_model::TabInfo> {
        self.tabs
            .iter()
            .map(|t| workbench_model::TabInfo {
                id: t.doc.id(),
                title: t.title(),
                path: t.doc.path().map(Path::to_path_buf),
            })
            .collect()
    }

    fn context(&self) -> workbench_model::ActionContext {
        self.action_context()
    }

    fn path_marks(&self) -> Option<workspace::marks::ProjectMarks> {
        workbench_model::WorkbenchHost::project_root(self).map(|_| self.data.marks.clone())
    }

    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        self.data.git.clone()
    }

    fn confirm(
        &mut self,
        question: workbench_model::Question,
        then: workbench_model::OnYes,
    ) -> bool {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        let (tone, kind) = if question.danger {
            (Tone::Danger, ButtonKind::Destructive)
        } else {
            (Tone::Accent, ButtonKind::Primary)
        };
        self.open_dialog(DialogSpec {
            tone,
            icon: question.icon,
            title: question.title,
            message: question.message,
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("yes", question.yes, kind),
            ],
            cancel: "cancel",
            input: None,
            check: None,
            list: question.list,
        });
        self.dialogs.confirming = Some(Confirming(then));
        true
    }

    fn set_progress(&mut self, id: &'static str, progress: Option<workbench_model::Progress>) {
        self.progress.retain(|(p, _)| *p != id);
        if let Some(progress) = progress {
            self.progress.push((id, progress));
        }
    }

    fn set_menu_context(&mut self, context: Option<workbench_model::ActionContext>) {
        Workbench::set_menu_context(self, context);
    }

    fn view_command(&mut self, id: &str) -> bool {
        let used = self.viewer_command(id);
        if used {
            self.take_command_action();
        }
        used
    }

    fn ask_running(
        &mut self,
        project: kernel::ProjectId,
        owner: &str,
        then: workbench_model::OnYes,
    ) -> bool {
        let owner = owner.to_owned();
        self.running_request = Some(super::running::RunningRequest { project, owner, then });
        true
    }

    fn request_command(&mut self, id: command_model::CommandId) {
        self.request = Some(MouseResult::Command(id.0));
    }

    fn request_command_named(&mut self, id: &str) -> bool {
        let Some(found) = self.support.commands.iter().find(|c| c.spec.id.0 == id) else {
            return false;
        };
        self.request = Some(MouseResult::Command(found.spec.id.0));
        true
    }

    fn keyed_view(&self) -> Option<&'static str> {
        if self.keyboard_in_overlay() { None } else { self.keyed_view_kind() }
    }

    fn editor_focused(&self) -> bool {
        Workbench::editor_focused(self)
    }

    fn window_focused(&self) -> bool {
        Workbench::window_focused(self)
    }

    fn relist(&mut self, dir: &Path) {
        self.relist_dir(dir);
    }

    fn rename_entry(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        if to.exists() {
            return Err(format!("{} already exists", to.display()));
        }
        self.move_file(from, to).map_err(|e| e.to_string())
    }

    fn choose(&mut self, list: ChoiceList, then: OnChoice) -> bool {
        let choices = list
            .items
            .into_iter()
            .map(|item| ui_workbench::Choice {
                icon: item.icon,
                label: item.label,
                value: item.detail,
                shortcut: None,
                group: 0,
            })
            .collect();
        self.code_hover.hide();
        self.completion.close();
        let header = (list.title.as_str(), list.subject.as_str(), list.icon, list.action.as_str());
        self.choice_menu.open(header, choices, None);
        self.choice_purpose = ChoicePurpose::Contributed(then);
        self.choice_menu.is_open()
    }

    fn spawn(&mut self, job: Job) {
        let tx = self.data.jobs.tx.clone();
        let waker = self.waker.as_ref().map(|w| Arc::clone(&w.0));
        // A command's result: the user waits for it.
        self.data.jobs.modules.spawn(
            kernel::Priority::Interactive,
            Some(&self.data.jobs.cancel),
            move || {
                if tx.send(job()).is_ok()
                    && let Some(wake) = waker
                {
                    wake();
                }
            },
        );
    }

    fn notify(&mut self, notification: workbench_model::Notification) {
        self.show_notification(notification);
    }

    fn update_notification(&mut self, notification: workbench_model::Notification) {
        Workbench::update_notification(self, notification);
    }

    fn dismiss_notification(&mut self, id: workbench_model::NotificationId) {
        Workbench::dismiss_notification(self, id);
    }

    fn shows_notification(&self, id: workbench_model::NotificationId) -> bool {
        self.data.notifications.get(id).is_some_and(|t| !t.is_leaving())
    }

    fn answers(&self) -> Option<Arc<workbench_model::AnswerBook>> {
        Some(Arc::clone(&self.support.answers))
    }

    fn events(&self) -> &kernel::EventBus {
        &self.data.events
    }

    fn project_settings(&self) -> Option<&settings_model::SettingsService> {
        self.data.settings.as_deref()
    }

    fn user_settings(&self) -> Option<&'static settings_model::SettingsService> {
        Some(crate::config::settings())
    }

    fn language_of(&self, path: &Path) -> Option<String> {
        files::language_id(&self.support, path)
    }

    fn project_storage(&self) -> Option<PathBuf> {
        self.project_storage_dir()
    }
}

impl ViewHost for Workbench {
    fn anchor(&self, of: workbench_model::Anchor) -> Option<kernel::Rect> {
        match of {
            workbench_model::Anchor::TopBarLabel => self.chrome.top_bar.branch_anchor(),
        }
    }

    fn open_view(&mut self, title: &str, extension: &str, data: &dyn std::any::Any) -> bool {
        let dir = self.project.as_ref().map(|p| p.root_path().to_path_buf()).unwrap_or_default();
        let path = dir.join(format!("{title}.{extension}"));
        if let Some(g) = self.group_for_view(&path) {
            self.focus_group(g);
        }
        if !self.open_virtual(path) {
            return false;
        }
        self.set_viewer_data(data);
        self.focus = Focus::Editor;
        true
    }

    fn focus_view(&mut self, extension: &str) -> bool {
        self.focus_view_tab(extension)
    }

    fn open_view_below(&mut self, title: &str, extension: &str, data: &dyn std::any::Any) -> bool {
        Workbench::open_view_below(self, title, extension, data)
    }

    fn show_in_panel(
        &mut self,
        tool: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        Workbench::show_in_panel(self, tool, title, title, extension, data)
    }

    fn show_in_panel_keyed(
        &mut self,
        tool: &str,
        key: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        Workbench::show_in_panel(self, tool, key, title, extension, data)
    }

    fn add_to_panel_keyed(
        &mut self,
        tool: &str,
        key: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        Workbench::add_to_panel(self, tool, key, title, extension, data)
    }

    fn in_panel(&self, tool: &str, title: &str) -> bool {
        self.tools.windows.iter().any(|w| w.id == tool && w.views().any(|v| v.key == title))
    }

    fn open_modal(&mut self, title: &str, extension: &str, data: &dyn std::any::Any) -> bool {
        self.open_modal_view(title, extension, data)
    }

    fn open_overlay(&mut self, extension: &str, data: &dyn std::any::Any, popover: bool) -> bool {
        self.open_bare_view(extension, data, popover)
    }

    fn open_overlay_everywhere(&mut self, extension: &str, data: &dyn std::any::Any) -> bool {
        Workbench::open_everywhere(self, extension, data)
    }

    fn open_full_screen_everywhere(&mut self, extension: &str) -> bool {
        Workbench::open_full_everywhere(self, extension)
    }

    fn compare(&mut self, input: diff::CompareInput, title: &str) -> bool {
        let dir = input.right.0.parent().map(Path::to_path_buf).unwrap_or_default();
        if !self.open_virtual(dir.join(format!("{title}.{}", diff::COMPARE_VIEW))) {
            return false;
        }
        self.set_viewer_data(&input);
        self.focus = Focus::Editor;
        true
    }

    fn toggle_tool(&mut self, id: &str) -> bool {
        Workbench::toggle_tool(self, id)
    }

    fn show_side_panel(&mut self, id: &str) -> bool {
        Workbench::show_side_panel(self, id)
    }

    fn tell_panel(&mut self, tool: &str, data: &dyn std::any::Any, focus: bool) -> bool {
        if !self.show_side_panel(tool) {
            return false;
        }
        let Some(side) = self.tools.sides.iter_mut().find(|s| s.id == tool) else { return false };
        side.panel.set_data(data);
        if focus {
            self.focus = Focus::Side(side.dock);
        }
        true
    }

    fn set_tool_badge(&mut self, tool: &'static str, badge: Option<workbench_model::ToolBadge>) {
        self.data.tool_badges.retain(|(t, _)| *t != tool);
        if let Some(badge) = badge.filter(|b| b.count > 0) {
            self.data.tool_badges.push((tool, badge));
        }
    }
}

impl EditorHost for Workbench {
    fn file_text(&self, path: &Path) -> Option<String> {
        Workbench::file_text(self, path)
    }

    fn file_text_later(&self, path: &Path) -> workbench_model::FileText {
        if let Some(text) = self.open_text(path) {
            return workbench_model::FileText::Open(text);
        }
        let (disk, path) = (Arc::clone(&self.ports.disk), path.to_path_buf());
        workbench_model::FileText::Read(Box::new(move || super::disk::read_text(&*disk, &path)))
    }

    fn edit_active(&mut self, range: std::ops::Range<usize>, text: &str) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active).filter(|t| t.editor_visible()) else {
            return false;
        };
        let len = tab.doc.buffer().len();
        if range.start > range.end || range.end > len {
            return false;
        }
        if tab.locked() {
            tab.doc.refuse();
            return false;
        }
        let edit = document::Edit::new(range, text.to_owned());
        tab.doc.apply(document::Transaction::new(vec![edit]), None, document::EditKind::Command);
        true
    }

    fn refuse_if_locked(&mut self) -> bool {
        Workbench::refuse_if_locked(self)
    }

    fn unsaved_files(&self) -> Vec<std::path::PathBuf> {
        // The current group's tabs are `self.tabs`; the others' are kept in their groups.
        let current = self.editors.current;
        let others = self.editors.groups.iter().enumerate().filter(|(g, _)| *g != current);
        let tabs = self.tabs.iter().chain(others.flat_map(|(_, g)| g.tabs.iter()));
        let mut found: Vec<std::path::PathBuf> = tabs
            .filter(|t| t.untitled.is_none() && t.is_dirty())
            .filter_map(|t| t.doc.path().map(std::path::Path::to_path_buf))
            .collect();
        found.sort();
        found.dedup();
        found
    }

    fn save_active(&mut self) -> bool {
        let i = self.active;
        let Some(tab) = self.tabs.get(i).filter(|t| t.doc.path().is_some()) else { return false };
        // Unchanged: already as on disk (nothing saved, nobody told).
        !tab.doc.is_dirty() || self.save_tab(i, None).is_ok()
    }

    fn select_active(&mut self, range: std::ops::Range<usize>) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active).filter(|t| t.editor_visible()) else {
            return false;
        };
        let len = tab.doc.buffer().len();
        if range.start > range.end || range.end > len {
            return false;
        }
        commands::click(&mut tab.doc, range.start, false, false);
        commands::click(&mut tab.doc, range.end, true, false);
        self.focus = Focus::Editor;
        true
    }

    fn open_file(&mut self, path: &Path) -> bool {
        match Workbench::open_file(self, path) {
            Ok(()) => {
                self.focus = Focus::Editor;
                true
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "can't open");
                false
            }
        }
    }

    fn open_file_with(&mut self, path: &Path, data: &dyn std::any::Any) -> bool {
        let opened = workbench_model::EditorHost::open_file(self, path);
        if opened {
            self.set_viewer_data(data);
        }
        opened
    }

    fn active_document(&self) -> Option<DocumentId> {
        // A tree file's menu: only the editor's commands use it.
        if self.menu_context.is_some() {
            return None;
        }
        self.tabs.get(self.active).map(|t| t.doc.id())
    }

    fn type_at_cursor(&mut self) -> Option<String> {
        Workbench::type_at_cursor(self)
    }
}

impl RefactoringHost for Workbench {
    fn rename_at_cursor(&mut self) -> Result<refactor::application::RenameContext, String> {
        self.rename_context()
    }

    fn usages_at_cursor(&mut self) -> Result<refactor::application::RenameContext, String> {
        self.usages_context()
    }

    fn change_signature_at_cursor(
        &mut self,
    ) -> Result<refactor::signature::SignatureContext, String> {
        self.signature_context()
    }

    fn move_context(&mut self) -> Result<refactor::moving::MoveContext, String> {
        // What it acts on: the menu's entry, the panel's selection when it has the keyboard,
        // the active file.
        let from = self.action_context().path().map(Path::to_path_buf);
        let from = from.ok_or_else(|| i18n::t("refactor.nothing_to_move"))?;
        let into = from.parent().map(Path::to_path_buf).unwrap_or_default();
        self.move_context_for(&from, &into)
    }

    fn move_context_to(
        &mut self,
        from: &Path,
        into: &Path,
    ) -> Result<refactor::moving::MoveContext, String> {
        self.move_context_for(from, into)
    }

    fn rename_type_of_file(&mut self, path: &Path) -> Option<refactor::application::RenameContext> {
        Workbench::rename_type_of_file(self, path)
    }

    fn apply_refactoring(&mut self, plan: &refactor::Plan) -> Result<usize, String> {
        Workbench::apply_refactoring(self, plan)
    }

    fn undo_refactoring(&mut self) -> Option<usize> {
        Workbench::undo_refactoring(self)
    }
}

impl ServicesHost for Workbench {
    fn queries(&mut self) -> Option<Arc<dyn workbench_model::ProjectQueries>> {
        Workbench::queries(self)
    }

    fn resources(&self) -> Option<Arc<dyn workbench_model::ResourceSource>> {
        Some(Arc::clone(&self.task_data) as Arc<dyn workbench_model::ResourceSource>)
    }

    fn themes(&self) -> Option<theming::studio::StudioInput> {
        self.studio_input()
    }
}

impl SystemHost for Workbench {
    fn clipboard_text(&self) -> Option<String> {
        self.clipboard.as_ref().and_then(|read| (read.0)())
    }

    fn set_clipboard_text(&mut self, text: &str) {
        self.request = Some(MouseResult::CopyText(text.to_owned()));
    }

    fn clipboard_files(&self) -> Vec<PathBuf> {
        self.files_clipboard.as_ref().map(|c| (c.read)()).unwrap_or_default()
    }

    fn set_clipboard_files(&mut self, paths: &[PathBuf]) {
        if let Some(clipboard) = &self.files_clipboard {
            (clipboard.write)(paths);
        }
    }

    fn reveal_in_file_manager(&mut self, path: &Path) {
        self.request = Some(MouseResult::Reveal(path.to_path_buf()));
    }

    fn reveal_in_project(&mut self, path: &Path) -> bool {
        self.reveal_path_in_project(path)
    }

    fn open_project(&mut self, root: &Path) -> bool {
        // The app opens it (its dialog, the session, the theme).
        self.request = Some(MouseResult::OpenProject(root.to_path_buf()));
        true
    }

    fn open_projects(&self) -> Vec<workbench_model::ProjectWindow> {
        self.project_windows.clone()
    }

    fn set_project_number(&mut self, number: u8) -> bool {
        if !(1..=workbench_model::windows::NUMBERED).contains(&number) {
            return false;
        }
        self.request = Some(MouseResult::NumberProject(number));
        true
    }

    fn capture_window(&mut self) -> bool {
        self.capture_wanted = true;
        true
    }

    fn focus_project(&mut self, window: u64) -> bool {
        if !self.project_windows.iter().any(|p| p.window == window) {
            return false;
        }
        self.request = Some(MouseResult::FocusWindow(window));
        true
    }

    fn restart_to_update(&mut self, staged: &Path) -> bool {
        let request = ui_workbench::AppRequest::RestartToUpdate(staged.to_path_buf());
        self.request = Some(MouseResult::ViewerRequest(request));
        true
    }

    fn open_settings_page(&mut self, page: &str) -> bool {
        // The app opens them (it makes their model).
        let request = ui_workbench::AppRequest::OpenSettings(page.to_owned());
        self.request = Some(MouseResult::ViewerRequest(request));
        true
    }
}

impl Workbench {
    /// The windows' projects, as the app tells (this one's marked).
    pub fn set_project_windows(&mut self, windows: Vec<workbench_model::ProjectWindow>) {
        self.project_windows = windows;
    }

    /// This window's id among the windows with a project (`None`: no project open here).
    pub fn this_window(&self) -> Option<u64> {
        self.project_windows.iter().find(|p| p.current).map(|p| p.window)
    }

    /// Whether a module asked for this window's picture since the last call.
    pub fn take_capture_wanted(&mut self) -> bool {
        std::mem::take(&mut self.capture_wanted)
    }

    /// The number this project prefers among the open ones (`[project] number`).
    pub fn preferred_number(&self) -> Option<u8> {
        let settings = self.data.settings.as_deref()?;
        u8::try_from(settings.get(&crate::settings::PROJECT_NUMBER)).ok().filter(|n| *n > 0)
    }

    /// Keeps `number` as this project's preferred one (said in the log when it can't be).
    pub fn save_preferred_number(&mut self, number: u8) {
        let Some(settings) = self.data.settings.as_deref() else { return };
        if let Err(e) = settings.set(&crate::settings::PROJECT_NUMBER, i64::from(number)) {
            tracing::warn!(error = %e, "the project's number wasn't saved");
        }
    }

    /// Runs a module's command. `None` if no module contributed `id`.
    pub fn run_command(&mut self, id: command_model::CommandId) -> Option<bool> {
        let support = Arc::clone(&self.support);
        let handler =
            support.commands.iter().find(|c| c.spec.id == id).map(|c| &c.handler).or_else(
                || support.implementations.iter().find(|c| c.id == id).map(|c| &c.handler),
            )?;
        if !handler.enabled(self) {
            return Some(false);
        }
        Some(handler.run(self))
    }

    /// Runs a module's command by its id's text (from a view's button).
    /// The open project's data folder (outside the project).
    pub(crate) fn project_storage_dir(&self) -> Option<PathBuf> {
        crate::config::project_data_dir(self.project.as_ref()?.root_path())
    }

    pub fn run_command_named(&mut self, id: &str) -> Option<bool> {
        let command = self.support.commands.iter().find(|c| c.spec.id.0 == id)?.spec.id;
        self.run_command(command)
    }

    /// The completions of the modules' background jobs. `true` if any ran.
    pub fn poll_jobs(&mut self) -> bool {
        let done: Vec<Completion> = self.data.jobs.rx.try_iter().collect();
        let ran = !done.is_empty();
        for completion in done {
            completion(self);
        }
        ran
    }

    /// Row `i` of the choice list was chosen.
    pub(super) fn choice_picked(&mut self, i: usize) -> MouseResult {
        match std::mem::take(&mut self.choice_purpose) {
            ChoicePurpose::Copy(values) => {
                values.get(i).cloned().map_or(MouseResult::Redraw, MouseResult::CopyText)
            }
            ChoicePurpose::Contributed(then) => {
                then(i, self);
                MouseResult::Redraw
            }
            ChoicePurpose::Navigate(rows) => {
                if let Some(row) = rows.into_iter().nth(i) {
                    self.navigation_picked(row);
                }
                MouseResult::Redraw
            }
            ChoicePurpose::Actions(rows) => {
                self.data.actions.chosen = rows.into_iter().nth(i);
                MouseResult::Redraw
            }
            ChoicePurpose::Format(doc, picks) => {
                if let Some(pick) = picks.into_iter().nth(i) {
                    self.format_picked(doc, pick);
                }
                MouseResult::Redraw
            }
            ChoicePurpose::None => MouseResult::Redraw,
        }
    }
}
