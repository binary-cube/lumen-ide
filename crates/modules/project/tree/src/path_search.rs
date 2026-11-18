//! The panel's search for files and folders, on a thread of its own: matching a large
//! project's paths (or walking it, before it's indexed) never holds up typing. A newer text
//! overtakes the one waiting (it's never searched); the latest result is shown when it comes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};

use kernel::work::{self, Pool, Priority};
use workbench_model::ProjectQueries;
use workspace::{PathQuery, PathResults};

/// Its thread's name (the Task Manager puts it on the module's row).
pub(crate) const THREAD: &str = "tree-search";

#[derive(Debug, Default)]
pub(crate) struct PathSearch {
    rx: Option<Receiver<PathResults>>,
    generation: Arc<AtomicU64>,
    pool: OnceLock<Pool>,
}

impl PathSearch {
    /// Searches `text` (with up to `folders` / `files` results) through `queries`; `wake`
    /// redraws when found. One asked for meanwhile is dropped.
    pub(crate) fn request(
        &mut self,
        queries: Arc<dyn ProjectQueries>,
        text: String,
        (folders, files): (usize, usize),
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let current = Arc::clone(&self.generation);
        let pool = self.pool.get_or_init(|| Pool::new(THREAD, 1, work::IDLE));
        pool.spawn(Priority::Interactive, None, move || {
            if current.load(Ordering::SeqCst) != generation {
                return;
            }
            let results = queries.paths(&PathQuery { text: &text, folders, files });
            if current.load(Ordering::SeqCst) == generation
                && tx.send(results).is_ok()
                && let Some(wake) = wake
            {
                wake();
            }
        });
    }

    /// Nothing to search: the one going on is dropped.
    pub(crate) fn cancel(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.rx = None;
    }

    /// The results found since the last call, if any.
    pub(crate) fn take(&mut self) -> Option<PathResults> {
        let results = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        Some(results)
    }
}
