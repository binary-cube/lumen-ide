//! Watching a project's files (the [`FileWatching`] port; the local adapter lives in
//! `fs-local`): entries created, deleted and renamed (for the tree), files written (their
//! tabs follow), and a repository's own folder changing (what's committed may differ).

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Starts watching a project (port).
pub trait FileWatching: std::fmt::Debug + Send + Sync {
    /// Watches everything under `root`; `dirs`: the folders heard apart or not at all.
    /// `wake` is called (from another thread) when something was heard. `None`: it can't be
    /// watched.
    fn watch(
        &self,
        root: &Path,
        dirs: WatchedDirs,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<Box<dyn FileWatcher>>;
}

/// The folders a watcher treats apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchedDirs {
    /// The repositories' own folders (`.git`, from the version control modules): heard
    /// apart ([`FileWatcher::take_vcs_changed`]), not as the project's entries.
    pub vcs: Vec<&'static str>,
    /// The folders of the project's own folder (`kernel::brand::PROJECT_DIR`) holding the
    /// user's files (the modules': the query consoles'), heard as the project's; the rest of
    /// it is the app's own, never heard.
    pub own: Vec<&'static str>,
}

impl WatchedDirs {
    /// `path` (relative to the root) is a file right inside the project's own folder.
    pub fn own_file(path: &Path) -> bool {
        let mut parts = path.components();
        parts.next().is_some_and(|c| c.as_os_str() == kernel::brand::PROJECT_DIR)
            && parts.next().is_some()
            && parts.next().is_none()
    }

    /// `path` is the app's own (its project folder, but for [`Self::own`]) or a
    /// repository's: not one of the project's entries.
    pub fn internal(&self, path: &Path) -> bool {
        let mut parts = path.components();
        while let Some(part) = parts.next() {
            let name = part.as_os_str();
            if self.vcs.iter().any(|d| name == *d) {
                return true;
            }
            if name == kernel::brand::PROJECT_DIR {
                let next = parts.next().map(|c| c.as_os_str());
                return !next.is_some_and(|n| self.own.iter().any(|d| n == *d));
            }
        }
        false
    }
}

/// A project being watched; dropped, the watching stops.
pub trait FileWatcher: std::fmt::Debug + Send + Sync {
    /// The paths created, deleted or renamed since the last call, without duplicates
    /// (sorted).
    fn drain_changed(&self) -> Vec<PathBuf>;

    /// The files written since the last call (their content may have changed: a download,
    /// another program), without duplicates.
    fn drain_written(&self) -> Vec<PathBuf>;

    /// The files of the project's own folder written since the last call (right inside it:
    /// its settings and lists, not its caches), for whoever shows or follows them. They
    /// aren't the project's entries.
    fn drain_own_written(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Whether everything must be looked at again since the last call: watching just
    /// started (what changed before may have been missed), or events were dropped.
    fn take_rescan(&self) -> bool;

    /// Whether a repository's own folder (`.git`) changed since the last call.
    fn take_vcs_changed(&self) -> bool;
}

#[cfg(test)]
#[path = "../tests/unit/watch.rs"]
mod tests;
