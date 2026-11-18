//! Locked tabs (read-only): a text's tab — a file's, a query console's, a
//! preview's source — or a view that says it edits ([`ui_workbench::Viewer::lockable`]) can
//! be locked from the status bar's padlock, its tab's menu or the command; its document then
//! refuses every edit (typing, paste, undo, formatting, refactorings, a module's), while
//! selecting, moving, copying, finding and folding still work, and the disk's changes still
//! come in. A file that can't be written (read-only on disk) opens locked and stays so until
//! it can. The lock is the document's (`Document::set_read_only`): every tab of the same file
//! shares it; the session keeps it.

use super::*;

/// How long the word about a refused edit stays before it's said again.
const HINT_EVERY: std::time::Duration = workbench_model::notifications::SHORT;

impl Tab {
    /// It can be locked: its document is a text shown in the editor, or its view edits.
    pub(super) fn lockable(&self) -> bool {
        match &self.viewer {
            Some(v) if v.kind == ui_workbench::ViewerKind::Replace => v.viewer.lockable(),
            _ => true,
        }
    }

    pub(super) fn locked(&self) -> bool {
        self.doc.is_read_only()
    }

    /// Locked or unlocked: its document, and its view told.
    pub(super) fn set_locked(&mut self, locked: bool) {
        self.doc.set_read_only(locked);
        if let Some(v) = &mut self.viewer {
            v.viewer.set_locked(locked);
        }
    }

    /// Its file on disk, saved once and not virtual (where a read-only attribute can be).
    fn disk_path(&self) -> Option<&Path> {
        self.doc.path().filter(|p| !is_virtual(p) && self.untitled.is_none())
    }
}

/// What the status bar's padlock says of the active tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Writable,
    Locked,
    /// Locked because its file is read-only on disk (it can't be unlocked here).
    ReadOnlyOnDisk,
}

impl Workbench {
    /// Tab `i`'s lock (`None`: it can't be locked — an image, a comparison).
    pub fn tab_lock(&self, i: usize) -> Option<LockState> {
        let tab = self.tabs.get(i).filter(|t| t.lockable())?;
        Some(match (tab.locked(), tab.disk_read_only) {
            (_, true) => LockState::ReadOnlyOnDisk,
            (true, false) => LockState::Locked,
            (false, false) => LockState::Writable,
        })
    }

    /// The active tab's lock (`None`: no tab, or one that can't be locked).
    pub fn active_lock(&self) -> Option<LockState> {
        self.tab_lock(self.active)
    }

    /// Locks tab `i` or unlocks it — every tab of its file with it. A file read-only on
    /// disk isn't unlocked: the user is told why (unless it became writable meanwhile).
    /// `true` if it changed.
    pub fn toggle_lock(&mut self, i: usize) -> bool {
        let Some(state) = self.tab_lock(i) else { return false };
        let lock = state == LockState::Writable;
        if state == LockState::ReadOnlyOnDisk {
            let tab = &self.tabs[i];
            let still = tab
                .disk_path()
                .is_some_and(|p| document::DocumentStorage::read_only(&*self.ports.disk, p));
            if still {
                let name = tab.title();
                let notification = workbench_model::Notification::warning(i18n::tr!(
                    "lock.on_disk_title",
                    name = name
                ))
                .with_detail(i18n::t("lock.on_disk_detail"));
                workbench_model::WorkbenchHost::notify(self, notification);
                return false;
            }
        }
        let path = self.tabs[i].disk_path().map(Path::to_path_buf);
        match path {
            Some(path) => {
                for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
                    if tab.doc.path() == Some(path.as_path()) {
                        tab.disk_read_only = false;
                        tab.set_locked(lock);
                    }
                }
            }
            None => self.tabs[i].set_locked(lock),
        }
        tracing::info!(locked = lock, "tab lock toggled");
        true
    }

    /// The active tab locked or unlocked (the status bar's padlock, the command).
    pub fn toggle_active_lock(&mut self) -> bool {
        !self.tabs.is_empty() && self.toggle_lock(self.active)
    }

    /// A tab about to open: locked when its file is read-only on disk, or when the same
    /// file is already open locked (a split, another group).
    pub(super) fn lock_on_open(&self, tab: &mut Tab) {
        if !tab.lockable() {
            return;
        }
        let Some(path) = tab.disk_path() else { return };
        let on_disk = document::DocumentStorage::read_only(&*self.ports.disk, path);
        let shared = all_tabs(&self.tabs, &self.editors.groups)
            .any(|t| t.doc.path() == Some(path) && t.locked());
        if on_disk || shared {
            tab.disk_read_only = on_disk;
            tab.set_locked(true);
        }
    }

    /// The active tab is locked: the edit about to be made is refused (noted, the user is
    /// told). `true` if it was.
    pub(super) fn refuse_if_locked(&mut self) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        if !tab.locked() || !tab.editor_visible() {
            return false;
        }
        tab.doc.refuse();
        true
    }

    /// The files among `paths` open locked, by name: what a change to all of them can't
    /// touch.
    pub(super) fn locked_among(&self, paths: &[PathBuf]) -> Vec<String> {
        let mut names: Vec<String> = all_tabs(&self.tabs, &self.editors.groups)
            .filter(|t| t.locked() && t.doc.path().is_some_and(|p| paths.iter().any(|q| q == p)))
            .map(Tab::title)
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// A change to `paths` (a refactoring, its undo) refused when one of them is open
    /// locked: nothing is changed, the user told which. `Err`: the message.
    pub(super) fn refuse_locked_files(&mut self, paths: &[PathBuf]) -> Result<(), String> {
        let names = self.locked_among(paths);
        if names.is_empty() {
            return Ok(());
        }
        Err(i18n::tr!("lock.refactor_refused", names = names.join(", "), count = names.len()))
    }

    /// An edit was refused in a locked tab since the last call: the user is told (once a
    /// while, not on every key). `true` if something is shown.
    pub fn report_refused_edits(&mut self) -> bool {
        let mut refused = None;
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if tab.doc.take_refused() && refused.is_none() {
                refused = Some(tab.disk_read_only);
            }
        }
        let Some(on_disk) = refused else { return false };
        let now = std::time::Instant::now();
        if self.lock_hint.is_some_and(|at| now.duration_since(at) < HINT_EVERY) {
            return false;
        }
        self.lock_hint = Some(now);
        let key = if on_disk { "lock.refused_on_disk" } else { "lock.refused" };
        let notification = workbench_model::Notification::info(i18n::t(key))
            .with_key("read-only-refused")
            .with_timeout(workbench_model::notifications::Timeout::After(HINT_EVERY));
        workbench_model::WorkbenchHost::notify(self, notification);
        true
    }

    /// The padlock's tooltip: what a click does, or why it can't.
    pub(super) fn lock_tooltip(&self) -> Option<String> {
        Some(match self.active_lock()? {
            LockState::Writable => i18n::t("lock.make_read_only"),
            LockState::Locked => i18n::t("lock.make_writable"),
            LockState::ReadOnlyOnDisk => i18n::t("lock.read_only_on_disk"),
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/locking.rs"]
mod tests;
