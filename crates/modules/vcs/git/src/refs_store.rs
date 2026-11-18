//! A repository's references as every view shows them — its branches and tags, each local
//! branch against what it tracks, its remotes, the current branch's state, its stashes and
//! submodules: read once in the background, held once for every view of the repository (the
//! log's branches panel, the commit screen's places), read again when they may have moved
//! (a push, a fetch, a pull, a commit, a checkout, a branch or tag made: [`moved`]) — every
//! view following the same copy shows the change at once.
//!
//! A view holds its repository's [`Refs`] while it's open; the last one let go, the copy goes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use version_control::{RefLabel, Repository, Stash, SyncState};

/// What was read of the repository's references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RepoRefs {
    pub refs: Vec<RefLabel>,
    /// Each local branch against its upstream.
    pub states: Vec<SyncState>,
    pub remotes: Vec<String>,
    /// The current branch against its upstream (`None`: detached, or nothing tracked).
    pub sync: Option<SyncState>,
    pub stashes: Vec<Stash>,
    pub submodules: Vec<String>,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// A repository's references, shared by its views.
pub(crate) struct Refs {
    repo: Arc<dyn Repository>,
    /// The last read, and how many reads there were (0: none yet).
    read: Mutex<(u64, Option<Arc<RepoRefs>>)>,
    reading: AtomicBool,
    /// Asked again while reading: read once more after.
    again: AtomicBool,
    /// The views' windows, woken when a read comes.
    wakes: Mutex<Vec<Wake>>,
}

impl std::fmt::Debug for Refs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Refs").field("root", &self.repo.root()).finish_non_exhaustive()
    }
}

/// The repositories' shared references (held weakly: by their views), by repository (the
/// project's one, which every view of it is given; its root tells [`moved`] which).
static HELD: Mutex<Vec<(usize, PathBuf, Weak<Refs>)>> = Mutex::new(Vec::new());

/// Which repository `repo` is (the same one for each view of a project).
fn identity(repo: &Arc<dyn Repository>) -> usize {
    Arc::as_ptr(repo).cast::<()>() as usize
}

/// `repo`'s shared references (made, and read, if no view holds them yet); `wake` is told
/// when a read comes.
pub(crate) fn of(repo: &Arc<dyn Repository>, wake: Option<Wake>) -> Arc<Refs> {
    let (id, root) = (identity(repo), repo.root().to_path_buf());
    let refs = {
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|(.., w)| w.strong_count() > 0);
        match held.iter().find(|(i, ..)| *i == id).and_then(|(.., w)| w.upgrade()) {
            Some(refs) => refs,
            None => {
                let refs = Arc::new(Refs {
                    repo: Arc::clone(repo),
                    read: Mutex::new((0, None)),
                    reading: AtomicBool::new(false),
                    again: AtomicBool::new(false),
                    wakes: Mutex::new(Vec::new()),
                });
                held.push((id, root, Arc::downgrade(&refs)));
                refs.refresh();
                refs
            }
        }
    };
    if let Some(wake) = wake {
        refs.follow(wake);
    }
    refs
}

/// The references of the repository at `root` may have moved: every view holding them shows
/// them read again.
pub(crate) fn moved(root: &Path) {
    let held: Vec<Arc<Refs>> = HELD
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|(_, r, _)| r == root)
        .filter_map(|(.., w)| w.upgrade())
        .collect();
    for refs in held {
        refs.refresh();
    }
}

impl Refs {
    /// The last read and its number (`(0, None)`: not read yet).
    pub fn current(&self) -> (u64, Option<Arc<RepoRefs>>) {
        let read = self.read.lock().unwrap_or_else(PoisonError::into_inner);
        (read.0, read.1.clone())
    }

    /// A read is going on (what's shown may change soon).
    pub fn reading(&self) -> bool {
        self.reading.load(Ordering::Acquire)
    }

    /// Wakes `wake` too when a read comes.
    pub fn follow(&self, wake: Wake) {
        let mut wakes = self.wakes.lock().unwrap_or_else(PoisonError::into_inner);
        if !wakes.iter().any(|w| Arc::ptr_eq(w, &wake)) {
            wakes.push(wake);
        }
    }

    /// Read again, in the background (one read at a time: one more if asked meanwhile).
    pub fn refresh(self: &Arc<Self>) {
        if self.reading.swap(true, Ordering::AcqRel) {
            self.again.store(true, Ordering::Release);
            return;
        }
        let weak = Arc::downgrade(self);
        kernel::work::run(THREAD, move || {
            loop {
                let Some(refs) = weak.upgrade() else { return };
                refs.again.store(false, Ordering::Release);
                let repo = refs.repo.as_ref();
                let read = RepoRefs {
                    refs: repo.refs(),
                    states: repo.branch_states(),
                    remotes: repo.remotes().into_iter().map(|r| r.name).collect(),
                    sync: repo.sync_state(),
                    stashes: repo.stashes(),
                    submodules: repo.submodules(),
                };
                {
                    let mut last = refs.read.lock().unwrap_or_else(PoisonError::into_inner);
                    let changed = last.1.as_deref() != Some(&read);
                    if changed || last.0 == 0 {
                        *last = (last.0 + 1, Some(Arc::new(read)));
                    }
                }
                let wakes = refs.wakes.lock().unwrap_or_else(PoisonError::into_inner).clone();
                for wake in wakes {
                    wake();
                }
                if refs.again.load(Ordering::Acquire) {
                    continue;
                }
                refs.reading.store(false, Ordering::Release);
                // Asked between the look and the release (it saw this read going): once more,
                // unless another read took it.
                if !refs.again.load(Ordering::Acquire) || refs.reading.swap(true, Ordering::AcqRel)
                {
                    return;
                }
            }
        });
    }
}

/// The worker the references are read on.
const THREAD: &str = "git-refs";

#[cfg(test)]
#[path = "../tests/unit/refs_store.rs"]
mod tests;
