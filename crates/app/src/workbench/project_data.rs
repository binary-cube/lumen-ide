//! The data of the project open in a window: the symbol index and the indexer, the file
//! index, the usages search, the history of closed tabs,
//! recent files, the language levels, the frameworks, the notifications.
//!
//! Its events have their own bus (see `events`).
//!
//! They live together so they can be replaced all at once (`ProjectData::new()`) when
//! another project is opened: no index, result or history carries over from one project
//! to another. The old project's background threads (indexer, symbols) stop along with
//! them.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};

use code_intelligence::SymbolIndex;

#[derive(Debug, Default)]
pub(super) struct ProjectData {
    /// The project's identity (a UUID v4, new at each opening: replaced wholesale with the
    /// data). What modules keep per project they key by it, never by name or path.
    pub id: kernel::ProjectId,
    /// Its name and path as shown (worked out once: drawn on every frame).
    pub described: Option<ui_workbench::RecentProject>,
    /// The symbol index as last looked at: building, and how many builds had ended.
    pub indexing_seen: (bool, u64),
    /// What the app wrote to the project's files (its saves), for the Task Manager.
    pub disk: workbench_model::DiskIo,
    /// The project's events (its own: another window's project never hears them, a batch
    /// of changes there never holds this one's work); gone with the project, its
    /// subscriptions with it.
    pub events: kernel::EventBus,
    /// The project's own settings (`<project dir>/project.toml`): what modules keep per project (a
    /// formatter, a checker), declared like any setting; gone with the project.
    pub settings: Option<Arc<settings_model::SettingsService>>,
    /// The project's declarations (classes, functions, members), for completion and
    /// definitions; shared with the indexer, which keeps it up to date.
    pub symbols: Arc<RwLock<SymbolIndex>>,
    /// What the user marked in the project.
    pub marks: workspace::marks::ProjectMarks,
    /// The project's index (the indexing module's): kept up to date from the events.
    pub indexer: Option<Box<dyn workbench_model::ProjectIndex>>,
    /// The thread that extracts the active document's symbols (started on the first request).
    pub symbol_worker: Option<crate::symbols::SymbolWorker>,
    /// Event subscriptions of this project's services (they end with the project).
    pub subscriptions: Vec<kernel::Subscription>,
    /// The project's git repository (read with gitoxide: no `git` needed), if any.
    pub git: Option<Arc<dyn version_control::Repository>>,
    /// The repository being looked for in the background.
    pub repository_search:
        Option<std::sync::mpsc::Receiver<Option<Arc<dyn version_control::Repository>>>>,
    /// The modules' decorations of its documents (the changed lines, the blame) and its
    /// label (the branch); their work ends with the project.
    pub decorations: Vec<Box<dyn workbench_model::ProjectDecorations>>,
    /// What undoes the last refactoring (this project's files only).
    pub refactoring_undo: Option<refactor::applying::Undo>,
    /// The files of the closed tabs (most recent last).
    pub closed: Vec<PathBuf>,
    /// The files recently opened in the project (latest at the end), from `<project dir>/recent-files`.
    pub recent_files: Vec<PathBuf>,
    /// The project's language levels (e.g. PHP 8.2).
    pub levels: crate::levels::LanguageLevels,
    /// The frameworks the project uses (Symfony, Laravel, React…), with their support.
    pub frameworks: crate::frameworks::ProjectFrameworks,
    /// How many times the project's files changed (see [`Generations`]).
    pub generations: Arc<Generations>,
    /// What the editor's questions (completion, a hover) read of the project's files, kept
    /// until they change.
    pub disk_reads: Arc<crate::project_disk::DiskReads>,
    /// The manifests read (`composer.json`): their text and the files generation it's from.
    pub manifests: std::collections::HashMap<&'static str, (u64, Option<String>)>,
    /// The modules' background jobs started in this project: their completions come back
    /// here (see `poll_jobs`); gone with the project, the ones not started are cancelled
    /// and the others' completions dropped — they never reach another project.
    pub jobs: ProjectJobs,
    /// The counts the modules put on their tool windows' buttons (this project's).
    pub tool_badges: Vec<(&'static str, workbench_model::ToolBadge)>,
    /// The diagnostics its modules announced (a checker's problems), heard on its bus.
    pub diagnostics: workbench_model::ProjectDiagnostics,
    /// The files whose declarations the index updated one by one, heard on its bus: the
    /// open documents checked against the project are checked again.
    pub index_updates: IndexUpdates,
    /// What the project's modules and the app told the user (the toasts and their
    /// history): another project's never show here.
    pub notifications: workbench_model::Notifications,
    /// Where the project's modules tell it from their threads (shown at the next frame).
    pub notifier: Arc<super::toasts::ProjectNotifier>,
    /// What its documents' language servers were asked and answered (the files announced
    /// to them on its bus).
    pub served: super::servers::ServedRequests,
    /// The code actions at the cursor (the bulb, the list) and what they wait for.
    pub actions: super::context_actions::ActionsState,
    /// The inlay hints and code lenses being worked out (the jobs, the usages counted).
    pub insight: super::insight::InsightJobs,
    /// The sizes the user dragged in this project (its docks', its views' splitters):
    /// saved with its session, given to every view made; another project has its own.
    pub sizes: workbench_model::LayoutSizes,
}

impl ProjectData {
    /// A project's data, following its own bus (the diagnostics announced on it).
    pub fn new() -> Self {
        let mut data = Self::default();
        data.diagnostics = workbench_model::ProjectDiagnostics::listen(&data.events);
        data.index_updates = IndexUpdates::listen(&data.events);
        data
    }
}

/// The files the symbol index updated (`IndexUpdated` on the project's bus) not yet looked
/// at, each once however many times it was announced.
#[derive(Debug, Default)]
pub(super) struct IndexUpdates {
    pending: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    _subscription: Option<kernel::Subscription>,
}

impl IndexUpdates {
    /// Follows the project's `events` (until dropped).
    pub fn listen(events: &kernel::EventBus) -> Self {
        let pending: Arc<std::sync::Mutex<Vec<PathBuf>>> = Arc::default();
        let into = Arc::clone(&pending);
        let subscription = events.subscribe(move |e: &workspace::events::IndexUpdated| {
            if e.index != workspace::events::IndexKind::Symbols {
                return;
            }
            if let Ok(mut pending) = into.lock() {
                // Each once (a checkout's thousands: looked for in a set, not in the list).
                let mut known: std::collections::HashSet<PathBuf> =
                    pending.iter().cloned().collect();
                for path in &e.paths {
                    if known.insert(path.clone()) {
                        pending.push(path.clone());
                    }
                }
            }
        });
        Self { pending, _subscription: Some(subscription) }
    }

    /// What was heard since the last call.
    pub fn take(&self) -> Vec<PathBuf> {
        self.pending.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default()
    }
}

/// The project's background jobs: where their completions arrive, what cancels them, and
/// the project's own pools (another project's work never waits for this one's; their threads
/// start with the work and end when idle).
pub(super) struct ProjectJobs {
    pub tx: std::sync::mpsc::Sender<workbench_model::Completion>,
    pub rx: Receiver<workbench_model::Completion>,
    pub cancel: kernel::Cancel,
    /// The modules' jobs (`host.spawn`: pastes, deletions, git's sync…).
    pub modules: kernel::Pool,
}

impl Default for ProjectJobs {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let pool = |name, limit| kernel::Pool::new(name, limit, kernel::work::IDLE);
        Self {
            tx,
            rx,
            cancel: kernel::Cancel::new(),
            modules: pool("module-job", kernel::work::default_limit()),
        }
    }
}

impl Drop for ProjectJobs {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl std::fmt::Debug for ProjectJobs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectJobs").finish_non_exhaustive()
    }
}

/// How many times the project's files changed, counted from its events alone (whoever
/// changed them: a save, a refactoring, another program): what's gathered from the files
/// (the frameworks' routes, the marks) is recomputed after. Our own data, the repository's
/// internals and what the code generates (a running app's `var/cache`) don't count.
#[derive(Debug, Default)]
pub(super) struct Generations {
    /// Any file written, made, removed or renamed.
    files: std::sync::atomic::AtomicU64,
    /// Only files made, removed or renamed (not written): the lists of files the frameworks
    /// walk are kept until then.
    entries: std::sync::atomic::AtomicU64,
}

impl Generations {
    pub fn files(&self) -> u64 {
        self.files.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn entries(&self) -> u64 {
        self.entries.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The project's file list was built again (what was made or removed is in it now):
    /// what reads it reads it again.
    pub fn list_rebuilt(&self) {
        self.bump(false);
    }

    fn bump(&self, entries: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        self.files.fetch_add(1, Relaxed);
        if entries {
            self.entries.fetch_add(1, Relaxed);
        }
    }

    /// Counts the changes announced on `events` to the files under `root` (`vcs_dirs`: the
    /// repositories' own folders), until the subscriptions are dropped.
    pub fn follow(
        self: &Arc<Self>,
        events: &kernel::EventBus,
        root: &std::path::Path,
        vcs_dirs: Vec<&'static str>,
    ) -> Vec<kernel::Subscription> {
        use workspace::events::{
            EntriesChanged, EntriesDeleted, EntriesRescan, FileRenamed, FilesWritten,
        };
        let root = Arc::new(root.to_path_buf());
        let counts = Arc::new(move |path: &std::path::Path| {
            !path.components().any(|c| {
                vcs_dirs.iter().any(|d| c.as_os_str() == *d)
                    || c.as_os_str() == kernel::brand::PROJECT_DIR
            }) && !path.strip_prefix(root.as_path()).is_ok_and(kernel::paths::in_generated)
        });
        let on = |entries: bool| {
            let (generations, counts) = (Arc::clone(self), Arc::clone(&counts));
            move |paths: &[PathBuf]| {
                if paths.iter().any(|p| counts(p)) {
                    generations.bump(entries);
                }
            }
        };
        let (saved, written, changed, deleted, renamed) =
            (on(false), on(false), on(true), on(true), on(true));
        let rescan = Arc::clone(self);
        vec![
            events.subscribe(move |e: &document::events::FileSaved| {
                saved(std::slice::from_ref(&e.path));
            }),
            events.subscribe(move |e: &FilesWritten| written(&e.paths)),
            events.subscribe(move |e: &EntriesChanged| changed(&e.paths)),
            events.subscribe(move |e: &EntriesDeleted| deleted(&e.paths)),
            events.subscribe(move |e: &FileRenamed| renamed(&[e.from.clone(), e.to.clone()])),
            events.subscribe(move |_: &EntriesRescan| rescan.bump(true)),
        ]
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/project_data.rs"]
mod tests;
