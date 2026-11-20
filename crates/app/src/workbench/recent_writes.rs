//! The project's recent files, written off the UI thread: opening a file (ten of them when a
//! session comes back) never waits for the disk. Writes queued meanwhile for the same
//! project keep only the latest list; reading a project's list gives what was last saved,
//! written or not; a flush (the window gone) writes what's left before it returns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// The shared pool's name its writes run under (the Task Manager puts it on the interface's
/// row).
pub(crate) const RECENT_FILES_THREAD: &str = "recent-files";

/// The store, with what waits to be written.
struct Queue {
    store: Arc<dyn workspace::RecentFiles>,
    /// The latest list of each project not written yet.
    pending: Mutex<HashMap<PathBuf, Vec<PathBuf>>>,
    /// Held while writing: writes never overlap (a later list never lands first).
    writing: Mutex<()>,
}

impl Queue {
    /// Writes whatever is waiting (the latest list of each project).
    fn write_pending(&self) {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let batch =
            std::mem::take(&mut *self.pending.lock().unwrap_or_else(PoisonError::into_inner));
        for (root, files) in batch {
            if let Err(e) = self.store.save(&root, &files) {
                tracing::debug!(error = %e, "can't keep the recent files");
            }
        }
    }
}

/// See the module's doc.
pub(super) struct QueuedRecentFiles {
    queue: Arc<Queue>,
}

impl std::fmt::Debug for QueuedRecentFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueuedRecentFiles")
            .field("store", &self.queue.store)
            .finish_non_exhaustive()
    }
}

impl QueuedRecentFiles {
    pub(super) fn new(store: Arc<dyn workspace::RecentFiles>) -> Self {
        let queue = Queue { store, pending: Mutex::default(), writing: Mutex::new(()) };
        Self { queue: Arc::new(queue) }
    }

    /// The project's list, as last saved (written or still waiting).
    pub(super) fn load(&self, root: &Path) -> Vec<PathBuf> {
        let waiting =
            self.queue.pending.lock().unwrap_or_else(PoisonError::into_inner).get(root).cloned();
        if let Some(files) = waiting {
            return files;
        }
        // A write under way finishes first (the file is whole).
        let _writing = self.queue.writing.lock().unwrap_or_else(PoisonError::into_inner);
        self.queue.store.load(root)
    }

    /// Keeps `files` for the project at `root`, in the background.
    pub(super) fn save(&self, root: &Path, files: &[PathBuf]) {
        self.queue
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root.to_path_buf(), files.to_vec());
        let queue = Arc::clone(&self.queue);
        kernel::work::run(RECENT_FILES_THREAD, move || queue.write_pending());
    }

    /// Everything saved so far is on disk when this returns.
    pub(super) fn flush(&self) {
        self.queue.write_pending();
    }
}

impl Drop for QueuedRecentFiles {
    /// The window gone: what it saved is on disk before it is (another opens it next).
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/recent_writes.rs"]
mod tests;
