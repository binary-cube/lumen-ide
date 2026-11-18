//! The folders shown, checked when the user comes back to the window: the system's watcher
//! may have lost what changed meanwhile (macOS loses a folder's events without a word while
//! the disk is very busy: an install, a build). When the window is left, only the time is
//! kept (nothing read); when it's back, the folders read so far are looked at on the
//! listing's thread: those changed since (a folder's time moves when an entry comes, goes or
//! is renamed in it) are told, as the watcher would have. Nothing is read again when nothing
//! changed, and nothing at all is done for a window taken without having been left.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

/// Taken off the time the window was left: a file system that keeps times coarsely (FAT:
/// 2 s) still shows a change made right after. At worst a folder changed just before is read
/// again for nothing.
const COARSE: Duration = Duration::from_secs(2);

/// When the window was left, and the folders found changed since it's back.
#[derive(Debug, Default, Clone)]
pub(crate) struct AwayCheck {
    /// Since when the window is left (the first time, if left again before coming back).
    left: Arc<Mutex<Option<SystemTime>>>,
    /// The window was taken again (done at the next poll, where the tree is).
    back: Arc<AtomicBool>,
    /// The folders found changed, to be told.
    changed: Arc<Mutex<Vec<PathBuf>>>,
}

impl AwayCheck {
    /// The window left (`true`) or taken again: left, its time kept (nothing read); back, done
    /// at the next [`Self::run`].
    pub(crate) fn heard(&self, left: bool) {
        if left {
            let mut since = self.left.lock().unwrap_or_else(PoisonError::into_inner);
            since.get_or_insert_with(SystemTime::now);
        } else {
            self.back.store(true, Ordering::Release);
        }
    }

    /// Back after having been left: the folders read so far (`loaded`, asked only then)
    /// looked at on `spawn` (the listing's thread; `wake` once some changed). The folders
    /// found changed by an earlier look are returned (to be told as the watcher would).
    pub(crate) fn run(
        &self,
        loaded: impl FnOnce() -> Vec<PathBuf>,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>),
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Vec<PathBuf> {
        let found =
            std::mem::take(&mut *self.changed.lock().unwrap_or_else(PoisonError::into_inner));
        if !self.back.swap(false, Ordering::AcqRel) {
            return found;
        }
        let Some(since) = self.left.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            return found;
        };
        let since = since.checked_sub(COARSE).unwrap_or(since);
        let dirs = loaded();
        let changed = Arc::clone(&self.changed);
        spawn(Box::new(move || {
            let moved: Vec<PathBuf> =
                dirs.into_iter().filter(|d| changed_since(d, since)).collect();
            if moved.is_empty() {
                return;
            }
            changed.lock().unwrap_or_else(PoisonError::into_inner).extend(moved);
            if let Some(wake) = wake {
                wake();
            }
        }));
        found
    }
}

/// `dir`'s entries changed since `since` (gone, or not readable: told too, its folder reads
/// it again).
fn changed_since(dir: &Path, since: SystemTime) -> bool {
    std::fs::metadata(dir).and_then(|m| m.modified()).map_or(true, |at| at >= since)
}

#[cfg(test)]
#[path = "../tests/unit/away.rs"]
mod tests;
