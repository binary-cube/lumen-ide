//! The project's paths index (files and folders: Go to File, the home screen's search, the
//! project search's folders, the filtered file lists): built on a thread of its own
//! (`file-index`) when first asked, marked out of date by the project's events (entries made,
//! removed, renamed; a rescan; a batch of changes ended) and rebuilt on its next use (the old
//! one serving meanwhile), let go after a while unused.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kernel::{EventBus, Subscription};
use workspace::events::{
    BulkChangeEnded, EntriesChanged, EntriesDeleted, EntriesRescan, FileRenamed, ReindexRequested,
};
use workspace::{PathIndex, PathQuery, PathResults};

/// How many files are indexed at most.
pub const LIMIT: usize = workspace::search::FILE_LIMIT;
/// How long the index is kept unused before it's let go (rebuilt on the next search).
const IDLE: Duration = Duration::from_secs(10 * 60);

type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    files: Option<Arc<Vec<String>>>,
    folders: Option<Arc<Vec<String>>>,
    /// The files `.gitignore` leaves out (found by name only).
    ignored: Option<Arc<Vec<String>>>,
    building: Option<Receiver<PathIndex>>,
    used: Option<Instant>,
    /// Entries were made / removed since it was built: rebuilt on its next use.
    stale: bool,
    /// A build finished since [`Paths::take_built`] was last asked.
    built: bool,
}

pub struct Paths {
    root: PathBuf,
    wake: Waker,
    marks: workspace::marks::ProjectMarks,
    state: Arc<Mutex<State>>,
    _subscriptions: Vec<Subscription>,
}

impl std::fmt::Debug for Paths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Paths").field("root", &self.root).finish_non_exhaustive()
    }
}

impl Paths {
    /// The index of `root`'s paths, marked out of date by `events`.
    pub fn new(
        root: &Path,
        events: &EventBus,
        wake: Waker,
        marks: workspace::marks::ProjectMarks,
    ) -> Self {
        let state: Arc<Mutex<State>> = Arc::default();
        let stale = |state: &Arc<Mutex<State>>| {
            let state = Arc::clone(state);
            move || {
                if let Ok(mut state) = state.lock() {
                    state.stale = true;
                }
            }
        };
        let (changed, deleted, renamed, rescan, ended, remarked) = (
            stale(&state),
            stale(&state),
            stale(&state),
            stale(&state),
            stale(&state),
            stale(&state),
        );
        let forget = Arc::clone(&state);
        // The project's own events (its bus): nothing of another project's comes here.
        let subscriptions = vec![
            events.subscribe(move |_: &EntriesChanged| changed()),
            events.subscribe(move |_: &EntriesDeleted| deleted()),
            events.subscribe(move |_: &FileRenamed| renamed()),
            events.subscribe(move |_: &EntriesRescan| rescan()),
            events.subscribe(move |_: &BulkChangeEnded| ended()),
            events.subscribe(move |_: &workspace::events::PathMarksChanged| remarked()),
            // From scratch: forgotten, built again when asked.
            events.subscribe(move |_: &ReindexRequested| {
                if let Ok(mut state) = forget.lock() {
                    *state = State::default();
                }
            }),
        ];
        Self { marks, root: root.to_path_buf(), wake, state, _subscriptions: subscriptions }
    }

    /// Starts building the index in the background, if it's missing or out of date (and not
    /// being built already). `true` if a build started.
    pub fn ensure(&self) -> bool {
        let Ok(mut state) = self.state.lock() else { return false };
        state.used = Some(Instant::now());
        if (state.files.is_some() && !state.stale) || state.building.is_some() {
            return false;
        }
        state.stale = false;
        let (tx, rx) = std::sync::mpsc::channel();
        let (root, wake) = (self.root.clone(), Arc::clone(&self.wake));
        let marks = self.marks.get();
        kernel::work::run("file-index", move || {
            let started = Instant::now();
            let found =
                workspace::search::index_marked(&root, &fs_local::LocalFileSystem, LIMIT, &marks);
            let index = found.unwrap_or_else(|e| {
                tracing::warn!("can't index {}: {e}", root.display());
                PathIndex::default()
            });
            tracing::info!(
                files = index.files.len(),
                folders = index.folders.len(),
                elapsed = ?started.elapsed(),
                "files indexed"
            );
            let _ = tx.send(index);
            wake();
        });
        state.building = Some(rx);
        true
    }

    /// A build finished since the last call (taken in: what shows paths refreshes).
    pub fn take_built(&self) -> bool {
        let Ok(mut state) = self.state.lock() else { return false };
        if let Some(rx) = &state.building
            && let Ok(index) = rx.try_recv()
        {
            state.files = Some(Arc::new(index.files));
            state.folders = Some(Arc::new(index.folders));
            state.ignored = Some(Arc::new(index.ignored));
            state.building = None;
            state.built = true;
        }
        std::mem::take(&mut state.built)
    }

    /// Being built, with nothing to serve yet (the search shows "indexing…").
    pub fn first_build(&self) -> bool {
        self.state.lock().is_ok_and(|s| s.building.is_some() && s.files.is_none())
    }

    /// The project's files (relative, with `/`), if indexed; counts as a use.
    pub fn files(&self) -> Option<Arc<Vec<String>>> {
        let mut state = self.state.lock().ok()?;
        if state.files.is_some() {
            state.used = Some(Instant::now());
        }
        state.files.clone()
    }

    /// The files without counting as a use.
    pub fn peek(&self) -> Option<Arc<Vec<String>>> {
        self.state.lock().ok()?.files.clone()
    }

    /// The folders without counting as a use.
    pub fn peek_folders(&self) -> Option<Arc<Vec<String>>> {
        self.state.lock().ok()?.folders.clone()
    }

    /// The files `.gitignore` leaves out, without counting as a use.
    pub fn peek_ignored(&self) -> Option<Arc<Vec<String>>> {
        self.state.lock().ok()?.ignored.clone()
    }

    /// The one path search (folders and files, fuzzy; the ignored files by name too):
    /// empty until indexed.
    pub fn find(&self, query: &PathQuery<'_>) -> PathResults {
        let Some(files) = self.files() else { return PathResults::default() };
        let folders = self.peek_folders().unwrap_or_default();
        let ignored = self.peek_ignored().unwrap_or_default();
        workspace::search_paths_with(&files, &ignored, &folders, query)
    }

    /// Lets the index go after a while unused. `true` if it did.
    pub fn release_if_idle(&self) -> bool {
        let Ok(mut state) = self.state.lock() else { return false };
        if state.used.is_some_and(|t| t.elapsed() > IDLE) && state.building.is_none() {
            *state = State::default();
            return true;
        }
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/paths.rs"]
mod tests;
