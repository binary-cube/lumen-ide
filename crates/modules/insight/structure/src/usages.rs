//! Where the declaration at the cursor is used, for the panel: the project's files of the
//! languages it can be used in searched by its language's rules (the project's queries say
//! which), on a thread of the panel's own; a new declaration cancels the search going on, the
//! same one (moved by typing above it) isn't searched again.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};

use code_intelligence::usages::{UsageQuery, find_usages};
use kernel::work::{self, Pool, Priority};
use ui_components::Usage;
use workbench_model::ProjectQueries;

/// The files it read (the Task Manager's disk column, on the module's row).
pub(crate) static DISK: workbench_model::DiskIo = workbench_model::DiskIo::new();

#[derive(Debug, Default)]
pub(crate) struct Usages {
    query: Option<UsageQuery>,
    result: Option<Vec<Usage>>,
    rx: Option<Receiver<(u64, Vec<Usage>)>>,
    generation: Arc<AtomicU64>,
    /// Its searches, one at a time (this panel's: this window's project).
    pool: OnceLock<Pool>,
}

impl Usages {
    /// The result for the current declaration (`None`: searching, or nothing to search).
    pub(crate) fn result(&self) -> Option<&[Usage]> {
        self.result.as_deref()
    }

    /// Searches `query` (if it's another declaration than the current one) through
    /// `queries`; `wake` redraws when it's found.
    pub(crate) fn request(
        &mut self,
        queries: &Arc<dyn ProjectQueries>,
        query: Option<UsageQuery>,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        let same = match (&query, &self.query) {
            (Some(a), Some(b)) => a.same_symbol(b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.query = query.clone();
        self.result = None;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let Some(query) = query else {
            self.rx = None;
            return;
        };
        let Some((extensions, rules)) = queries.usage_search(&query.language, query.kind) else {
            self.rx = None;
            self.result = Some(Vec::new());
            return;
        };
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let queries = Arc::clone(queries);
        let current = Arc::clone(&self.generation);
        let pool = self.pool.get_or_init(|| Pool::new("usages", 1, work::IDLE));
        // A search overtaken by a newer one stops (or, still waiting, ends at once).
        pool.spawn(Priority::Interactive, None, move || {
            let cancelled = || current.load(Ordering::SeqCst) != generation;
            if cancelled() {
                return;
            }
            let filter = workspace::FileFilter::code(extensions);
            let files: Vec<String> =
                queries.file_list().iter().filter(|f| filter.accepts(f)).cloned().collect();
            let read = |path: &Path| {
                let text = queries.read_source(path)?;
                DISK.read(text.len() as u64);
                Some(text)
            };
            let usages: Vec<Usage> = find_usages(&files, &query, &rules, &read, &cancelled)
                .into_iter()
                .map(|u| Usage { path: u.path, line: u.line })
                .collect();
            if !cancelled()
                && tx.send((generation, usages)).is_ok()
                && let Some(wake) = wake
            {
                wake();
            }
        });
    }

    /// Takes the finished result. `true` if it came.
    pub(crate) fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let Ok((generation, usages)) = rx.try_recv() else { return false };
        self.rx = None;
        if generation != self.generation.load(Ordering::SeqCst) {
            return false;
        }
        self.result = Some(usages);
        true
    }
}
