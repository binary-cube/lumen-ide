//! The documents' line decorations (the lines changed since the last commit, the blame
//! annotations) and the project's label (its branch), from the modules' [`LineDecorator`]s
//! (the git module): opened with the project, told before each frame which documents are
//! shown, polled when their background work wakes the app. The workbench only draws what
//! they have; without one, or outside a repository, nothing is shown.
//!
//! [`LineDecorator`]: workbench_model::LineDecorator

use super::*;
use workbench_model::{DocumentDecorations, ShownDocument};

impl Workbench {
    /// Looks for the project's repository in the background: reading its settings never
    /// holds the window. It comes with a later poll ([`Self::take_repository`]).
    pub(super) fn open_repository(&mut self, root: &Path) {
        let (vcs, root) = (self.support.vcs.clone(), root.to_path_buf());
        let waker = self.waker.as_ref().map(|w| Arc::clone(&w.0));
        let (tx, rx) = std::sync::mpsc::channel();
        self.data.repository_search = Some(rx);
        self.data.jobs.modules.spawn(
            kernel::Priority::Interactive,
            Some(&self.data.jobs.cancel),
            move || {
                let repo = vcs.iter().find_map(|provider| provider.open(&root));
                if tx.send(repo).is_ok()
                    && let Some(wake) = waker
                {
                    wake();
                }
            },
        );
    }

    /// The repository looked for, if it came: the decorators start. `true` if it did.
    fn take_repository(&mut self) -> bool {
        use std::sync::mpsc::TryRecvError;
        let Some(search) = &self.data.repository_search else { return false };
        match search.try_recv() {
            Ok(repo) => {
                self.data.repository_search = None;
                let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
                if let Some(root) = root {
                    self.use_repository(&root, repo);
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.data.repository_search = None;
                false
            }
        }
    }

    /// The project's repository looked for already (found or not).
    #[cfg(any(test, feature = "snapshot"))]
    pub fn repository_known(&self) -> bool {
        self.data.repository_search.is_none()
    }

    /// The project's repository (if it is in one), and the decorators for it.
    fn use_repository(&mut self, root: &Path, repo: Option<Arc<dyn version_control::Repository>>) {
        if let Some(repo) = &repo {
            tracing::info!(root = %repo.root().display(), "git repository");
        }
        self.data.git = repo;
        let wake: Arc<dyn Fn() + Send + Sync> = match &self.waker {
            Some(w) => Arc::clone(&w.0),
            None => Arc::new(|| {}),
        };
        let context = workbench_model::DecoratorContext {
            id: self.data.id,
            root,
            events: &self.data.events,
            wake,
            repository: self.data.git.clone(),
        };
        let support = Arc::clone(&self.support);
        self.data.decorations =
            support.decorators.iter().filter_map(|d| d.open(&context)).collect();
        self.chrome.project_label = self.data.decorations.iter().find_map(|d| d.label());
        if let Some(repo) = &self.data.git {
            let found = version_control::RepositoryOpened { root: repo.root().to_path_buf() };
            self.data.events.publish(&found);
        }
    }

    /// For the current group's documents shown: the decorators ask for what's missing (in
    /// the background); their annotations' width follows. A tab off screen for a while
    /// (`UNSEEN`) isn't asked: what they read for it goes, read again once it's shown.
    pub(super) fn sync_decorations(&mut self) {
        if self.data.decorations.is_empty() {
            return;
        }
        let active = self.active;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if tab.viewer.as_ref().is_some_and(|v| !v.editor_visible()) {
                continue;
            }
            if i != active && tab.seen.elapsed() > Self::UNSEEN {
                continue;
            }
            let Some(path) = tab.doc.path() else { continue };
            let shown = ShownDocument {
                id: tab.doc.id(),
                path,
                version: tab.doc.version(),
                text: tab.doc.buffer(),
            };
            let mut chars = 0;
            for decorations in &mut self.data.decorations {
                decorations.sync(&shown);
                chars = chars.max(decorations.annotation_chars(shown.id));
            }
            tab.view.set_annotation_chars(chars);
        }
    }

    /// Takes what the decorators found in the background; after a change inside the
    /// repository's own folder, they look again (a new `HEAD`, another branch). `true` if a
    /// redraw is needed.
    pub fn poll_decorations(&mut self) -> bool {
        let found = self.take_repository();
        let vcs = self.tree_watcher.as_ref().is_some_and(|w| w.take_vcs_changed());
        if self.data.decorations.is_empty() {
            return found;
        }
        let mut changed = (vcs && self.refresh_vcs()) || found;
        let open: Vec<DocumentId> =
            all_tabs(&self.tabs, &self.editors.groups).map(|t| t.doc.id()).collect();
        for decorations in &mut self.data.decorations {
            decorations.keep(&open);
            changed |= decorations.poll();
        }
        // What was read in the background may be the branch shown.
        if changed {
            self.refresh_label();
        }
        changed
    }

    /// The name shown beside the project's follows the decorators. `true` if it changed.
    fn refresh_label(&mut self) -> bool {
        let label = self.data.decorations.iter().find_map(|d| d.label());
        let changed = label != self.chrome.project_label;
        self.chrome.project_label = label;
        changed
    }

    /// The repository looked at again (its `HEAD`, its branch, an operation in progress):
    /// after a change heard in its folder, and as the window takes the focus — another
    /// program (SourceTree, the terminal) may have switched the branch or left a rebase,
    /// and its folder may be out of the watched tree, or its events dropped. Cheap: `HEAD`
    /// and a few state files read. `true` if something shown changed.
    pub fn refresh_vcs(&mut self) -> bool {
        let mut changed = false;
        for decorations in &mut self.data.decorations {
            changed |= decorations.vcs_changed();
        }
        changed |= self.refresh_label();
        changed
    }

    /// Document `doc`'s decorations (the first decorator's that has some).
    pub(super) fn decorations_of(&self, doc: DocumentId) -> Option<DocumentDecorations> {
        self.data.decorations.iter().find_map(|d| d.decorations(doc))
    }

    /// The first line of the active tab's mark `i` (a gutter bar).
    pub(super) fn change_line(&self, i: usize) -> Option<usize> {
        let tab = self.tabs.get(self.active)?;
        self.decorations_of(tab.doc.id())?.marks.get(i).map(|m| m.lines.start)
    }

    /// A click on the active tab's mark starting at `line` no module's gutter action took:
    /// what its decorator runs (git: the file compared with its last commit).
    pub(super) fn mark_clicked(&mut self, line: usize) -> bool {
        let Some(doc) = self.tabs.get(self.active).map(|t| t.doc.id()) else { return false };
        let command = self.data.decorations.iter().find_map(|d| d.mark_clicked(doc, line));
        command.is_some_and(|c| self.run_command(c) == Some(true))
    }

    #[cfg(feature = "snapshot")]
    /// The active tab's annotations are computed (diagnostics / snapshots).
    pub fn annotations_ready(&self) -> bool {
        self.tabs
            .get(self.active)
            .and_then(|t| self.decorations_of(t.doc.id()))
            .and_then(|d| d.annotations)
            .is_some_and(|a| !a.is_empty())
    }

    /// The tooltip of the annotation under `p` (the git module's: the commit's id, author,
    /// date and message).
    pub(super) fn annotation_tip(&self, p: Point) -> Option<ui_workbench::Tip> {
        let tab = self.tabs.get(self.active)?;
        let line = tab.view.annotation_at(p)?;
        let label = self
            .data
            .decorations
            .iter()
            .find_map(|d| d.annotation_tip(tab.doc.id(), line as usize))?;
        Some(ui_workbench::Tip::new(tab.view.annotation_rect(line)?, label))
    }

    #[cfg(feature = "snapshot")]
    /// Requests and takes the decorations of the shown documents; `true` once none is being
    /// computed (diagnostics / snapshots).
    pub fn decorations_settled(&mut self) -> bool {
        self.sync_decorations();
        self.poll_decorations();
        self.sync_decorations();
        self.repository_known()
            && self.tabs.iter().all(|t| {
                let (doc, version) = (t.doc.id(), t.doc.version());
                self.data.decorations.iter().all(|d| d.settled(doc, version))
            })
    }
}
