//! Watching the project files ([`workspace::FileWatching`] on the local disk): creating,
//! deleting and renaming files and directories (for the tree), and files whose content
//! changed (open tabs are reloaded from them).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use notify::{EventKind, RecursiveMode, Watcher, event::ModifyKind};
use workspace::{FileWatcher, FileWatching, WatchedDirs};

/// The local disk's watching (`notify`: FSEvents, inotify, ReadDirectoryChangesW).
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalWatching;

impl FileWatching for LocalWatching {
    fn watch(
        &self,
        root: &Path,
        dirs: WatchedDirs,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<Box<dyn FileWatcher>> {
        TreeWatcher::start(root, dirs, wake).map(|w| Box::new(w) as Box<dyn FileWatcher>)
    }
}

/// Paths kept between two looks (each once); past this many (a checkout, an install), they're
/// let go and everything is looked at again instead.
const KEPT: usize = 4096;

/// After too many paths, how long the events must stop before everything is looked at
/// again: once at the end of an install, not at each 4096 of its files.
const QUIET: std::time::Duration = std::time::Duration::from_secs(1);

/// Too many paths heard: when the last event came, and whether a look is waiting for the
/// quiet.
#[derive(Debug)]
struct Storm {
    last: Mutex<std::time::Instant>,
    waiting: AtomicBool,
}

impl Storm {
    fn new() -> Self {
        Self { last: Mutex::new(std::time::Instant::now()), waiting: AtomicBool::new(false) }
    }

    /// Another path past the limit: the look waits for the events to stop (`look`: asks
    /// for it then), on a worker of its own while the storm lasts.
    fn heard(self: &Arc<Self>, look: Arc<dyn Fn() + Send + Sync>) {
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = std::time::Instant::now();
        if self.waiting.swap(true, Ordering::AcqRel) {
            return;
        }
        let storm = Arc::clone(self);
        kernel::work::run("watch-quiet", move || {
            loop {
                let since = storm.last.lock().unwrap_or_else(PoisonError::into_inner).elapsed();
                if since >= QUIET {
                    break;
                }
                std::thread::sleep(QUIET - since);
            }
            storm.waiting.store(false, Ordering::Release);
            look();
        });
    }
}

/// Taken while a watcher registers. The system serves one process's registrations one after
/// another (FSEvents: each a request to `fseventsd`, its cost growing with the number of
/// entries in the executable's folder — ~250 ms beside 150k files, as in `target/debug/deps`),
/// so waiting here costs nothing more, and a project closed before its turn (switched away
/// from at once) is never registered at all instead of delaying the next one's.
static REGISTERING: Mutex<()> = Mutex::new(());

/// The watchers registered so far (what the tests count).
#[cfg(test)]
static REGISTERED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Paths heard, each once, until taken.
#[derive(Debug, Default)]
struct Heard(Mutex<BTreeSet<PathBuf>>);

impl Heard {
    /// Kept; `false` if there are too many (they're let go: look at everything again).
    fn add(&self, path: PathBuf) -> bool {
        let mut paths = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if paths.len() >= KEPT {
            paths.clear();
            return false;
        }
        paths.insert(path);
        true
    }

    fn take(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
            .into_iter()
            .collect()
    }
}

/// What happens to the tree (the affected paths), plus waking the UI.
pub struct TreeWatcher {
    /// Filled in the background (starting FSEvents blocks for a moment, not on the UI
    /// thread); dropped with the tree watcher.
    _watcher: Arc<Mutex<Option<notify::RecommendedWatcher>>>,
    changed: Arc<Heard>,
    /// Files written (their content may have changed: a download, another program).
    written: Arc<Heard>,
    /// Files right inside the project's own folder, written.
    own_written: Arc<Heard>,
    /// Something changed inside a repository's own folder (`.git`: a commit, a checkout):
    /// what's committed may differ.
    vcs: Arc<AtomicBool>,
    /// It just started watching (registering takes a while): what changed before may have
    /// been missed.
    started: Arc<AtomicBool>,
}

impl std::fmt::Debug for TreeWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TreeWatcher")
    }
}

impl TreeWatcher {
    /// Watches `root`; `dirs`: the repositories' own folders (`.git`, from the version
    /// control modules), and those of the project's own folder that are the user's.
    pub fn start(
        root: &Path,
        dirs: WatchedDirs,
        waker: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<Self> {
        let (watcher, handler) = Self::hearing(root, dirs, Arc::clone(&waker));
        let weak = Arc::downgrade(&watcher._watcher);
        let root = root.to_path_buf();
        let live = Arc::clone(&watcher.started);
        let spawned = std::thread::Builder::new().name("tree-watch".into()).spawn(move || {
            // One registration at a time, the project still open when its turn comes.
            let _turn = REGISTERING.lock().unwrap_or_else(PoisonError::into_inner);
            let slot = weak.upgrade()?;
            let mut system = notify::recommended_watcher(handler)
                .inspect_err(|e| tracing::warn!("can't watch the tree: {e}"))
                .ok()?;
            system
                .watch(&root, RecursiveMode::Recursive)
                .inspect_err(|e| tracing::warn!("can't watch {}: {e}", root.display()))
                .ok()?;
            #[cfg(test)]
            REGISTERED.fetch_add(1, Ordering::SeqCst);
            // Closed meanwhile: the watcher drops here, with the last handle on its slot.
            *slot.lock().ok()? = Some(system);
            live.store(true, Ordering::Relaxed);
            waker();
            Some(())
        });
        spawned.ok()?;
        Some(watcher)
    }

    /// The watcher, not yet watching, and what hears the system's events for it (what
    /// [`Self::start`] registers; the tests give it events of their own).
    fn hearing(
        root: &Path,
        dirs: WatchedDirs,
        waker: Arc<dyn Fn() + Send + Sync>,
    ) -> (Self, impl Fn(notify::Result<notify::Event>) + Send + 'static) {
        let (changed, written) = (Arc::<Heard>::default(), Arc::<Heard>::default());
        let (tx, written_tx) = (Arc::clone(&changed), Arc::clone(&written));
        let own_written = Arc::<Heard>::default();
        let own_tx = Arc::clone(&own_written);
        let vcs = Arc::new(AtomicBool::new(false));
        let vcs_flag = Arc::clone(&vcs);
        let vcs_waker = Arc::clone(&waker);
        let vcs_dirs = dirs.vcs.clone();
        let inside = root.to_path_buf();
        let in_vcs_dir =
            move |c: std::path::Component<'_>| vcs_dirs.iter().any(|d| c.as_os_str() == *d);
        let started = Arc::new(AtomicBool::new(false));
        let dropped = Arc::clone(&started);
        let overflowed = Arc::clone(&started);
        let storm = Arc::new(Storm::new());
        let storm_waker = Arc::clone(&waker);
        let look: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            overflowed.store(true, Ordering::Relaxed);
            storm_waker();
        });
        let dropped_waker = Arc::clone(&waker);
        let handler = move |event: notify::Result<notify::Event>| {
            let Ok(mut event) = event else { return };
            // The system dropped events (too many at once, under load): everything is looked
            // at again, as when watching starts.
            if event.need_rescan() {
                dropped.store(true, Ordering::Relaxed);
                dropped_waker();
                return;
            }
            // A save's temporary file is nothing to anyone.
            event.paths.retain(|p| !crate::own_saves::is_temporary(p));
            if event.paths.is_empty() {
                return;
            }
            // `.git/HEAD`, refs, the index: the gutter's committed texts may be out of date
            // (a lock being written isn't a change yet).
            let in_vcs = event.paths.iter().any(|p| {
                p.components().any(&in_vcs_dir) && !p.extension().is_some_and(|e| e == "lock")
            });
            if in_vcs && !vcs_flag.swap(true, Ordering::Relaxed) {
                vcs_waker();
            }
            let structural = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Name(_))
            );
            // Written in place or put there (a temporary file renamed over it).
            let written = matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_));
            // The repository's folder and our own change often (but for the user's files in
            // ours: the consoles').
            let internal = |path: &Path| dirs.internal(path.strip_prefix(&inside).unwrap_or(path));
            // Too many to keep: everything is looked at again instead.
            let kept = |heard: &Heard, path: PathBuf| {
                if !heard.add(path) {
                    storm.heard(Arc::clone(&look));
                }
            };
            // Only what was kept wakes the loop: our own folders' writes (the repository's
            // index refreshed by our `git status`, the sessions, the caches) wake nothing.
            let mut heard = false;
            if written {
                // Files only: a folder is "modified" when an entry in it comes or goes (our
                // save's temporary file renamed in it), which writes nothing of anyone's. Its
                // entries are told as such (structural, below). Asked on this thread.
                for path in event.paths.iter().filter(|p| !internal(p) && !p.is_dir()) {
                    kept(&written_tx, path.clone());
                    heard = true;
                }
                for path in &event.paths {
                    let relative = path.strip_prefix(&inside).unwrap_or(path);
                    // Too many of these is no reason to look at the whole project again.
                    if workspace::WatchedDirs::own_file(relative) && internal(path) {
                        own_tx.add(path.clone());
                        heard = true;
                    }
                }
                if !structural && heard {
                    waker();
                }
            }
            if !structural {
                return;
            }
            for path in event.paths {
                // A file a save of ours put in its own place: written (above), the same entry;
                // the folder it's in, the same entries.
                let ours = matches!(
                    crate::own_saves::heard(&path),
                    crate::own_saves::Heard::Replaced | crate::own_saves::Heard::Holder
                );
                if !internal(&path) && !ours {
                    kept(&tx, path);
                    heard = true;
                }
            }
            if heard {
                waker();
            }
        };
        let slot = Arc::new(Mutex::new(None));
        (Self { _watcher: slot, changed, written, own_written, vcs, started }, handler)
    }
}

impl FileWatcher for TreeWatcher {
    fn drain_written(&self) -> Vec<PathBuf> {
        self.written.take()
    }

    fn drain_own_written(&self) -> Vec<PathBuf> {
        self.own_written.take()
    }

    fn take_rescan(&self) -> bool {
        self.started.swap(false, Ordering::Relaxed)
    }

    fn take_vcs_changed(&self) -> bool {
        self.vcs.swap(false, Ordering::Relaxed)
    }

    fn drain_changed(&self) -> Vec<PathBuf> {
        self.changed.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/watch.rs"]
mod tests;
