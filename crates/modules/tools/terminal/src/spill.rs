//! Where the terminals' histories wait while they aren't shown (the port
//! `terminal_session::SpillStore`, used by the PTY's parking): a file each, compressed
//! (deflate) then sealed with the run's key (`secrets::RunKey`, never written anywhere: the
//! files are unreadable once the app ends), under the project's data folder
//! (`<project dir>/terminal-spill/run-<id>/`, ignored by git) or, without a project, the
//! system's temporary folder.
//!
//! - **A file lives** from the history's leaving to its coming back (read once, then deleted),
//!   or until the history is erased or the terminal closes.
//! - **A store's folder** (`run-<id>`, one per project root and opening) is held locked
//!   (`run.lock`) while it's used, and deleted when it's let go (the project closed). The
//!   first time a store writes, the folders of others whose lock file is there, free and
//!   older than [`LEFT_AFTER`] (left by an app that stopped abruptly) are deleted: what they
//!   hold can't be read anyway. A folder that couldn't be made is tried again after
//!   [`RETRY_AFTER`].
//! - **Off the UI thread**: written and read on the parking thread or its worker.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use terminal_session::{SpillStore, SpillToken};

/// The folder under the project's data folder (and the temporary one).
const FOLDER: &str = "terminal-spill";
/// A run's folder name starts so (its id after it).
const RUN_PREFIX: &str = "run-";
/// The file a run holds locked.
const LOCK_FILE: &str = "run.lock";
/// A run folder whose lock is free is deleted only once its lock file is this old (another
/// app may have just made it and not locked it yet).
const LEFT_AFTER: std::time::Duration = std::time::Duration::from_secs(60);
/// A folder that couldn't be made is tried again after this long.
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(60);
/// Deflate's level: the fastest (a history is mostly text: it shrinks well already).
const LEVEL: u8 = 1;
/// The most a history may inflate to (bytes): a file grown past it is refused.
const MOST_INFLATED: usize = 512 * 1024 * 1024;

/// The stores made, by project root (`None`: the temporary folder's).
static STORES: Mutex<Vec<(Option<PathBuf>, Arc<DiskSpill>)>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The store for terminals of the project at `root` (`None`: none open), one per root.
pub(crate) fn store(root: Option<&Path>) -> Arc<dyn SpillStore> {
    let mut stores = lock(&STORES);
    if let Some((_, store)) = stores.iter().find(|(r, _)| r.as_deref() == root) {
        return Arc::clone(store) as Arc<dyn SpillStore>;
    }
    let base = match root {
        Some(root) => root.join(kernel::brand::PROJECT_DIR).join(FOLDER),
        None => std::env::temp_dir().join(FOLDER),
    };
    let store = Arc::new(DiskSpill::new(base, root.is_some()));
    stores.push((root.map(Path::to_path_buf), Arc::clone(&store)));
    store
}

/// The project at `root` closed: its store let go (its folder deleted once its terminals,
/// which hold it, are gone too).
pub(crate) fn release(root: &Path) {
    lock(&STORES).retain(|(r, _)| r.as_deref() != Some(root));
}

/// A run's folder of history files (see the module).
pub(crate) struct DiskSpill {
    /// The folder of every run (`…/terminal-spill`).
    base: PathBuf,
    /// Under a project's data folder (its `.gitignore` written there too).
    in_project: bool,
    /// Its own folder's name (`run-<id>`).
    run: String,
    /// Its folder made (and the others cleaned) at the first write: its lock held; or when
    /// making it last failed.
    ready: Mutex<Ready>,
}

/// Whether a store's folder is made.
#[derive(Debug, Default)]
enum Ready {
    #[default]
    NotYet,
    Made(std::fs::File),
    Failed(std::time::Instant),
}

impl std::fmt::Debug for DiskSpill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiskSpill").field("base", &self.base).finish_non_exhaustive()
    }
}

impl DiskSpill {
    pub(crate) fn new(base: PathBuf, in_project: bool) -> Self {
        let run = format!("{RUN_PREFIX}{:032x}", kernel::id::new_uuid());
        Self { base, in_project, run, ready: Mutex::default() }
    }

    /// Its folder.
    pub(crate) fn dir(&self) -> PathBuf {
        self.base.join(&self.run)
    }

    /// Its folder made, git told to ignore it, its lock held and others' left folders
    /// deleted — once; failed, tried again after [`RETRY_AFTER`]. `false`: it can't be
    /// written now.
    fn prepared(&self) -> bool {
        // Decided under the lock; the folder made with it let go (only the result kept).
        match &*lock(&self.ready) {
            Ready::Made(_) => return true,
            Ready::Failed(at) if at.elapsed() < RETRY_AFTER => return false,
            _ => {}
        }
        let made = self.prepare();
        let mut ready = lock(&self.ready);
        // Made by another write meanwhile: that one's kept (this one's lock let go).
        if matches!(*ready, Ready::Made(_)) {
            return true;
        }
        *ready = match made {
            Some(held) => Ready::Made(held),
            None => Ready::Failed(std::time::Instant::now()),
        };
        matches!(*ready, Ready::Made(_))
    }

    fn prepare(&self) -> Option<std::fs::File> {
        let dir = self.dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(error = %e, "can't make the terminals' history folder");
            return None;
        }
        // Never in a commit: the project's data folder and this one ignore everything.
        if self.in_project
            && let Some(data) = self.base.parent()
        {
            ignore_all(data);
        }
        ignore_all(&self.base);
        let held = std::fs::File::create(dir.join(LOCK_FILE)).ok()?;
        held.try_lock().ok()?;
        remove_left(&self.base, &self.run);
        Some(held)
    }

    fn file(&self, token: &SpillToken) -> Option<PathBuf> {
        // A token is our own UUID: anything else is never a path here.
        let safe = token.0.chars().all(|c| c.is_ascii_hexdigit());
        (safe && !token.0.is_empty()).then(|| self.dir().join(format!("{}.bin", token.0)))
    }
}

/// `dir`'s `.gitignore` made ignoring everything, unless one is there — through the shared
/// helper, which keeps what other modules let through (`!` lines) untouched.
fn ignore_all(dir: &Path) {
    if let Err(e) = files::ignore::let_through(dir, &[]) {
        tracing::warn!(error = %e, "the terminals' history folder isn't ignored by git");
    }
}

/// The run folders in `base` (but `own`) no app holds any more, deleted: their lock file
/// there, free, and older than [`LEFT_AFTER`] (one without it, or just made, may be another
/// app's being made).
pub(crate) fn remove_left(base: &Path, own: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(base) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let ours = name.to_str() == Some(own);
        let run = name.to_str().is_some_and(|n| n.starts_with(RUN_PREFIX));
        // Only folders (never a link followed out of here).
        let folder = entry.file_type().is_ok_and(|t| t.is_dir());
        if ours || !run || !folder {
            continue;
        }
        let Ok(lock) = std::fs::File::open(entry.path().join(LOCK_FILE)) else { continue };
        let old = lock
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|at| at.elapsed().is_ok_and(|age| age >= LEFT_AFTER));
        let free = old && lock.try_lock().is_ok();
        drop(lock);
        if free && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

impl SpillStore for DiskSpill {
    fn write(&self, bytes: &[u8]) -> Option<SpillToken> {
        if !self.prepared() {
            return None;
        }
        let key = secrets::RunKey::shared()?;
        let token = SpillToken(format!("{:032x}", kernel::id::new_uuid()));
        let packed = miniz_oxide::deflate::compress_to_vec(bytes, LEVEL);
        let sealed = key.seal(token.0.as_bytes(), &packed).ok()?;
        let file = self.file(&token)?;
        match files::write_private(&file, &sealed) {
            Ok(()) => Some(token),
            Err(e) => {
                tracing::warn!(error = %e, "a terminal's history wasn't written out");
                None
            }
        }
    }

    fn take(&self, token: &SpillToken) -> Option<Vec<u8>> {
        let file = self.file(token)?;
        let sealed = std::fs::read(&file);
        let _ = std::fs::remove_file(&file);
        let sealed = match sealed {
            Ok(sealed) => sealed,
            Err(e) => {
                tracing::warn!(error = %e, "a terminal's history wasn't read back");
                return None;
            }
        };
        let packed = secrets::RunKey::shared()?.open(token.0.as_bytes(), &sealed)?;
        miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, MOST_INFLATED).ok()
    }

    /// Not while making its folder failed lately (tried again after [`RETRY_AFTER`]).
    fn ready(&self) -> bool {
        !matches!(&*lock(&self.ready), Ready::Failed(at) if at.elapsed() < RETRY_AFTER)
    }

    fn remove(&self, token: &SpillToken) {
        if let Some(file) = self.file(token) {
            let _ = std::fs::remove_file(file);
        }
    }
}

impl Drop for DiskSpill {
    /// Let go (its project closed, its terminals gone): its folder goes with it.
    fn drop(&mut self) {
        let ready = std::mem::take(self.ready.get_mut().unwrap_or_else(PoisonError::into_inner));
        if let Ready::Made(held) = ready {
            // Deleting a folder on a worker (this may be the UI thread: a project closed).
            let dir = self.dir();
            kernel::work::run("terminal-park", move || {
                drop(held);
                let _ = std::fs::remove_dir_all(dir);
            });
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/spill.rs"]
mod tests;
