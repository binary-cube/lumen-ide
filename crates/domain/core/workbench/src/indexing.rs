//! The project's index, as the app asks for it: a module ([`IndexProvider`]) opens it for
//! the project — its paths (files and folders) and its declarations / text — and keeps it
//! up to date by itself — it follows the domain's events (saves,
//! renames, deletions, the disk, refactorings, batches of changes) — while the app only
//! reads it (the declarations, the text search, the paths); a rebuild is asked for with an event (`ReindexRequested`).

use std::sync::{Arc, RwLock};

use code_intelligence::{LanguageIntelligence, SymbolIndex};
use kernel::{EventBus, QueueStats};
use text_search::CodeSearch;
use workspace::{PathQuery, PathResults};

/// What an index is opened with.
pub struct IndexContext<'a> {
    /// The project's identity (new at each opening).
    pub id: kernel::ProjectId,
    pub root: &'a std::path::Path,
    /// The project's events (followed while the index lives).
    pub events: &'a EventBus,
    /// Wakes the app (a redraw) when the index changed.
    pub wake: Arc<dyn Fn() + Send + Sync>,
    /// The languages that declare things (what's extracted from each file; none: only the
    /// paths are indexed).
    pub intelligence: Vec<Arc<dyn LanguageIntelligence>>,
    /// What the user marked in the project (what's excluded isn't indexed).
    pub marks: workspace::marks::ProjectMarks,
}

impl std::fmt::Debug for IndexContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexContext")
            .field("id", &self.id)
            .field("root", &self.root)
            .field("languages", &self.intelligence.len())
            .finish_non_exhaustive()
    }
}

/// A project's index, while it's open (dropped with the project: its threads end).
pub trait ProjectIndex: Send + Sync + std::fmt::Debug {
    /// The declarations (shared: read by completion, navigation, refactorings).
    fn symbols(&self) -> Arc<RwLock<SymbolIndex>>;
    /// The text search (lexical, semantic with the model).
    fn text(&self) -> Arc<dyn CodeSearch>;
    /// Building (the first time, or rebuilding).
    fn is_busy(&self) -> bool;
    /// How many builds ended so far (one that starts and ends between two looks still
    /// counts).
    fn builds_finished(&self) -> u64;
    /// The files whose declarations changed since the last call, outside a build (a save,
    /// a change on disk, a rename reindexed one by one).
    fn take_updated(&self) -> Vec<std::path::PathBuf> {
        Vec::new()
    }
    /// Its text search takes the model it finds by meaning with, or gives it back (turned
    /// off, or let go while unused). `true` if that changed (an open search finds again).
    fn sync_model(&self) -> bool {
        false
    }
    /// How much its queue merged (for the Task Manager, for tests).
    fn stats(&self) -> QueueStats;

    /// The paths: built in the background if missing or out of date (on use). `true`: a
    /// build started.
    fn ensure_paths(&self) -> bool;
    /// A paths build finished since the last call (what shows paths refreshes).
    fn take_paths_built(&self) -> bool;
    /// The text index was read back since the last look: an open search asks again.
    fn take_search_ready(&self) -> bool {
        false
    }
    /// The paths' first build is running (nothing to serve yet).
    fn paths_first_build(&self) -> bool;
    /// The path search (folders and files, fuzzy); counts as a use.
    fn find_paths(&self, query: &PathQuery<'_>) -> PathResults;
    /// The project's files (relative, with `/`), if indexed; counts as a use.
    fn files(&self) -> Option<Arc<Vec<String>>>;
    /// The files / folders without counting as a use.
    fn peek_files(&self) -> Option<Arc<Vec<String>>>;
    fn peek_folders(&self) -> Option<Arc<Vec<String>>>;
    /// The files `.gitignore` leaves out (found by name, never searched in).
    fn peek_ignored(&self) -> Option<Arc<Vec<String>>> {
        None
    }
    /// Lets go what was unused for a while (the paths, the model it searches with). `true`
    /// if it did.
    fn release_idle(&self) -> bool;
}

pub trait IndexProvider: Send + Sync + std::fmt::Debug {
    fn open(&self, project: IndexContext<'_>) -> std::io::Result<Box<dyn ProjectIndex>>;
}
