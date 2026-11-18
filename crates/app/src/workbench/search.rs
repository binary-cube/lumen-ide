//! File search (the file index) and project search.

use super::*;

impl Workbench {
    /// Starts indexing the project's paths in the background, if they're missing or out of
    /// date (the indexing module's index).
    pub(super) fn ensure_index(&mut self) {
        if self.data.indexer.as_ref().is_some_and(|i| i.ensure_paths()) {
            let index = workspace::events::IndexKind::Paths;
            self.data.events.publish(&workspace::events::IndexingStarted { index });
        }
    }

    /// What the index searches with (the file index, the semantic model) let go after a
    /// while unused.
    pub(super) fn release_idle_index(&mut self) {
        if let Some(indexer) = &self.data.indexer {
            indexer.release_idle();
        }
    }

    /// Takes the finished index. `true` if a redraw is needed.
    pub fn poll_index(&mut self) -> bool {
        if self.data.indexer.as_ref().is_some_and(|i| i.take_search_ready()) {
            self.modal_view_command("refresh");
            return true;
        }
        if !self.data.indexer.as_ref().is_some_and(|i| i.take_paths_built()) {
            return false;
        }
        // A new list (a file made, removed): what reads it (a panel's list) reads it again.
        self.data.generations.list_rebuilt();
        let index = workspace::events::IndexKind::Paths;
        self.data.events.publish(&workspace::events::IndexingFinished { index });
        // An open search finds the folders now.
        self.modal_view_command("refresh");
        true
    }

    /// The project's one path search (folders and files), used by every place that
    /// searches paths; each asks for as many folders / files as it shows. Starts indexing
    /// if needed (until then: nothing found).
    pub(super) fn search_paths(&mut self, text: &str, folders: usize, files: usize) -> PathResults {
        self.ensure_index();
        // `User.php:12`: the name is searched, the place is used when it's opened.
        let text = workspace::split_location(text.trim()).0;
        if text.is_empty() {
            return PathResults::default();
        }
        let Some(index) = &self.data.indexer else { return PathResults::default() };
        index.find_paths(&PathQuery { text, folders, files })
    }

    /// The project's queries (paths, declarations, text, files) as a snapshot for any
    /// thread; starts indexing the files if needed (meanwhile the snapshot walks the project
    /// once itself). `None` without a project.
    pub fn queries(&mut self) -> Option<Arc<dyn workbench_model::ProjectQueries>> {
        self.project.as_ref()?;
        self.ensure_index();
        let files = self.data.indexer.as_ref().and_then(|i| i.files());
        self.snapshot(files)
    }

    /// The project's queries as they are, without starting the index nor counting as its
    /// use (what a panel is given at each change: the index is built when it searches).
    pub(super) fn quiet_queries(&self) -> Option<Arc<dyn workbench_model::ProjectQueries>> {
        self.snapshot(self.data.indexer.as_ref().and_then(|i| i.peek_files()))
    }

    fn snapshot(
        &self,
        files: Option<Arc<Vec<String>>>,
    ) -> Option<Arc<dyn workbench_model::ProjectQueries>> {
        let root = self.project.as_ref()?.root_path().to_path_buf();
        Some(Arc::new(crate::queries::QueriesSnapshot {
            root,
            marks: self.data.marks.clone(),
            disk: Arc::clone(&self.ports.disk) as _,
            files,
            folders: self.data.indexer.as_ref().and_then(|i| i.peek_folders()),
            ignored: self.data.indexer.as_ref().and_then(|i| i.peek_ignored()),
            symbols: Arc::clone(&self.data.symbols),
            text: self.data.indexer.as_ref().map(|i| i.text()),
            walked: std::sync::OnceLock::new(),
            language_of: {
                let support = Arc::clone(&self.support);
                Arc::new(move |path: &Path| support.languages.detect(path, "").to_string())
            },
            generation: self.data.generations.files(),
            list_dir: {
                let services = self.support.project_services.clone();
                let disk = Arc::clone(&self.ports.disk);
                Arc::new(move |dir: &Path| {
                    use workspace::application::FileSystem as _;
                    listing::ListedFs::new(&*disk, &services).read_dir(dir)
                })
            },
            file_icon: {
                let support = Arc::clone(&self.support);
                Arc::new(move |name: &str| support.languages.icon_for(name))
            },
            folder_color: {
                let support = Arc::clone(&self.support);
                Arc::new(move |name: &str| support.languages.folder_color(name))
            },
            intelligence: self.support.intelligence.clone(),
        }))
    }

    /// Moves focus to the file search field. `false` without a project.
    pub fn focus_file_search(&mut self) -> bool {
        self.ensure_index();
        self.tell_project_panel(workbench_model::EntriesRequest::FocusSearch, true)
    }

    /// Shows folder `path` in the project tree, expanded.
    pub fn show_folder(&mut self, path: &Path) {
        let request =
            workbench_model::EntriesRequest::Reveal { path: path.to_path_buf(), expand: true };
        self.tell_project_panel(request, false);
    }

    /// "Search in Project…" (its module's view over the window), unless a modal is open:
    /// nothing behind it gets commands.
    pub fn open_project_search(&mut self) -> bool {
        !self.modal_open() && self.run_command(crate::commands::SEARCH_PROJECT) == Some(true)
    }

    #[cfg(feature = "snapshot")]
    /// Moves to a settings section (diagnostics / snapshots).
    pub fn settings_step(&mut self, delta: isize) -> bool {
        self.settings.step_section(delta)
    }

    #[cfg(feature = "snapshot")]
    /// The wheel over the settings' content (snapshots).
    pub fn settings_wheel(&mut self, dy: f32) -> bool {
        self.settings.scroll_by(dy)
    }

    /// A settings page opened by its id (Help › Keyboard Shortcuts…, snapshots).
    pub fn settings_show_page(&mut self, id: &str) {
        self.settings.show_page(id);
    }

    #[cfg(feature = "snapshot")]
    /// Every settings page's "Advanced" opened (diagnostics / snapshots).
    pub fn settings_open_advanced(&mut self) {
        self.settings.open_advanced();
    }

    #[cfg(feature = "snapshot")]
    /// The settings' chosen search result opened (diagnostics / snapshots).
    pub fn settings_open_result(&mut self) {
        self.settings.open_selected(&self.settings_model);
        self.settings_result_opened();
    }
}
