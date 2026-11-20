//! The project session (`<project dir>/session.toml`): saving and restoring.

use super::*;

impl Workbench {
    /// The tabs, groups, splits and terminal, as they are now (without virtual tabs or
    /// tabs not saved on disk: what's kept is the Workbench context's rule).
    pub(super) fn current_session(&self) -> Option<workbench_model::Session> {
        use workbench_model::{Current, OpenGroup, OpenTab, Session};
        let root = self.project.as_ref()?.root_path();
        let groups = (0..self.editors.groups.len())
            .map(|g| {
                let (tabs, active) = if g == self.editors.current {
                    (&self.tabs, self.active)
                } else {
                    (&self.editors.groups[g].tabs, self.editors.groups[g].active)
                };
                let tabs = tabs
                    .iter()
                    .map(|tab| {
                        let path =
                            tab.doc.path().filter(|p| !is_virtual(p) && tab.untitled.is_none())?;
                        let split = tab.viewer.as_ref().and_then(|v| v.split.saved());
                        // Locked by the user (a file read-only on disk is locked again anyway).
                        let locked = tab.locked() && !tab.disk_read_only;
                        Some(OpenTab { path, pinned: tab.pinned, split, locked })
                    })
                    .collect();
                OpenGroup { tabs, active }
            })
            .collect();
        Some(Session::capture(
            root,
            Current {
                groups,
                group: self.editors.current,
                split: self.editors.tree.to_spec(),
                tool_windows: self.saved_tool_windows(),
                bottom: self.tools.bottom.clone(),
                zoom: self.editor_zoom,
                theme: self.project_theme.clone(),
                panels: self.tools.panel_mementos(),
                sizes: self.data.sizes.entries(),
            },
        ))
    }

    /// Saves the session if it changed since the last save (called after painting): at most
    /// every half second (not on every frame); closing (`freeze_session`) saves it right
    /// away anyway.
    pub(super) fn save_session_soon(&mut self) {
        if self.session.due(std::time::Instant::now()) {
            self.save_session_if_changed();
        }
    }

    pub(super) fn save_session_if_changed(&mut self) {
        let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
            return;
        };
        let current = self.current_session();
        if let Err(e) = self.session.save_if_changed(&self.sessions, &root, current) {
            tracing::warn!(error = %e, "cannot save the session");
        }
    }

    /// Closing the window / project: the current session is saved (on disk when this
    /// returns) and no longer changes while the tabs are being closed.
    pub fn freeze_session(&mut self) {
        self.save_session_if_changed();
        self.sessions.flush();
        self.recent_writes.flush();
        self.session.freeze();
    }

    /// A closing was cancelled (the window / the project stays): the session is saved again
    /// as it changes.
    pub fn thaw_session(&mut self) {
        self.session.thaw();
    }

    /// Reopens the project's saved session (tabs, splits, terminal). `false` if it
    /// doesn't have one.
    pub fn restore_session(&mut self) -> bool {
        let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
            return false;
        };
        let Some(session) = workbench_model::SessionStore::load(&self.sessions, &root) else {
            return false;
        };
        // The sizes first: the views made below read theirs (the docks' too).
        self.data.sizes.replace(&session.sizes);
        self.set_editor_zoom(session.zoom);
        self.project_theme = session.theme.clone();
        let disk = Arc::clone(&self.ports.disk);
        let reopen = session.reopen(&root, &|p: &Path| disk.kind(p).is_some());
        let mut opened_groups = 0;
        for (g, group) in reopen.iter().enumerate() {
            if g > 0 {
                // A new (empty) group, made the current group so it receives the tabs.
                self.editors.groups.push(GroupState::default());
                let last = self.editors.groups.len() - 1;
                self.swap_group(self.editors.current);
                self.swap_group(last);
                self.editors.current = last;
            }
            let mut active = None;
            for tab in group {
                if let Err(e) = self.open_file(&tab.path) {
                    tracing::warn!(path = %tab.path.display(), error = %e, "can't reopen");
                    continue;
                }
                if tab.pinned
                    && let Some(t) = self.tabs.get_mut(self.active)
                {
                    t.pinned = true;
                }
                if tab.locked
                    && let Some(t) = self.tabs.get_mut(self.active).filter(|t| t.lockable())
                {
                    t.set_locked(true);
                }
                // Its preview's halves as they were.
                if let Some(ratio) = tab.split
                    && let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
                {
                    v.split = preview_split::PreviewSplit::new(ratio);
                }
                if tab.active {
                    active = Some(self.active);
                }
            }
            if let Some(a) = active {
                self.activate(a);
            }
            opened_groups += 1;
        }
        // Groups left empty (files deleted meanwhile) are removed; otherwise, the layout.
        let tree =
            crate::split_tree::SplitTree::from_spec(&session.split, self.editors.groups.len());
        let any_empty = (0..self.editors.groups.len()).any(|g| {
            if g == self.editors.current {
                self.tabs.is_empty()
            } else {
                self.editors.groups[g].tabs.is_empty()
            }
        });
        if any_empty || tree.is_none() {
            self.merge_groups();
        } else if let Some(tree) = tree {
            self.editors.tree = tree;
            self.focus_group(session.group.min(self.editors.groups.len() - 1));
        }
        // What the panels kept of themselves, given as each one is made.
        self.tools.mementos = session.panels.iter().cloned().collect();
        self.restore_tool_windows(&session.tool_windows, session.bottom.as_deref());
        self.focus = Focus::Editor;
        self.layout();
        // What was reopened is exactly the saved session (not rewritten right away).
        self.session.remember(self.current_session());
        tracing::info!(groups = opened_groups, "session restored");
        true
    }
}

/// The sessions' store, written on a thread of its own: a session saved while painting never
/// waits for the disk. Saves queued meanwhile for the same project keep only the latest; a
/// flush waits until all are written (closing).
pub(super) struct QueuedSessions {
    store: Arc<dyn workbench_model::SessionStore + Send + Sync>,
    queue: std::cell::OnceCell<std::sync::mpsc::Sender<SessionJob>>,
}

enum SessionJob {
    Save(PathBuf, workbench_model::Session),
    /// Answered once everything before it is written.
    Flush(std::sync::mpsc::Sender<()>),
}

/// Its thread's name (the Task Manager puts it on the interface's row).
pub(crate) const SESSION_THREAD: &str = "session-save";

impl std::fmt::Debug for QueuedSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueuedSessions").field("store", &self.store).finish_non_exhaustive()
    }
}

impl QueuedSessions {
    pub(super) fn new(store: Arc<dyn workbench_model::SessionStore + Send + Sync>) -> Self {
        Self { store, queue: std::cell::OnceCell::new() }
    }

    /// The writing thread, started on the first save; it ends with the store.
    fn queue(&self) -> Option<&std::sync::mpsc::Sender<SessionJob>> {
        if let Some(queue) = self.queue.get() {
            return Some(queue);
        }
        let (tx, rx) = std::sync::mpsc::channel::<SessionJob>();
        let store = Arc::clone(&self.store);
        let started = std::thread::Builder::new().name(SESSION_THREAD.into()).spawn(move || {
            while let Ok(job) = rx.recv() {
                // What came meanwhile: the latest of each project.
                let mut saves: Vec<(PathBuf, workbench_model::Session)> = Vec::new();
                let mut flushes = Vec::new();
                for job in std::iter::once(job).chain(rx.try_iter()) {
                    match job {
                        SessionJob::Save(root, session) => {
                            saves.retain(|(r, _)| *r != root);
                            saves.push((root, session));
                        }
                        SessionJob::Flush(done) => flushes.push(done),
                    }
                }
                for (root, session) in saves {
                    if let Err(e) = store.save(&root, &session) {
                        tracing::warn!(error = %e, "cannot save the session");
                    }
                }
                for done in flushes {
                    let _ = done.send(());
                }
            }
        });
        if let Err(e) = started {
            tracing::warn!(error = %e, "the session's writer can't start");
            return None;
        }
        let _ = self.queue.set(tx);
        self.queue.get()
    }

    /// Everything saved so far is on disk when this returns.
    pub(super) fn flush(&self) {
        let Some(queue) = self.queue.get() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        if queue.send(SessionJob::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        }
    }
}

impl Drop for QueuedSessions {
    /// The window gone: what it saved is on disk before it is (another opens it next).
    fn drop(&mut self) {
        self.flush();
    }
}

impl workbench_model::SessionStore for QueuedSessions {
    fn load(&self, root: &Path) -> Option<workbench_model::Session> {
        // A save still queued is read back as written.
        self.flush();
        self.store.load(root)
    }

    fn save(&self, root: &Path, session: &workbench_model::Session) -> std::io::Result<()> {
        match self.queue() {
            Some(queue) => {
                let _ = queue.send(SessionJob::Save(root.to_path_buf(), session.clone()));
                Ok(())
            }
            // No thread: written here.
            None => self.store.save(root, session),
        }
    }
}
