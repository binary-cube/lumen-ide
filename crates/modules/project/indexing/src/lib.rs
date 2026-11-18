//! The project's index, as a module: it opens the index for the project ([`IndexProvider`])
//! — the declarations and the text, kept in the project's data (`<project dir>/index`) — and keeps
//! it up to date by itself from the domain's events ([`feed`]): nothing in the app or the
//! other modules calls it to reindex. One watcher for the project (the app's), whose word
//! comes as events; batches of changes (a paste) come out as one reindex.

mod exact;
pub mod feed;
pub mod paths;
mod search_panel;
pub mod search_view;
pub mod semantic;
pub mod settings;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, Weak};

use code_intelligence::SymbolIndex;
use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use indexer::ProjectIndexer;
use kernel::QueueStats;
use text_search::CodeSearch;
use workbench_model::{
    Activities, ActivityKind, ActivityUnit, CommandContribution, CommandHandler,
    CommandImplementation, IndexContext, IndexProvider, Outcome, ProjectIndex, ResourceReporter,
    ResourceUse, WorkbenchHost,
};

pub use feed::{Feed, Sink, Work};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("indexing"),
    name: "Indexing",
    description: "The project's index (declarations, text): kept up to date from the project's events",
    depends_on: &[],
};

/// The indexes open (one per window's project), for the Task Manager.
static OPEN: Mutex<Vec<Weak<Shared>>> = Mutex::new(Vec::new());

#[derive(Debug, Default)]
pub struct IndexingModule;

impl Module for IndexingModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager (with the semantic
        // model's download).
        ctx.contribute(workbench_model::ModuleThreads(&["file-index", semantic::THREAD]));
        ctx.contribute(workbench_model::ModuleCounters {
            disk: None,
            traffic: Some(&semantic::TRAFFIC),
        });
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::SemanticSettings::default(),
        ));
        ctx.contribute::<Arc<dyn workbench_model::EmbeddingSource>>(Arc::new(semantic::Source));
        ctx.contribute::<Arc<dyn IndexProvider>>(Arc::new(Provider));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            search_view::SearchProvider,
        ));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: REINDEX,
                label: "Reindex Project",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::TOOLS_MAINTENANCE,
                    10,
                )),
                shortcuts: &[],
            },
            icon: Some("refresh"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::Project],
                Arc::new(Reindex),
            ),
        });
        ctx.contribute::<Arc<dyn ResourceReporter>>(Arc::new(Reporter));
        ctx.contribute(CommandImplementation {
            id: workbench_model::SEARCH_PROJECT,
            handler: Arc::new(SearchInProject { in_folder: false, replace: false }),
        });
        let find_menu =
            command_model::MenuAt::new(workbench_model::menus::sections::FIND_PROJECT, 20);
        let commands = [
            (REPLACE_IN_FILES, "Replace in Files…", Some(find_menu), "replace", false, true),
            (FIND_IN_FOLDER, "Find in Files…", None, "search", true, false),
            (REPLACE_IN_FOLDER, "Replace in Files…", None, "replace", true, true),
        ];
        for (id, label, menu, icon, in_folder, replace) in commands {
            let shortcuts: &'static [&'static str] =
                if id == REPLACE_IN_FILES { &["CmdOrCtrl+Shift+R"] } else { &[] };
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu, shortcuts },
                icon: Some(icon),
                handler: workbench_model::Requiring::new(
                    &[command_model::Requires::Project],
                    Arc::new(SearchInProject { in_folder, replace }),
                ),
            });
        }
        // A folder's right-click menu: searched, or replaced, in it.
        for (command, order) in [(FIND_IN_FOLDER, 10), (REPLACE_IN_FOLDER, 11)] {
            ctx.contribute(workbench_model::menus::ContextMenuItem::new(
                command,
                &[workbench_model::menus::MenuPlace::TreeFolder],
                workbench_model::menus::MenuSection::Navigate,
                order,
            ));
        }
        Ok(())
    }

    /// The semantic model, unused for its own while (also in a window without a project,
    /// where no index looks).
    fn release_idle(&self, _idle: std::time::Duration) -> bool {
        semantic::release_if_idle()
    }
}

/// What a selection fills the search with: its text, if it's on one line (a terminal's may
/// end in the spaces of its row).
fn search_seed(selected: &str) -> Option<String> {
    let line = selected.trim();
    (!line.is_empty() && !line.contains('\n')).then(|| line.to_owned())
}

/// "Find in Files…" / "Replace in Files…" on a folder (the project tree's menu): the search
/// in that folder, to replace or not; "Replace in Files…" in the whole project.
pub const FIND_IN_FOLDER: CommandId = CommandId("project.find_in_folder");
pub const REPLACE_IN_FOLDER: CommandId = CommandId("project.replace_in_folder");
pub const REPLACE_IN_FILES: CommandId = CommandId("project.replace_in_files");

/// "Search in Project…": its view over the window, filled with what's selected where the
/// keyboard is (the editor, the terminal, a view), if it's on one line; `in_folder`, in the
/// folder right-clicked (relative to the project).
struct SearchInProject {
    in_folder: bool,
    replace: bool,
}

impl SearchInProject {
    /// The folder right-clicked, relative to the project (`None`: outside it, or not one).
    fn folder(host: &dyn WorkbenchHost) -> Option<String> {
        let root = host.project_root()?;
        let context = host.context();
        let relative = context.path()?.strip_prefix(&root).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }
}

impl CommandHandler for SearchInProject {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_root().is_some() && (!self.in_folder || Self::folder(host).is_some())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(project), Some(queries)) = (host.project_id(), host.queries()) else {
            return false;
        };
        let folder = if self.in_folder { Self::folder(host) } else { None };
        // From a folder's menu, no selection elsewhere fills the search.
        let selection = if self.in_folder {
            None
        } else {
            host.selected_text().as_deref().and_then(search_seed)
        };
        let semantic: Option<Arc<dyn semantic::SemanticSearch>> =
            Some(Arc::new(semantic::Port(host.user_settings())));
        let replace = self.replace;
        let data =
            search_view::SearchData { project, queries, semantic, selection, folder, replace };
        host.open_overlay(search_view::SEARCH_VIEW, &data, false)
    }
}

/// Builds the project's indexes again from scratch.
pub const REINDEX: CommandId = CommandId("project.reindex");

/// "Reindex Project": asked with an event (whoever indexes the project follows it).
struct Reindex;

impl CommandHandler for Reindex {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_root().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(root) = host.project_root() else { return false };
        host.events().publish(&workspace::events::ReindexRequested { root });
        true
    }
}

#[derive(Debug)]
struct Provider;

impl IndexProvider for Provider {
    fn open(&self, project: IndexContext<'_>) -> std::io::Result<Box<dyn ProjectIndex>> {
        let marks = project.marks.clone();
        let paths = paths::Paths::new(
            project.root,
            project.events,
            Arc::clone(&project.wake),
            marks.clone(),
        );
        // Its window is woken when the semantic model is ready or let go (then it's polled).
        semantic::follow(&project.wake);
        // No language declaring things: only the paths.
        if project.intelligence.is_empty() {
            return Ok(Box::new(Index {
                id: project.id,
                declarations: None,
                paths,
                empty: Arc::default(),
                _forward_or_left: Vec::new(),
                _marks_changed: None,
            }));
        }
        // Told what changed, already gathered (the feed below).
        let indexer = ProjectIndexer::start_marked(
            project.root.to_path_buf(),
            marks,
            project.intelligence,
            project.wake,
        )?;
        let shared = Arc::new_cyclic(|me| Shared {
            indexer,
            id: project.id,
            root: project.root.to_path_buf(),
            activities: Activities::new(std::time::Instant::now),
            following: Mutex::default(),
            stats: Mutex::new(None),
            behind: Mutex::default(),
            me: me.clone(),
        });
        let remarked = Arc::downgrade(&shared);
        let marks_changed =
            project.events.subscribe(move |_: &workspace::events::PathMarksChanged| {
                if let Some(shared) = remarked.upgrade() {
                    shared.indexer.refresh();
                }
            });
        let forward = Arc::downgrade(&shared);
        let left = Arc::downgrade(&shared);
        let forward_or_left = vec![
            project.events.subscribe(move |_: &workspace::events::ProjectBroughtForward| {
                if let Some(shared) = forward.upgrade() {
                    shared.brought_forward();
                }
            }),
            project.events.subscribe(move |_: &workspace::events::ProjectLeft| {
                if let Some(shared) = left.upgrade() {
                    shared.left();
                }
            }),
        ];
        // Left until its window comes forward (opened with the app behind: it may never).
        shared.left();
        let feed = Feed::start(project.events, Arc::clone(&shared) as Arc<dyn Sink>)?;
        if let Ok(mut stats) = shared.stats.lock() {
            *stats = Some(feed.queue());
        }
        if let Ok(mut open) = OPEN.lock() {
            open.retain(|w| w.strong_count() > 0);
            open.push(Arc::downgrade(&shared));
        }
        Ok(Box::new(Index {
            id: project.id,
            declarations: Some((shared, feed)),
            paths,
            empty: Arc::default(),
            _forward_or_left: forward_or_left,
            _marks_changed: Some(marks_changed),
        }))
    }
}

/// The indexer, shared with the feed's thread.
struct Shared {
    indexer: ProjectIndexer,
    /// The project's identity (its parts in the Task Manager go by it).
    id: kernel::ProjectId,
    root: PathBuf,
    /// Its builds as operations (for the Task Manager), and the one followed now.
    activities: Activities,
    following: Mutex<Following>,
    /// What the feed's queue merged (filled once the feed runs).
    stats: Mutex<Option<kernel::SharedQueue<Work>>>,
    /// Behind another project's window, or left: its wait to let go of the index.
    behind: Mutex<Option<kernel::work::Later>>,
    me: Weak<Shared>,
}

/// How long a project stays behind another's before its index leaves the memory.
const REST_AFTER: std::time::Duration = std::time::Duration::from_secs(120);

/// How long a project's window stays left (another program in front, minimized) before its
/// index leaves the memory: longer, it may be the one the user comes back to.
const LEFT_AFTER: std::time::Duration = std::time::Duration::from_secs(600);

impl Shared {
    /// Its window came in front: its index back, the others' on their way out.
    fn brought_forward(&self) {
        let others = OPEN.lock().map_or_else(|_| Vec::new(), |open| open.clone());
        for other in others.iter().filter_map(Weak::upgrade) {
            if other.id != self.id {
                other.rest_after(REST_AFTER);
            }
        }
        self.slot().take();
        self.indexer.wake();
    }

    /// Its window lost the keyboard.
    fn left(&self) {
        self.rest_after(LEFT_AFTER);
    }

    /// Its index leaves the memory after `wait` (unless it rests sooner already, or comes
    /// forward meanwhile).
    fn rest_after(&self, wait: std::time::Duration) {
        #[cfg(test)]
        let wait = tests::WAIT.with(std::cell::Cell::get).unwrap_or(wait);
        let me = self.me.clone();
        kernel::work::Later::sooner(&mut self.slot(), "index-rest", wait, move || {
            if let Some(me) = me.upgrade() {
                me.indexer.rest();
            }
        });
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<kernel::work::Later>> {
        self.behind.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Sink for Shared {
    fn wants(&self, path: &Path) -> bool {
        self.indexer.wants(path)
    }
    fn indexed_under(&self, dir: &Path) -> Vec<PathBuf> {
        let index = self.indexer.index();
        let Ok(index) = index.read() else { return Vec::new() };
        index.files().filter(|(p, _)| p.starts_with(dir)).map(|(p, _)| p.to_path_buf()).collect()
    }
    fn changed(&self, paths: Vec<PathBuf>) {
        self.indexer.changed(paths);
    }
    fn refresh(&self) {
        self.indexer.refresh();
    }
    fn rebuild(&self) {
        self.indexer.rebuild();
    }
    fn mark_busy(&self) {
        self.indexer.mark_busy();
    }
}

struct Index {
    /// The project's identity (what's kept for it goes with the index).
    id: kernel::ProjectId,
    /// The declarations and the text (with the feed keeping them up to date), if any
    /// language declares things.
    declarations: Option<(Arc<Shared>, Feed)>,
    paths: paths::Paths,
    /// The declarations without an indexer: none.
    empty: Arc<RwLock<SymbolIndex>>,
    _forward_or_left: Vec<kernel::Subscription>,
    _marks_changed: Option<kernel::Subscription>,
}

/// The project closed (its index goes with it): what was kept for it goes too.
impl Drop for Index {
    fn drop(&mut self) {
        search_view::forget(self.id);
    }
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Index").field("paths", &self.paths).finish_non_exhaustive()
    }
}

impl Index {
    fn indexer(&self) -> Option<&ProjectIndexer> {
        self.declarations.as_ref().map(|(shared, _)| &shared.indexer)
    }
}

/// Searches nothing (no language declares things).
struct NoText;

impl CodeSearch for NoText {
    fn search(&self, _: &text_search::SearchQuery) -> Vec<text_search::Hit> {
        Vec::new()
    }
    fn related(&self, _: &str, _: u32, _: usize) -> Vec<text_search::Hit> {
        Vec::new()
    }
    fn is_semantic(&self) -> bool {
        false
    }
}

impl ProjectIndex for Index {
    fn symbols(&self) -> Arc<RwLock<SymbolIndex>> {
        self.indexer().map_or_else(|| Arc::clone(&self.empty), ProjectIndexer::index)
    }
    fn text(&self) -> Arc<dyn CodeSearch> {
        self.indexer()
            .map_or_else(|| Arc::new(NoText) as Arc<dyn CodeSearch>, ProjectIndexer::search_api)
    }
    fn is_busy(&self) -> bool {
        self.indexer().is_some_and(ProjectIndexer::is_busy)
    }
    fn take_updated(&self) -> Vec<std::path::PathBuf> {
        self.indexer().map(ProjectIndexer::take_updated).unwrap_or_default()
    }
    fn builds_finished(&self) -> u64 {
        self.indexer().map_or(0, |i| i.usage().built.load(std::sync::atomic::Ordering::Relaxed))
    }
    fn sync_model(&self) -> bool {
        let Some(indexer) = self.indexer() else { return false };
        let model = semantic::model();
        if indexer.is_semantic() && model.is_none() {
            // Turned off, or let go while unused: the index drops it too (only then is its
            // memory released).
            indexer.release_embedder();
            // Now the model's last holder let it go: its memory can return to the system.
            kernel::memory::release_free();
            return true;
        }
        match model {
            Some(model) if !indexer.is_semantic() => {
                indexer.set_embedder(model);
                true
            }
            _ => false,
        }
    }
    fn stats(&self) -> QueueStats {
        self.declarations.as_ref().map(|(_, feed)| feed.stats()).unwrap_or_default()
    }
    fn ensure_paths(&self) -> bool {
        self.paths.ensure()
    }
    fn take_paths_built(&self) -> bool {
        self.paths.take_built()
    }
    fn take_search_ready(&self) -> bool {
        self.declarations.as_ref().is_some_and(|(shared, _)| shared.indexer.take_search_loaded())
    }
    fn paths_first_build(&self) -> bool {
        self.paths.first_build()
    }
    fn find_paths(&self, query: &workspace::PathQuery<'_>) -> workspace::PathResults {
        self.paths.find(query)
    }
    fn files(&self) -> Option<Arc<Vec<String>>> {
        self.paths.files()
    }
    fn peek_files(&self) -> Option<Arc<Vec<String>>> {
        self.paths.peek()
    }
    fn peek_folders(&self) -> Option<Arc<Vec<String>>> {
        self.paths.peek_folders()
    }
    fn peek_ignored(&self) -> Option<Arc<Vec<String>>> {
        self.paths.peek_ignored()
    }
    fn release_idle(&self) -> bool {
        let model = semantic::release_if_idle();
        if model {
            tracing::debug!("the semantic model released (no searches)");
        }
        let paths = self.paths.release_if_idle();
        if paths {
            tracing::debug!("the file index released (unused)");
        }
        model || paths
    }
}

/// The indexer's builds followed as operations: the last build seen, and the one running
/// (its operation, the files counted so far).
#[derive(Debug, Default)]
struct Following {
    builds: u64,
    current: Option<(u128, u64)>,
}

impl Shared {
    /// The indexer's builds since the last look, as operations: one started (a quick one
    /// started and ended between two looks is shown ended), advanced, ended.
    fn follow_builds(&self, usage: &indexer::Usage, name: &str) {
        use std::sync::atomic::Ordering::Relaxed;
        let Ok(mut following) = self.following.lock() else { return };
        let (builds, built) = (usage.builds.load(Relaxed), usage.built.load(Relaxed));
        if builds > following.builds {
            if let Some((id, _)) = following.current.take() {
                self.activities.end(id, Outcome::Done);
            }
            let total = Some(usage.files.load(Relaxed)).filter(|t| *t > 0);
            let id = self.activities.start(
                Some(self.id),
                ActivityKind::Index,
                name,
                ActivityUnit::Files,
                total,
            );
            following.current = Some((id, 0));
            following.builds = builds;
        }
        if let Some((id, counted)) = following.current {
            let done = usage.files_done.load(Relaxed);
            if done > counted {
                self.activities.advance(id, done - counted);
                following.current = Some((id, done));
            }
            if built >= builds {
                self.activities.end(id, Outcome::Done);
                following.current = None;
            }
        }
    }
}

/// What the open indexes hold (files, declarations) and what their queues merged; what
/// each read and wrote, its CPU (its own thread's: the project's), its builds.
#[derive(Debug)]
struct Reporter;

impl ResourceReporter for Reporter {
    fn report(&self) -> Vec<ResourceUse> {
        let open: Vec<Arc<Shared>> =
            OPEN.lock().map(|o| o.iter().filter_map(Weak::upgrade).collect()).unwrap_or_default();
        open.iter()
            .map(|shared| {
                let name = shared.root.file_name().map(|n| n.to_string_lossy().into_owned());
                let name = i18n::tr!("tasks.project_index", project = name.unwrap_or_default());
                let project = shared.root.file_name().map(|n| n.to_string_lossy().into_owned());
                let mut part = ResourceUse::new(name, MANIFEST.id.as_str());
                part.threads = vec!["indexer", "index-queue"];
                part.group = workbench_model::resources::INDEXING;
                part.project = Some(shared.id);
                let usage = shared.indexer.usage();
                let load =
                    |a: &std::sync::atomic::AtomicU64| a.load(std::sync::atomic::Ordering::Relaxed);
                part.read = Some(load(&usage.read));
                part.written = Some(load(&usage.written));
                part.cpu = Some(std::time::Duration::from_nanos(load(&usage.cpu)));
                shared.follow_builds(&usage, &project.unwrap_or_default());
                shared.activities.report(&mut part);
                let files = shared.indexer.index().read().map_or(0, |i| i.files().count());
                let stats = shared
                    .stats
                    .lock()
                    .ok()
                    .and_then(|q| q.as_ref().map(kernel::SharedQueue::stats))
                    .unwrap_or_default();
                part.detail = i18n::tr!(
                    "tasks.project_index_detail",
                    files = files,
                    merged = stats.merged,
                    received = stats.received
                );
                part
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
