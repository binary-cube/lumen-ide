//! The canvas's graph, built by the module itself: from the project's queries (a copy of
//! the declarations' index, the files, their languages), on a thread of its own (`graph`),
//! the catalog of what can be jumped to kept while the files don't change. A newer request
//! makes an older one give up.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use code_intelligence::{CatalogEntry, DepGraph};
use workbench_model::ProjectQueries;

/// Wakes the app (a redraw) once a graph is ready.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a canvas opens with: the project's queries, and the file (and line) to center on.
#[derive(Clone)]
pub struct CanvasStart {
    pub queries: Arc<dyn ProjectQueries>,
    pub focus: Option<(PathBuf, Option<u32>)>,
}

impl std::fmt::Debug for CanvasStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasStart").field("focus", &self.focus).finish_non_exhaustive()
    }
}

/// The catalog kept: for which file list (its `Arc`) and generation.
type CatalogKey = (usize, u64);

#[derive(Default)]
pub struct Builder {
    queries: Option<Arc<dyn ProjectQueries>>,
    catalog: Option<(CatalogKey, Arc<Vec<CatalogEntry>>)>,
    /// The request going on: its catalog's key and where its graph arrives.
    building: Option<(CatalogKey, Receiver<DepGraph>)>,
    generation: Arc<AtomicU64>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl std::fmt::Debug for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builder").field("building", &self.building.is_some()).finish()
    }
}

impl Builder {
    /// The project to build from (a fresh snapshot each time the canvas is opened).
    pub fn set_queries(&mut self, queries: Arc<dyn ProjectQueries>) {
        self.queries = Some(queries);
    }

    /// Who to wake when a graph is ready (known at the first paint).
    pub fn set_waker(&self, waker: Option<Waker>) {
        if let (Some(waker), Ok(mut slot)) = (waker, self.waker.lock()) {
            *slot = Some(waker);
        }
    }

    pub fn is_building(&self) -> bool {
        self.building.is_some()
    }

    /// Builds the graph around file `focus` (the declaration at `line` to select), in the
    /// background; an older request still going on gives up.
    pub fn request(&mut self, focus: PathBuf, line: Option<u32>) {
        let Some(queries) = self.queries.clone() else { return };
        let files = queries.file_list();
        let key = (Arc::as_ptr(&files) as usize, queries.generation());
        let catalog = self.catalog.as_ref().filter(|(k, _)| *k == key).map(|(_, c)| Arc::clone(c));
        let generation = Arc::clone(&self.generation);
        let mine = generation.fetch_add(1, Ordering::Relaxed) + 1;
        let current = move || generation.load(Ordering::Relaxed) == mine;
        let waker = Arc::clone(&self.waker);
        let (tx, rx) = channel();
        kernel::work::run("graph", move || {
            if !current() {
                return;
            }
            let started = std::time::Instant::now();
            let root = queries.root();
            let read = |path: &Path| -> Option<String> {
                std::fs::metadata(path)
                    .ok()
                    .filter(|m| m.is_file() && m.len() <= workspace::MAX_SOURCE_FILE)?;
                let text = std::fs::read_to_string(path).ok()?;
                crate::DISK.read(text.len() as u64);
                Some(text)
            };
            let list = |_: &Path| Vec::new();
            let language_of = |path: &Path| queries.language_of(path);
            let project = code_intelligence::Project {
                root: Some(&root),
                read: &read,
                list: &list,
                modified: &|_| None,
                levels: &[],
                languages: &[],
            };
            // A copy: building reads files from disk, and holding the index that long would
            // block the indexer.
            let index = queries.symbol_index();
            let rules = queries.source_rules();
            let graph = code_intelligence::build_graph(
                &index,
                &project,
                &files,
                &focus,
                &language_of,
                &rules,
            );
            if !current() {
                return;
            }
            let catalog = catalog.unwrap_or_else(|| {
                Arc::new(code_intelligence::catalog(&index, &root, &files, &rules))
            });
            drop(index);
            if let Some(mut graph) = graph.filter(|_| current()) {
                graph.cursor_line = line;
                graph.catalog = catalog;
                tracing::debug!(files = graph.files.len(), edges = graph.edges.len(), elapsed = ?started.elapsed(), "dependency graph");
                if tx.send(graph).is_ok()
                    && let Some(wake) = waker.lock().ok().and_then(|w| w.clone())
                {
                    wake();
                }
            }
        });
        self.building = Some((key, rx));
    }

    /// The graph built meanwhile, if one is ready (its catalog kept for the next).
    pub fn poll(&mut self) -> Option<DepGraph> {
        let (key, rx) = self.building.as_ref()?;
        let graph = rx.try_recv().ok()?;
        self.catalog = Some((*key, Arc::clone(&graph.catalog)));
        self.building = None;
        Some(graph)
    }
}

#[cfg(test)]
#[path = "../tests/unit/graph.rs"]
mod tests;
