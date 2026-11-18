//! Files and tabs: opening, saving, closing, pinning, navigation.

use super::*;

impl Workbench {
    /// Opens a folder as a project, or a file if `path` isn't a directory.
    pub fn open_path(&mut self, path: &Path) -> io::Result<()> {
        if self.is_dir(path) { self.open_project(path) } else { self.open_file(path) }
    }

    pub fn open_project(&mut self, root: &Path) -> io::Result<()> {
        let started = std::time::Instant::now();
        let root = simplify_path(self.ports.disk.canonical(root)?);
        let tree = open_project(
            root,
            &listing::ListedFs::new(&*self.ports.disk, &self.support.project_services),
        )?;
        tracing::info!(root = %tree.root_path().display(), "project opened");
        if self.remember_projects {
            crate::recent::add(tree.root_path());
            self.home.recent = self.recent_projects();
        }
        self.drop_project_data();
        // Its own settings, in its folder (`<project dir>/project.toml`, the other tables kept).
        let file = tree.root_path().join(kernel::brand::PROJECT_DIR).join("project.toml");
        let store = (self.ports.project_settings)(file);
        self.data.settings = Some(Arc::new(settings_model::SettingsService::new(store)));
        // The project's files changing, counted (what's gathered from them follows).
        let vcs_dirs = self.support.vcs.iter().filter_map(|v| v.metadata_dir()).collect();
        let counted = self.data.generations.follow(&self.data.events, tree.root_path(), vcs_dirs);
        self.data.subscriptions.extend(counted);
        let reads = self.data.disk_reads.follow(&self.data.events);
        self.data.subscriptions.extend(reads);
        self.start_symbol_index(tree.root_path().to_path_buf());
        self.open_repository(tree.root_path());
        let waker: Arc<dyn Fn() + Send + Sync> = match &self.waker {
            Some(w) => Arc::clone(&w.0),
            None => Arc::new(|| {}),
        };
        let dirs = workspace::WatchedDirs {
            vcs: self.support.vcs.iter().filter_map(|v| v.metadata_dir()).collect(),
            own: self.support.project_services.iter().flat_map(|s| s.own_dirs()).copied().collect(),
        };
        self.tree_watcher = self.ports.watching.watch(tree.root_path(), dirs, waker);
        self.data.described = Some(crate::recent::describe(tree.root_path()));
        self.project = Some(tree);
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        if let Some(root) = root.as_ref() {
            self.attach_project_services(root);
            // What the services know beyond the disk (a remote project's files), in the app's
            // tree. Not announced: the project panel lists it after this, the services
            // attached, and an announced root would have the indexes walk the whole project
            // again (they start with their own walk).
            self.relist_own(root);
        }
        if let Some(root) = root {
            self.load_levels(&root);
            self.load_frameworks(&root);
        }
        self.data.recent_files = self
            .project
            .as_ref()
            .map(|p| self.recent_writes.load(p.root_path()))
            .unwrap_or_default();
        // The project panel (the tree's module) shows the new project.
        self.show_side_panel(PROJECT_TOOL);
        self.layout();
        if let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) {
            self.data.events.publish(&workspace::events::ProjectOpened { root });
        }
        tracing::info!(elapsed = ?started.elapsed(), "project ready");
        Ok(())
    }

    /// Everything from the previous project goes away: its data (indexes, results,
    /// histories, its events and their subscriptions — the modules' services, its language
    /// servers), its bottom windows (its shells, its Git log…: made for it, they'd keep
    /// showing and following it; its session already kept the ones that come back) and what
    /// its panels kept of themselves (its session has it).
    fn drop_project_data(&mut self) {
        self.data = ProjectData::new();
        self.entry_changes = entry_events::EntryChanges::listen(&self.data.events);
        self.set_bottom(None);
        self.tools.windows.clear();
        self.tools.mementos.clear();
        self.session.reset();
        self.project_theme = None;
    }

    /// File › Close Project, once its tabs are closed (their changes asked about) and its
    /// session saved: the project goes as a whole — its data, its watcher, its windows and
    /// panels — and the window shows the start screen, the project first among the recent
    /// ones. Nothing of it is left behind (another window's same project keeps its own).
    pub fn close_project(&mut self) {
        let Some(tree) = self.project.take() else { return };
        let root = tree.root_path().to_path_buf();
        drop(tree);
        tracing::info!(root = %root.display(), "project closed");
        if self.remember_projects {
            crate::recent::add(&root);
        }
        self.tree_watcher = None;
        self.drop_project_data();
        // Its panels (the tree, the structure…) were made for it: made again when shown.
        self.tools.sides.clear();
        self.tools.parked.clear();
        self.focus = Focus::Editor;
        // What pointed into it.
        self.back.clear();
        self.forward.clear();
        self.fetching.clear();
        self.pending_goto = None;
        self.pending_offset = None;
        self.beside_compare = None;
        self.menu_context = None;
        self.progress.clear();
        self.index_memory.set(None);
        // The start screen: the recent projects, its filter empty.
        self.home.recent = self.recent_projects();
        self.home.files = None;
        self.home.picker.clear_query();
        self.layout();
        // The Task Manager no longer lists it.
        self.refresh_task_data();
    }

    /// The modules' project services start following this project (their subscriptions end
    /// with its data).
    fn attach_project_services(&mut self, root: &Path) {
        let Some(storage) = crate::config::project_data_dir(root) else { return };
        let support = Arc::clone(&self.support);
        let wake: Arc<dyn Fn() + Send + Sync> = match &self.waker {
            Some(w) => Arc::clone(&w.0),
            None => Arc::new(|| {}),
        };
        // The project's own events (a cheap handle on its bus).
        let events = self.data.events.clone();
        let id = self.data.id;
        let languages = Arc::clone(&support);
        self.data.notifier.set_wake(Arc::clone(&wake));
        let notifier: Arc<dyn workbench_model::Notifier> = self.data.notifier.clone();
        let project = workbench_model::ProjectContext {
            id,
            root,
            storage: &storage,
            events: &events,
            marks: self.data.marks.clone(),
            wake,
            settings: self.data.settings.clone(),
            language_of: Arc::new(move |path: &Path| language_id(&languages, path)),
            quality_tools: support.quality_tools.clone(),
            text_formatters: support.text_formatters.clone(),
            notifier,
        };
        for service in &support.project_services {
            let subscriptions = service.attach(&project);
            self.data.subscriptions.extend(subscriptions);
        }
    }

    /// Opens the file in a new tab, or activates the tab where it's already open.
    pub fn open_file(&mut self, path: &Path) -> io::Result<()> {
        // The bottom window over all the height: back to its size, the file in view.
        if self.tools.bottom_size == tool_windows::BottomSize::Full {
            self.tools.bottom_size = tool_windows::BottomSize::Kept;
            self.layout();
        }
        // Not on disk yet (a remote project's file): brought first, opened then.
        if !self.exists(path) && self.fetch_to_open(path) {
            return Ok(());
        }
        let path = simplify_path(self.ports.disk.canonical(path)?);
        if let Some(i) = self.tabs.iter().position(|t| t.doc.path() == Some(path.as_path())) {
            self.activate(i);
            return Ok(());
        }
        // Does a module know how to show it (image, Markdown)?
        let provider =
            self.support.viewers.iter().find_map(|p| Some((Arc::clone(p), p.kind(&path)?)));
        if let Some((provider, ui_workbench::ViewerKind::Replace)) = &provider {
            let mut viewer = provider.open(&path)?;
            self.give_sizes(viewer.as_mut());
            viewer.set_data(&self.restored());
            let mut doc = Document::new("");
            doc.mark_saved(path.clone());
            let mut tab = Tab::new(doc, self.code_font.clone(), None, LanguageId::PLAIN_TEXT);
            tab.seen_on_disk(&*self.ports.disk);
            tab.viewer = Some(TabViewer::new(
                viewer,
                provider.id(),
                ui_workbench::ViewerKind::Replace,
                ViewMode::Preview,
            ));
            tracing::info!(path = %path.display(), viewer = provider.id(), "file opened in a viewer");
            self.push_tab(tab);
            return Ok(());
        }
        let started = std::time::Instant::now();
        let mut doc = open_document(&path, &*self.ports.disk)?;
        tracing::info!(path = %path.display(), lines = doc.buffer().line_count(), elapsed = ?started.elapsed(), "file opened");
        let ruler = apply_editorconfig(&mut doc, &path, &*self.ports.disk);
        let head = doc.buffer().snapshot().slice(0..SNIFF_BYTES.min(doc.buffer().len()));
        let language = self.support.languages.detect(&path, &head);
        tracing::info!(%language, path = %path.display(), "language detected");
        // A module's bar above its text (a query console's, a server to download).
        let toolbar = self.toolbar_for(&path, Some(language.as_str()));
        let mut tab = Tab::new(doc, self.code_font.clone(), None, language);
        tab.seen_on_disk(&*self.ports.disk);
        tab.view.set_ruler(ruler);
        tab.toolbar = toolbar;
        if let Some((provider, kind)) = provider {
            match provider.open(&path) {
                Ok(mut viewer) => {
                    self.give_sizes(viewer.as_mut());
                    viewer.set_data(&self.restored());
                    // A preview shown first (a CSV's data), the editor a toggle away.
                    let mode =
                        if viewer.opens_alone() { ViewMode::Preview } else { ViewMode::Split };
                    let mut v = TabViewer::new(viewer, provider.id(), kind, mode);
                    // Split at the last ratio the user chose for this viewer's tabs.
                    v.split = self.new_preview_split(provider.id());
                    tab.viewer = Some(v);
                }
                Err(e) => tracing::warn!("no preview for {}: {e}", path.display()),
            }
        }
        self.push_tab(tab);
        Ok(())
    }

    /// The project a file's view is in (given to it when it opens).
    fn restored(&self) -> ui_workbench::Restored {
        ui_workbench::Restored {
            project: self.project.as_ref().map(|p| p.root_path().to_path_buf()),
            id: workbench_model::WorkbenchHost::project_id(self),
            ..ui_workbench::Restored::default()
        }
    }

    pub fn new_file(&mut self) {
        let n = self.next_untitled;
        self.next_untitled += 1;
        self.push_tab(Tab::new(
            Document::new(""),
            self.code_font.clone(),
            Some(n),
            LanguageId::PLAIN_TEXT,
        ));
    }

    /// Does the window already have a project or files open?
    pub fn has_content(&self) -> bool {
        self.project.is_some() || !self.tabs.is_empty()
    }

    pub fn active_tab(&self) -> Option<usize> {
        (!self.tabs.is_empty()).then_some(self.active)
    }

    pub fn tab_title(&self, i: usize) -> String {
        self.tabs[i].title()
    }

    pub fn tab_dirty(&self, i: usize) -> bool {
        self.tabs[i].is_dirty()
    }

    pub fn tab_path(&self, i: usize) -> Option<PathBuf> {
        self.tabs[i].doc.path().map(Path::to_path_buf)
    }

    /// The files open in every group (views and untitled tabs aside), in order: what the
    /// app's session keeps of this window.
    pub fn open_file_paths(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for tab in super::tab::all_tabs(&self.tabs, &self.editors.groups) {
            if let Some(path) = tab.doc.path().filter(|p| !is_virtual(p))
                && !out.iter().any(|p| p == path)
            {
                out.push(path.to_path_buf());
            }
        }
        out
    }

    /// The tab is a view (canvas, comparison, a refactoring's preview…), not a file: there's
    /// nothing to save — unless its view says it has something unsaved (a file it edits in
    /// its own way: its save writes it).
    pub fn tab_is_view(&self, i: usize) -> bool {
        self.tabs.get(i).is_some_and(|t| {
            t.doc.path().is_some_and(is_virtual)
                && !t.viewer.as_ref().is_some_and(|v| v.viewer.is_dirty())
                || t.viewer.as_ref().is_some_and(|v| !v.editor_visible() && !v.viewer.is_dirty())
                    && t.doc.path().is_none()
        })
    }

    /// The project's language levels (saved, from its configuration, from the environment as
    /// found so far — never waiting for it).
    fn load_levels(&mut self, root: &Path) {
        self.levels_warmed = crate::levels::warm_generation();
        let support = Arc::clone(&self.support);
        let intelligence = &support.intelligence;
        let store = Arc::clone(&self.ports.levels);
        self.data.levels =
            self.with_project(|project| crate::levels::load(&*store, root, intelligence, project));
    }

    /// The environment was looked at since the levels were loaded (the installed PHP found in
    /// the background): loaded again. `true` if they were.
    pub(super) fn refresh_levels_if_warmed(&mut self) -> bool {
        if self.levels_warmed == crate::levels::warm_generation() {
            return false;
        }
        let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
            self.levels_warmed = crate::levels::warm_generation();
            return false;
        };
        self.load_levels(&root);
        true
    }

    /// Where the keys are, for the keymap: a view (its actions), or the editor; nothing
    /// in particular over a modal or in a view's field (only what works everywhere).
    pub fn key_place(&self) -> Option<command_model::When> {
        if self.keyboard_in_overlay() {
            return None;
        }
        if let Some(view) = self.keyed_view_kind() {
            return Some(command_model::When::View(view));
        }
        let typing_in_view = self.focus == Focus::Panel
            || self
                .tabs
                .get(self.active)
                .and_then(|t| t.viewer.as_ref())
                .is_some_and(|v| v.has_keys());
        (!typing_in_view).then_some(command_model::When::Editor)
    }

    /// Where the keys are, the innermost first: the editor of a file's language (when a
    /// module's command works there: a console's ⌘↵), then the editor; a view; nothing in
    /// particular over a modal.
    pub fn key_places(&self) -> Vec<command_model::When> {
        let Some(place) = self.key_place() else { return Vec::new() };
        if place != command_model::When::Editor {
            // A view inside a side panel (a Services tab): its keys first, then the panel's.
            let panel = self
                .keyed_side()
                .map(|side| command_model::When::View(side.id))
                .filter(|p| *p != place);
            return std::iter::once(place).chain(panel).collect();
        }
        let language = self.tabs.get(self.active).map(|t| t.syntax.language().clone());
        let own = language.and_then(|l| crate::keymaps::editor_language(l.as_str()));
        own.into_iter().chain([place]).collect()
    }

    /// The kind of view that has the keys (the bottom window's when it's focused, else the
    /// active tab's when it has no editor), unless one of its fields takes them.
    pub(super) fn keyed_view_kind(&self) -> Option<&'static str> {
        if self.focus == Focus::Panel {
            return self.keyed_panel_view();
        }
        if let Some(side) = self.keyed_side() {
            return Some(side.panel.keyed_view().unwrap_or(side.id));
        }
        let viewer = self.tabs.get(self.active)?.viewer.as_ref()?;
        // A view taking the keyboard whole (a terminal) has its keys too, as at the bottom.
        let keyed = !viewer.viewer.wants_text() || viewer.viewer.takes_keyboard();
        (viewer.has_keys() && keyed).then_some(viewer.provider)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// The tab under the cursor (for the context menu).
    pub fn tab_at(&self, p: Point) -> Option<usize> {
        match self.tab_bar.hit(p)? {
            TabHit::Activate(i) | TabHit::Close(i) => Some(i),
            TabHit::Back
            | TabHit::Forward
            | TabHit::MoreLeft
            | TabHit::MoreRight
            | TabHit::Maximize => None,
        }
    }

    /// The tab's path relative to the project (or absolute, if outside it).
    pub fn tab_relative_path(&self, i: usize) -> Option<PathBuf> {
        let path = self.tabs[i].doc.path()?;
        let root = self.project.as_ref().map(FileTree::root_path);
        Some(root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path).to_path_buf())
    }

    /// Selects the tab's file in the project tree. `false` if it's not in the project.
    pub fn reveal_in_project(&mut self, i: usize) -> bool {
        match self.tabs[i].doc.path().map(Path::to_path_buf) {
            Some(path) => self.reveal_path_in_project(&path),
            None => false,
        }
    }

    /// Shows `path` in the project panel (the tree's module), selected; a folder opened
    /// too if `expand`. `false`: no project panel.
    pub(super) fn reveal_path_in_project(&mut self, path: &Path) -> bool {
        self.tell_project_panel(
            workbench_model::EntriesRequest::Reveal { path: path.to_path_buf(), expand: false },
            false,
        )
    }

    /// Tells the project panel `request` (it's shown first), the keyboard given if `focus`.
    pub(super) fn tell_project_panel(
        &mut self,
        request: workbench_model::EntriesRequest,
        focus: bool,
    ) -> bool {
        self.project.is_some()
            && workbench_model::ViewHost::tell_panel(self, PROJECT_TOOL, &request, focus)
    }

    /// Saves the tab to its path or to `path` (Save As).
    pub fn save_tab(&mut self, i: usize, path: Option<PathBuf>) -> io::Result<()> {
        let tab = &mut self.tabs[i];
        // Images have no text: only their transformations (rotate, flip) are saved, by the
        // viewer; the empty document is never written over the file.
        if let Some(v) = tab.viewer.as_mut().filter(|v| v.kind == ui_workbench::ViewerKind::Replace)
        {
            v.viewer.save()?;
            // A view whose save is a file's text (a comparison's result): saved from that
            // file's tab, announced like any save.
            if let Some(ui_workbench::ViewerAction::SaveText { path, text }) =
                v.viewer.take_action()
            {
                return self.save_text(&path, &text);
            }
            return Ok(());
        }
        // One save of a file at a time: an automatic one under way finishes first.
        if let Some(target) = path.clone().or_else(|| tab.doc.path().map(Path::to_path_buf)) {
            self.finish_saves_of(&target);
        }
        let tab = &mut self.tabs[i];
        // A new file (or one saved elsewhere) takes on the destination's `.editorconfig` rules.
        if let Some(target) = &path {
            let ruler = apply_editorconfig(&mut tab.doc, target, &*self.ports.disk);
            tab.view.set_ruler(ruler);
        }
        // The final newline: `.editorconfig`'s choice, else the setting (on by default).
        let cleanup = tab.doc.save_cleanup();
        tab.doc.set_save_cleanup(cleanup.or_final_newline(crate::final_newline::enabled()));
        // What's on disk is still the previous version (the local history keeps it).
        let disk = &*self.ports.disk;
        let is_file = |p: &&Path| disk.kind(p) == Some(NodeKind::File);
        if let Some(existing) = path.as_deref().or(tab.doc.path()).filter(is_file) {
            let event = document::events::FileWillSave { path: existing.to_path_buf() };
            self.data.events.publish(&event);
        }
        let encoding = tab.doc.encoding();
        let saved = save_document(&mut tab.doc, path, &*self.ports.disk);
        tab.doc.set_save_cleanup(cleanup);
        saved?;
        let reencoded = tab.doc.encoding() != encoding;
        tab.disk_changed = false;
        tab.seen_on_disk(&*self.ports.disk);
        tab.saved_text = Some(text_fingerprint(tab.doc.buffer().chunks()));
        tab.untitled = None;
        tracing::info!(path = ?tab.doc.path(), "file saved");
        // What was written (for the Task Manager).
        if let Some((len, _)) = tab.doc.path().and_then(|p| self.ports.disk.stamp(p)) {
            self.data.disk.written(len);
        }
        // The same file in other groups (their copies follow this one).
        let source = &self.tabs[i].doc;
        let saved = (source.path().map(Path::to_path_buf), self.tabs[i].saved_text);
        let format = (source.encoding(), source.line_ending());
        if let (Some(path), Some(text)) = saved {
            self.saved_everywhere(&path, text, format);
        }
        // Whoever cares (the indexer, git) is subscribed; the watcher reports it later anyway.
        if let Some(path) = self.tabs[i].doc.path() {
            let event = document::events::FileSaved { path: path.to_path_buf() };
            self.data.events.publish(&event);
        }
        if reencoded {
            let name = self.tabs[i].title();
            let message = i18n::tr!("files.saved_as_utf8", name = name, from = encoding.label());
            workbench_model::WorkbenchHost::notify(
                self,
                workbench_model::Notification::info(message),
            );
        }
        Ok(())
    }

    /// Automatic saving: once typing has paused (`now`: at once — another tab, the window
    /// left), the changed files that already exist on disk are written. `true` if any was.
    pub fn autosave(&mut self, now: bool) -> bool {
        if !crate::autosave::enabled() {
            return false;
        }
        if !now && self.autosave_deadline().is_none_or(|at| std::time::Instant::now() < at) {
            return false;
        }
        self.last_edit = None;
        // Every group's tabs, not only the current one's. A file open in several groups is
        // written once, from the current group (where typing happens: the others' copies
        // only follow it, and may not have caught up yet).
        let mut seen = std::collections::HashSet::new();
        let mut saved = self.autosave_group(&mut seen);
        let current = self.editors.current;
        for g in (0..self.editors.groups.len()).filter(|&g| g != current) {
            self.swap_group(g);
            saved |= self.autosave_group(&mut seen);
            self.swap_group(g);
        }
        saved
    }

    /// Autosaves the current group's changed files not in `seen` (their paths are added).
    /// `true` if any was written.
    fn autosave_group(&mut self, seen: &mut std::collections::HashSet<PathBuf>) -> bool {
        let mut saved = false;
        for i in 0..self.tabs.len() {
            let tab = &self.tabs[i];
            let Some(path) = tab.doc.path() else { continue };
            if !seen.insert(path.to_path_buf()) {
                continue;
            }
            let due = tab.untitled.is_none() && tab.holds_text() && tab.doc.is_dirty();
            if !due {
                continue;
            }
            // Changed on disk meanwhile: only the user's own save writes over it (said in the
            // log: why a changed file wasn't saved).
            if tab.disk_changed {
                tracing::info!(?path, "autosave: not written, the file changed on disk meanwhile");
                continue;
            }
            // Changed on disk right now, not reported yet (or missed): looked at as the
            // watcher's report would be, never written over.
            if tab.changed_on_disk(&*self.ports.disk) {
                let path = path.to_path_buf();
                let (seen, now) = (tab.disk_stamp, self.ports.disk.stamp(&path));
                tracing::info!(
                    ?path,
                    ?seen,
                    ?now,
                    "autosave: not written, its stamp on disk differs"
                );
                self.reload_written(std::slice::from_ref(&path));
                continue;
            }
            // Written on the worker (the disk's sync is never the UI's wait).
            saved |= self.save_in_background(i);
        }
        saved
    }

    /// When the pause in typing will be long enough to save (a redraw is scheduled then).
    pub fn autosave_deadline(&self) -> Option<std::time::Instant> {
        if !crate::autosave::enabled() {
            return None;
        }
        self.last_edit.map(|at| at + crate::autosave::IDLE)
    }

    /// Closes the tab without asking (the app handles confirming changes).
    pub fn close_tab(&mut self, i: usize) {
        let tab = self.tabs.remove(i);
        // For "Reopen Closed Tab" (⌘⇧T).
        if let Some(path) = tab.doc.path().filter(|_| tab.untitled.is_none()) {
            self.data.closed.retain(|p| p != path);
            self.data.closed.push(path.to_path_buf());
            if self.data.closed.len() > 30 {
                self.data.closed.remove(0);
            }
        }
        if let Some(worker) = &self.worker {
            worker.forget(tab.doc.id());
        }
        // An unsaved document's symbols have no reason to stay in the index.
        if tab.doc.path().is_none()
            && let Ok(mut index) = self.data.symbols.write()
        {
            index.remove_file(&Self::index_path(&tab));
        }
        if self.active >= i && self.active > 0 {
            self.active -= 1;
        }
        self.focus = Focus::Editor;
        self.layout();
        self.drop_empty_group();
    }

    /// Reopens the last closed tab (that still exists on disk).
    pub fn reopen_closed_tab(&mut self) -> bool {
        while let Some(path) = self.data.closed.pop() {
            if self.exists(&path) && self.open_file(&path).is_ok() {
                return true;
            }
        }
        false
    }

    pub fn tab_pinned(&self, i: usize) -> bool {
        self.tabs.get(i).is_some_and(|t| t.pinned)
    }

    /// Pins / unpins the tab; pinned ones stay first (in pinning order).
    pub fn toggle_pin(&mut self, i: usize) -> bool {
        let Some(tab) = self.tabs.get_mut(i) else { return false };
        tab.pinned = !tab.pinned;
        let pinned = tab.pinned;
        let id = tab.doc.id();
        let tab = self.tabs.remove(i);
        let slot = self.tabs.iter().take_while(|t| t.pinned).count();
        let slot = if pinned { slot } else { slot.min(self.tabs.len()) };
        self.tabs.insert(slot, tab);
        if let Some(active) = self.tab_index(id) {
            self.active = active;
        }
        true
    }

    /// Renames the tab's file (on disk and in the tab; unsaved changes are kept).
    pub fn rename_tab(&mut self, i: usize, name: &str) -> io::Result<()> {
        let Some(old) = self.tabs.get(i).and_then(|t| t.doc.path()).map(Path::to_path_buf) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "the tab has no file"));
        };
        self.rename_path(&old, name)
    }

    /// Renames entry `old` (a file or a folder) to `name` in its folder — a tab's file, the
    /// tree's entry: on disk (not over another one; a change of case only is allowed),
    /// announced (whoever keeps it by its path follows: a console's module, the index, a
    /// remote project's server), then the tabs of it or under it follow — their views, their
    /// bars, their unsaved changes; a new extension may mean another language.
    pub fn rename_path(&mut self, old: &Path, name: &str) -> io::Result<()> {
        let name = name.trim();
        if workspace::entries::rename_problem(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                i18n::t("refactor.invalid_name"),
            ));
        }
        let new = old.with_file_name(name);
        if new == old {
            return Ok(());
        }
        self.move_file(old, &new)?;
        let support = Arc::clone(&self.support);
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if tab.doc.path() != Some(new.as_path()) {
                continue;
            }
            let head =
                tab.doc.buffer().snapshot().slice(0..SNIFF_BYTES.min(tab.doc.buffer().len()));
            let language = support.languages.detect(&new, &head);
            // The syntax redone for another language.
            if *tab.syntax.language() != language {
                tab.syntax = DocumentSyntax::new(language);
                tab.diagnostics.errors_version = None;
                tab.symbols_version = None;
            }
        }
        Ok(())
    }

    /// The tab's reference: the PHP class (e.g. `App\Models\User`), otherwise `path:line`.
    pub fn tab_reference(&self, i: usize) -> Option<String> {
        let tab = self.tabs.get(i)?;
        let path = Self::index_path(tab);
        if let Ok(index) = self.data.symbols.read()
            && let Some(file) = index.file(&path)
            && let Some(class) = file.symbols.iter().find(|s| s.kind.is_type())
        {
            return Some(class.fqn.to_string());
        }
        let relative = self.tab_relative_path(i)?;
        let line = tab.doc.buffer().line_of(tab.doc.selections().primary().head) + 1;
        Some(format!("{}:{line}", relative.display()))
    }

    /// Opens the "Copy" popover for `path` (a tab's file adds its line and reference), at
    /// `anchor` (where a context menu was) or centered.
    pub fn open_copy_menu(
        &mut self,
        path: &Path,
        tab: Option<usize>,
        anchor: Option<Point>,
    ) -> bool {
        let options = self.copy_options(path, tab);
        self.code_hover.hide();
        self.completion.close();
        let subject = kernel::paths::file_name(path);
        let values = options.iter().map(|o| o.value.clone()).collect();
        let (title, action) = (i18n::t("copy.title"), i18n::t("copy.action"));
        self.choice_menu.open((&title, &subject, "copy", &action), options, anchor);
        self.choice_purpose = ChoicePurpose::Copy(values);
        self.choice_menu.is_open()
    }

    fn copy_options(&self, path: &Path, tab: Option<usize>) -> Vec<ui_workbench::Choice> {
        use crate::commands::{COPY_PATH, COPY_REFERENCE, shortcut_label};
        use i18n::t;
        let option = |key: &str, value: String, shortcut: Option<String>, group: u8| {
            let icon = match key {
                "copy.absolute" => "file",
                "copy.name" => "file-text",
                "copy.project" => "folder",
                "copy.repository" => "git-branch",
                "copy.line" => "hash",
                "copy.reference" => "braces",
                _ => "globe",
            };
            ui_workbench::Choice { icon, label: t(key), value, shortcut, group }
        };
        let slashes = |p: &Path| p.to_string_lossy().replace('\\', "/");
        let mut options = vec![option(
            "copy.absolute",
            path.to_string_lossy().into_owned(),
            shortcut_label(COPY_PATH),
            0,
        )];
        if let Some(name) = path.file_name() {
            options.push(option("copy.name", name.to_string_lossy().into_owned(), None, 0));
        }
        let root = self.project.as_ref().map(FileTree::root_path);
        let relative =
            root.and_then(|r| path.strip_prefix(r).ok()).filter(|r| !r.as_os_str().is_empty());
        if let Some(relative) = relative {
            options.push(option("copy.project", slashes(relative), None, 1));
        }
        // The repository root, when it isn't the project's (a project inside a monorepo).
        let vcs_dirs: Vec<&str> =
            self.support.vcs.iter().filter_map(|v| v.metadata_dir()).collect();
        let disk = &*self.ports.disk;
        let repository = path
            .ancestors()
            .skip(1)
            .find(|dir| vcs_dirs.iter().any(|d| disk.kind(&dir.join(d)).is_some()));
        if let Some(repo) = repository.filter(|r| Some(*r) != root)
            && let Ok(in_repo) = path.strip_prefix(repo)
        {
            options.push(option("copy.repository", slashes(in_repo), None, 1));
        }
        if let Some(i) = tab.filter(|&i| i < self.tabs.len()) {
            let doc = &self.tabs[i].doc;
            let line = doc.buffer().line_of(doc.selections().primary().head) + 1;
            let shown = relative.map_or_else(|| path.to_string_lossy().into_owned(), slashes);
            options.push(option("copy.line", format!("{shown}:{line}"), None, 2));
            if let Some(reference) =
                self.tab_reference(i).filter(|r| !r.ends_with(&format!(":{line}")))
            {
                options.push(option(
                    "copy.reference",
                    reference,
                    shortcut_label(COPY_REFERENCE),
                    2,
                ));
            }
        }
        options.push(option("copy.url", file_url(path), None, 3));
        options
    }

    pub fn activate(&mut self, i: usize) {
        self.param_hint.hide();
        // Leaving a tab saves what changed (automatic saving).
        if i != self.active {
            self.autosave(true);
        }
        if i < self.tabs.len()
            && i != self.active
            && let Some(current) = self.tabs.get(self.active)
        {
            push_history(&mut self.back, current.doc.id());
            self.forward.clear();
        }
        self.switch_to(i);
    }

    /// Navigation in the tab history (← / →). `true` if the tab changed.
    pub fn navigate(&mut self, forward: bool) -> bool {
        let (from, to) = if forward {
            (&mut self.forward, &mut self.back)
        } else {
            (&mut self.back, &mut self.forward)
        };
        while let Some(id) = from.pop() {
            if let Some(i) = self.tabs.iter().position(|t| t.doc.id() == id) {
                if let Some(current) = self.tabs.get(self.active) {
                    push_history(to, current.doc.id());
                }
                self.switch_to(i);
                return true;
            }
        }
        false
    }

    /// The arrows in the tab bar: the tab to the left / right of the active one.
    /// (The history of visited tabs stays on ⌘[ / ⌘].)
    pub(super) fn navigation(&self) -> Navigation {
        Navigation {
            can_back: !self.tabs.is_empty() && self.active > 0,
            can_forward: self.active + 1 < self.tabs.len(),
            maximized: self.editor_full(),
        }
    }

    pub(super) fn switch_to(&mut self, i: usize) {
        self.completion.close();
        if i < self.tabs.len() {
            self.tab_bar.reveal_active();
            self.active = i;
            self.focus = if self.tabs[i].find_open && self.focus == Focus::Find {
                Focus::Find
            } else {
                Focus::Editor
            };
            self.layout();
        }
    }

    pub fn cycle_tab(&mut self, forward: bool) {
        let n = self.tabs.len();
        if n > 1 {
            self.activate(if forward { (self.active + 1) % n } else { (self.active + n - 1) % n });
        }
    }

    /// The starting folder for dialogs: the project, otherwise the current file's folder.
    pub fn dialog_dir(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|p| p.root_path().to_path_buf()).or_else(|| {
            self.tabs.get(self.active).and_then(|t| t.doc.path()?.parent().map(Path::to_path_buf))
        })
    }

    /// The project's recent files forgotten (File › Open Recent › Clear Recent).
    pub fn clear_recent_files(&mut self) {
        if self.data.recent_files.is_empty() {
            return;
        }
        self.data.recent_files.clear();
        if let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) {
            self.recent_writes.save(&root, &[]);
        }
    }

    /// The project's recent files, the latest last.
    pub fn recent_files(&self) -> &[PathBuf] {
        &self.data.recent_files
    }

    /// Remembers the opened file (the project's empty screen shows it under "recent").
    pub(super) fn remember_file(&mut self, path: &Path) {
        if is_virtual(path) {
            return;
        }
        self.data.recent_files.retain(|p| p != path);
        self.data.recent_files.push(path.to_path_buf());
        if self.data.recent_files.len() > 30 {
            self.data.recent_files.remove(0);
        }
        // Written in the background: opening a file never waits for the disk.
        if let Some(root) = self.project.as_ref().map(|p| p.root_path())
            && path.starts_with(root)
        {
            self.recent_writes.save(root, &self.data.recent_files);
        }
    }
}

/// At most this many steps back / forward in the tab history.
const HISTORY: usize = 100;

fn push_history(history: &mut Vec<DocumentId>, id: DocumentId) {
    history.push(id);
    if history.len() > HISTORY {
        history.remove(0);
    }
}

/// `file://` URL of an absolute path (percent-encoded; `C:\\x` → `file:///C:/x`).
fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from(if path.starts_with('/') { "file://" } else { "file:///" });
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                url.push(b as char)
            }
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/files.rs"]
mod tests;

/// A file's language id, as the languages' modules say (`None`: plain text).
pub(super) fn language_id(support: &LanguageSupport, path: &Path) -> Option<String> {
    let id = support.languages.detect(path, "");
    (id != kernel::LanguageId::PLAIN_TEXT).then(|| id.to_string())
}
