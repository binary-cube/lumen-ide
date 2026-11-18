//! What keeps an index up to date: the domain's events, put in a queue that merges them,
//! handed to the index in batches on a thread of its own (`index-queue`).
//!
//! | Event | Asks for |
//! |---|---|
//! | `FileSaved`, `FilesWritten` | the file |
//! | `FileRenamed` | both paths |
//! | `EntriesDeleted`, `EntriesChanged` | the files; a folder: the files in it (walked on the queue's thread), or — gone, with files indexed in it — the whole project looked at again |
//! | `RefactoringApplied` | its files on disk, at once (no waiting for quiet) |
//! | `EntriesRescan` | the whole project looked at again |
//! | `ReindexRequested` | everything again, without the cache (at once) |
//! | `BulkChangeStarted` / `Ended` | nothing handed out until it ends: one batch |
//!
//! The events are the project's own (its bus): another project's never come here. Only what
//! the index wants (its extensions, outside the folders it skips) is asked for.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use document::events::FileSaved;
use kernel::{Batch, EventBus, QueueStats, SharedQueue, Subscription};
use refactor::events::RefactoringApplied;
use workspace::events::{
    BulkChangeEnded, BulkChangeStarted, EntriesChanged, EntriesDeleted, EntriesRescan, FileRenamed,
    FilesWritten, ReindexRequested,
};

/// How long the queue waits for the requests to calm down.
const QUIET: Duration = Duration::from_millis(300);
/// At most this long after the first request (changes that never stop: a build).
const MAX_WAIT: Duration = Duration::from_secs(2);
/// A batch of changes never ended (a paste that died) stops holding after this.
const HOLD_LIMIT: Duration = Duration::from_secs(600);

/// What the queue holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Work {
    /// A path that changed: a file the index wants, a folder (its files), or nothing — told
    /// apart on the queue's thread (the events come on the UI's, thousands at once after an
    /// `npm install`).
    Path(PathBuf),
    /// Everything again, without the cache.
    Rebuild,
}

/// Where the batches go (the project's indexer).
pub trait Sink: Send + Sync {
    /// `path` is one it indexes.
    fn wants(&self, path: &Path) -> bool;
    /// The files it has indexed under folder `dir` (one gone: they go too, each asked for —
    /// missing, it drops them).
    fn indexed_under(&self, dir: &Path) -> Vec<PathBuf>;
    fn changed(&self, paths: Vec<PathBuf>);
    /// The whole project looked at again (only what changed parsed).
    fn refresh(&self);
    fn rebuild(&self);
    /// Shows as busy at once (a rebuild on its way through the queue).
    fn mark_busy(&self);
}

pub struct Feed {
    queue: SharedQueue<Work>,
    _subscriptions: Vec<Subscription>,
}

impl std::fmt::Debug for Feed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Feed").field("stats", &self.queue.stats()).finish_non_exhaustive()
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        // The worker ends (what's still waiting isn't needed: the project is closing).
        self.queue.close();
    }
}

/// Paths asked for, as they are: what each is (a file, a folder) is decided on the queue's
/// thread, never on the UI's.
fn ask(queue: &SharedQueue<Work>, _sink: &dyn Sink, paths: &[PathBuf], urgent: bool) {
    let keys: Vec<Work> = paths.iter().cloned().map(Work::Path).collect();
    if urgent {
        queue.push_urgent(keys);
    } else {
        queue.push_many(keys);
    }
}

/// A batch to the index: the files it wants; a folder's files, walked; a folder gone: the
/// files it had indexed in it (they're dropped); nothing for the rest (a `Makefile` changed).
fn hand_over(sink: &dyn Sink, work: Vec<Work>) {
    let mut paths = Vec::new();
    for item in work {
        let Work::Path(path) = item else { continue };
        if sink.wants(&path) {
            paths.push(path);
        } else if path.is_dir() {
            walk(sink, &path, &mut paths);
        } else if path.extension().is_none() && !path.exists() {
            paths.extend(sink.indexed_under(&path));
        }
    }
    if !paths.is_empty() {
        paths.sort();
        paths.dedup();
        sink.changed(paths);
    }
}

/// The files under `dir` the index wants (the folders it skips left out).
fn walk(sink: &dyn Sink, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            let name = entry.file_name();
            if !workspace::search::skipped_dir(&name.to_string_lossy()) {
                walk(sink, &path, out);
            }
        } else if sink.wants(&path) {
            out.push(path);
        }
    }
}

impl Feed {
    /// Follows the project's `events` for `sink`, until dropped.
    pub fn start(events: &EventBus, sink: Arc<dyn Sink>) -> std::io::Result<Self> {
        Self::start_timed(events, sink, QUIET, MAX_WAIT)
    }

    // Its own thread: it waits on the queue for the project's lifetime (a pool's thread
    // would be held as long).
    #[allow(clippy::disallowed_methods)]
    pub fn start_timed(
        events: &EventBus,
        sink: Arc<dyn Sink>,
        quiet: Duration,
        max_wait: Duration,
    ) -> std::io::Result<Self> {
        let queue = SharedQueue::new(quiet, max_wait, HOLD_LIMIT);
        let worker = (queue.clone(), Arc::clone(&sink));
        std::thread::Builder::new().name("index-queue".into()).spawn(move || {
            let (queue, sink) = worker;
            while let Some(batch) = queue.next() {
                match batch {
                    Batch::All => sink.refresh(),
                    Batch::Keys(work) if work.contains(&Work::Rebuild) => sink.rebuild(),
                    Batch::Keys(work) => hand_over(sink.as_ref(), work),
                }
            }
        })?;
        let on = |urgent: bool| {
            let (queue, sink) = (queue.clone(), Arc::clone(&sink));
            move |paths: &[PathBuf]| ask(&queue, sink.as_ref(), paths, urgent)
        };
        let (saved, written, renamed, deleted, changed, refactored) =
            (on(false), on(false), on(false), on(false), on(false), on(true));
        let (rescan, started, ended) = (queue.clone(), queue.clone(), queue.clone());
        let (reindex, busy) = (queue.clone(), Arc::clone(&sink));
        let subscriptions = vec![
            events.subscribe(move |e: &FileSaved| saved(std::slice::from_ref(&e.path))),
            events.subscribe(move |e: &FilesWritten| written(&e.paths)),
            events.subscribe(move |e: &FileRenamed| renamed(&[e.from.clone(), e.to.clone()])),
            events.subscribe(move |e: &EntriesDeleted| deleted(&e.paths)),
            events.subscribe(move |e: &EntriesChanged| changed(&e.paths)),
            events.subscribe(move |e: &RefactoringApplied| {
                // Changed only in their open documents: read once saved (their declarations
                // are the open documents' meanwhile).
                let mut paths: Vec<PathBuf> =
                    e.changed.iter().filter(|p| !e.unsaved.contains(p)).cloned().collect();
                paths.extend(e.renamed.iter().flat_map(|(from, to)| [from.clone(), to.clone()]));
                refactored(&paths);
            }),
            events.subscribe(move |_: &EntriesRescan| rescan.push_all()),
            events.subscribe(move |_: &ReindexRequested| {
                busy.mark_busy();
                reindex.push_urgent([Work::Rebuild]);
            }),
            events.subscribe(move |e: &BulkChangeStarted| started.hold(e.id)),
            events.subscribe(move |e: &BulkChangeEnded| ended.release(e.id)),
        ];
        Ok(Self { queue, _subscriptions: subscriptions })
    }

    pub fn stats(&self) -> QueueStats {
        self.queue.stats()
    }

    /// Its queue (for reading its stats elsewhere).
    pub fn queue(&self) -> SharedQueue<Work> {
        self.queue.clone()
    }
}

#[cfg(test)]
#[path = "../tests/unit/feed.rs"]
mod tests;
