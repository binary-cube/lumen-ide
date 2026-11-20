//! A project's query histories (plan 39i): each data source's statements, as its consoles
//! ran them, kept in the project ([`storage::TomlQueryHistory`]: a file per source, each run
//! appended, compacted now and then). A source's history is read the first time it's
//! needed — by a run, on the runner's thread; by the history window, in the background —
//! and kept while the project is open; another project's is never seen here.
//!
//! What's kept is the statement as written (the template: a parameter's value never), its
//! secrets masked, where it ran, when, how long and how many rows; the same statement run
//! again moves up ([`database::history::QueryHistory`]). The history window
//! ([`crate::history_view`]) lists them, searched, inserted, run again or deleted.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use database::DataSourceId;
use database::history::{HistoryEntry, HistoryId, HistoryStore, QueryHistory, Write};

use crate::runtime::Runtime;

/// Wakes the app from a background thread.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// A source's history: not read yet, being read, or read.
#[derive(Debug, Default)]
enum Slot {
    #[default]
    Unread,
    Loading,
    Read(QueryHistory),
}

/// A project's histories (see the module's doc).
pub struct Histories {
    store: Mutex<Arc<dyn HistoryStore>>,
    runtime: Mutex<Runtime>,
    sources: Mutex<HashMap<DataSourceId, Slot>>,
    /// One write at a time (the file's blocks in the order of the runs).
    writing: Mutex<()>,
    generation: AtomicU64,
    wakes: Mutex<Vec<(&'static str, Waker)>>,
    /// A broken file kept aside, told once (its path).
    broken: Mutex<Vec<std::path::PathBuf>>,
    /// The most entries kept when not the setting's (the tests').
    limit: Mutex<Option<usize>>,
}

impl std::fmt::Debug for Histories {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Histories").finish_non_exhaustive()
    }
}

impl Histories {
    /// Kept by `store`, read and written on `runtime`.
    pub fn new(store: Arc<dyn HistoryStore>, runtime: Runtime) -> Self {
        Self {
            store: Mutex::new(store),
            runtime: Mutex::new(runtime),
            sources: Mutex::default(),
            writing: Mutex::new(()),
            generation: AtomicU64::new(0),
            wakes: Mutex::default(),
            broken: Mutex::default(),
            limit: Mutex::default(),
        }
    }

    /// At most `limit` entries per source (`None`: the setting's).
    pub fn set_limit(&self, limit: Option<usize>) {
        *self.limit.lock().unwrap_or_else(PoisonError::into_inner) = limit;
    }

    fn limit(&self) -> usize {
        self.limit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or_else(crate::settings::history_size)
    }

    /// Kept by `store` from now (what was read forgotten).
    pub fn set_store(&self, store: Arc<dyn HistoryStore>) {
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) = store;
        self.lock().clear();
        self.changed();
    }

    pub fn set_runtime(&self, runtime: Runtime) {
        *self.runtime.lock().unwrap_or_else(PoisonError::into_inner) = runtime;
    }

    fn store(&self) -> Arc<dyn HistoryStore> {
        Arc::clone(&self.store.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn runtime(&self) -> Runtime {
        self.runtime.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<DataSourceId, Slot>> {
        self.sources.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes with each change of a history.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// `wake` is called when a history changes, as `who`'s (given again, replaced).
    pub fn add_wake(&self, who: &'static str, wake: Waker) {
        let mut wakes = self.wakes.lock().unwrap_or_else(PoisonError::into_inner);
        wakes.retain(|(w, _)| *w != who);
        wakes.push((who, wake));
    }

    fn changed(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let wakes: Vec<Waker> = self
            .wakes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(_, w)| Arc::clone(w))
            .collect();
        for wake in wakes {
            wake();
        }
    }

    /// Source `source`'s file read (here, on the caller's thread).
    fn read(&self, source: DataSourceId) -> QueryHistory {
        let limit = self.limit();
        match self.store().load(source) {
            Ok(loaded) => {
                if let Some(aside) = loaded.broken {
                    tracing::warn!(file = %aside.display(), "a query history couldn't be read: kept aside");
                    self.broken.lock().unwrap_or_else(PoisonError::into_inner).push(aside);
                }
                QueryHistory::from_blocks(loaded.entries, limit)
            }
            Err(why) => {
                tracing::warn!(error = %why, "a query history wasn't read");
                QueryHistory::default()
            }
        }
    }

    /// A broken file kept aside since the last look (its path), told once.
    pub fn take_broken(&self) -> Vec<std::path::PathBuf> {
        std::mem::take(&mut *self.broken.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// `f` on source `source`'s history, read first if it wasn't (on this thread).
    fn with<T>(&self, source: DataSourceId, f: impl FnOnce(&mut QueryHistory) -> T) -> T {
        let read = matches!(self.lock().get(&source), Some(Slot::Read(_)));
        if !read {
            let history = self.read(source);
            let mut sources = self.lock();
            // Read meanwhile by another thread: that one is kept.
            if !matches!(sources.get(&source), Some(Slot::Read(_))) {
                sources.insert(source, Slot::Read(history));
            }
        }
        let mut sources = self.lock();
        let Some(Slot::Read(history)) = sources.get_mut(&source) else {
            unreachable!("read just above")
        };
        f(history)
    }

    /// `entry` run (on the runner's thread): recorded — the same statement moved up —, its
    /// block appended to the source's file (or the file compacted).
    pub fn record(&self, entry: HistoryEntry) {
        let source = entry.source;
        let limit = self.limit();
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let write = self.with(source, |h| h.record(entry, limit));
        self.changed();
        let store = self.store();
        let done = match write {
            Write::Append(entry) => store.append(source, &entry),
            Write::Compact(all) => store.rewrite(source, &all),
        };
        if let Err(why) = done {
            tracing::warn!(error = %why, "a query history wasn't written");
        }
    }

    /// Source `source`'s entries, newest first (`None`: being read, in the background —
    /// its readers woken once it is).
    pub fn entries(self: &Arc<Self>, source: DataSourceId) -> Option<Vec<HistoryEntry>> {
        {
            let mut sources = self.lock();
            match sources.get(&source) {
                Some(Slot::Read(history)) => {
                    return Some(history.newest_first().cloned().collect());
                }
                Some(Slot::Loading) => return None,
                _ => {
                    sources.insert(source, Slot::Loading);
                }
            }
        }
        let this = Arc::downgrade(self);
        self.runtime().spawn(move || {
            let Some(this) = this.upgrade() else { return };
            let history = this.read(source);
            this.lock().insert(source, Slot::Read(history));
            this.changed();
        });
        None
    }

    /// Entry `id` of source `source`'s history deleted (the file written again in the
    /// background). `true` if it was there.
    pub fn remove(self: &Arc<Self>, source: DataSourceId, id: HistoryId) -> bool {
        let rest = {
            let mut sources = self.lock();
            match sources.get_mut(&source) {
                Some(Slot::Read(history)) => history.remove(id),
                _ => None,
            }
        };
        if rest.is_none() {
            return false;
        }
        self.changed();
        self.rewrite(source);
        true
    }

    /// Source `source`'s whole history deleted.
    pub fn clear(self: &Arc<Self>, source: DataSourceId) -> bool {
        {
            let mut sources = self.lock();
            match sources.get_mut(&source) {
                Some(Slot::Read(history)) if !history.is_empty() => history.clear(),
                _ => return false,
            }
        }
        self.changed();
        self.rewrite(source);
        true
    }

    /// Source `source`'s file written again in the background with what it holds then (a
    /// run recorded meanwhile included).
    fn rewrite(self: &Arc<Self>, source: DataSourceId) {
        let this = Arc::downgrade(self);
        self.runtime().spawn(move || {
            let Some(this) = this.upgrade() else { return };
            let _writing = this.writing.lock().unwrap_or_else(PoisonError::into_inner);
            let entries = match this.lock().get(&source) {
                Some(Slot::Read(history)) => history.entries().to_vec(),
                _ => return,
            };
            if let Err(why) = this.store().rewrite(source, &entries) {
                tracing::warn!(error = %why, "a query history wasn't written");
            }
        });
    }

    /// What was read is let go (read again when next needed).
    pub fn purge(&self) {
        self.lock().retain(|_, slot| matches!(slot, Slot::Loading));
        self.changed();
    }
}

#[cfg(test)]
#[path = "../tests/unit/history.rs"]
mod tests;
