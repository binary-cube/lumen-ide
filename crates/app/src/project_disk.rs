//! The project as the code intelligence reads it from the disk, in one place: a file's
//! text (an open one's from the editor, through `open`), a folder's entries, when a file
//! last changed. Used by the editor's own questions and by the refactorings' background
//! work alike.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use kernel::LanguageId;

/// Runs `f` with the project rooted at `root` (its language `levels`, every language's
/// intelligence `languages`); `open` gives an open file's text (unsaved changes included),
/// others are read from the disk.
pub fn with_project<R>(
    root: Option<&Path>,
    levels: &[(LanguageId, String)],
    languages: &[std::sync::Arc<dyn code_intelligence::LanguageIntelligence>],
    open: &dyn Fn(&Path) -> Option<String>,
    f: impl FnOnce(&code_intelligence::Project<'_>) -> R,
) -> R {
    with_project_kept(root, levels, languages, open, None, f)
}

/// [`with_project`], what's under `root` read through `kept` (the editor's own questions,
/// asked as the user types: never the disk again for the same file until it changes).
pub fn with_project_kept<R>(
    root: Option<&Path>,
    levels: &[(LanguageId, String)],
    languages: &[std::sync::Arc<dyn code_intelligence::LanguageIntelligence>],
    open: &dyn Fn(&Path) -> Option<String>,
    kept: Option<&DiskReads>,
    f: impl FnOnce(&code_intelligence::Project<'_>) -> R,
) -> R {
    // Kept only under the project (what its events tell the changes of).
    let kept = |path: &Path| kept.filter(|_| root.is_some_and(|r| path.starts_with(r)));
    let read = |path: &Path| -> Option<String> {
        if let Some(text) = open(path) {
            return Some(text);
        }
        match kept(path) {
            Some(reads) => reads.file(path, read_source),
            None => read_source(path),
        }
    };
    let list = |dir: &Path| -> Vec<(String, bool)> {
        match kept(dir) {
            Some(reads) => reads.dir(dir, list_dir),
            None => list_dir(dir),
        }
    };
    let modified = |path: &Path| match kept(path) {
        Some(reads) => reads.modified(path, modified_at),
        None => modified_at(path),
    };
    f(&code_intelligence::Project {
        root,
        read: &read,
        list: &list,
        modified: &modified,
        levels,
        languages,
    })
}

/// A source file's text: not a device, nothing larger than a source is (a dump, a bundle).
fn read_source(path: &Path) -> Option<String> {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= workspace::MAX_SOURCE_FILE)?;
    std::fs::read_to_string(path).ok()
}

fn list_dir(dir: &Path) -> Vec<(String, bool)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.file_type().ok()?.is_dir())))
        .collect()
}

fn modified_at(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// What the editor's questions read of a project (an included file's text, a folder's
/// entries, when a file changed), kept until the project's events tell of any change — a
/// save, a write, an entry made, removed or renamed, a rescan — then read again when next
/// asked. Typing in a file that includes others reads them once, not at every key. Read
/// outside its lock; at most [`DiskReads::MOST`] bytes of text kept (past it, forgotten).
#[derive(Debug, Default)]
pub struct DiskReads {
    /// How many changes the project's events told.
    changes: AtomicU64,
    kept: Mutex<Kept>,
}

#[derive(Debug, Default)]
struct Kept {
    /// The changes counted when these were read.
    at: u64,
    files: HashMap<PathBuf, Option<String>>,
    bytes: usize,
    dirs: HashMap<PathBuf, Vec<(String, bool)>>,
    modified: HashMap<PathBuf, Option<std::time::SystemTime>>,
}

impl DiskReads {
    /// The most text kept (bytes).
    pub const MOST: usize = 16 << 20;

    /// A change told (anything: the next question reads again).
    pub fn changed(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }

    /// Changes told on `events` (the project's), until the subscriptions are dropped.
    pub fn follow(self: &Arc<Self>, events: &kernel::EventBus) -> Vec<kernel::Subscription> {
        use workspace::events::{
            EntriesChanged, EntriesDeleted, EntriesRescan, FileRenamed, FilesWritten,
        };
        let on = || {
            let reads = Arc::clone(self);
            move || reads.changed()
        };
        let (a, b, c, d, e, g) = (on(), on(), on(), on(), on(), on());
        vec![
            events.subscribe(move |_: &document::events::FileSaved| a()),
            events.subscribe(move |_: &FilesWritten| b()),
            events.subscribe(move |_: &EntriesChanged| c()),
            events.subscribe(move |_: &EntriesDeleted| d()),
            events.subscribe(move |_: &FileRenamed| e()),
            events.subscribe(move |_: &EntriesRescan| g()),
        ]
    }

    /// What's kept, forgotten if a change came since.
    fn current(&self) -> std::sync::MutexGuard<'_, Kept> {
        let now = self.changes.load(Ordering::Relaxed);
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.at != now {
            *kept = Kept { at: now, ..Kept::default() };
        }
        kept
    }

    fn file(&self, path: &Path, read: impl FnOnce(&Path) -> Option<String>) -> Option<String> {
        if let Some(text) = self.current().files.get(path) {
            return text.clone();
        }
        let at = self.changes.load(Ordering::Relaxed);
        let text = read(path);
        let mut kept = self.current();
        // A change during the read: not kept (it may be older than the file now).
        let size = text.as_ref().map_or(0, String::len);
        if kept.at == at && kept.bytes.saturating_add(size) <= Self::MOST {
            kept.bytes += size;
            kept.files.insert(path.to_path_buf(), text.clone());
        }
        text
    }

    fn dir(
        &self,
        dir: &Path,
        list: impl FnOnce(&Path) -> Vec<(String, bool)>,
    ) -> Vec<(String, bool)> {
        if let Some(entries) = self.current().dirs.get(dir) {
            return entries.clone();
        }
        let at = self.changes.load(Ordering::Relaxed);
        let entries = list(dir);
        let mut kept = self.current();
        if kept.at == at && kept.dirs.len() < 1024 {
            kept.dirs.insert(dir.to_path_buf(), entries.clone());
        }
        entries
    }

    fn modified(
        &self,
        path: &Path,
        look: impl FnOnce(&Path) -> Option<std::time::SystemTime>,
    ) -> Option<std::time::SystemTime> {
        if let Some(at) = self.current().modified.get(path) {
            return *at;
        }
        let at = self.changes.load(Ordering::Relaxed);
        let modified = look(path);
        let mut kept = self.current();
        if kept.at == at && kept.modified.len() < 4096 {
            kept.modified.insert(path.to_path_buf(), modified);
        }
        modified
    }

    /// Everything forgotten ("Clear and Rebuild Caches").
    pub fn purge(&self) {
        *self.kept.lock().unwrap_or_else(PoisonError::into_inner) = Kept::default();
        self.changed();
    }
}

#[cfg(test)]
#[path = "../tests/unit/project_disk.rs"]
mod tests;
