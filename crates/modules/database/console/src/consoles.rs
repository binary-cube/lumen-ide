//! A project's consoles: their list (`consoles/consoles.toml` in the project's own folder,
//! read when a console is first needed, written in the background), their files beside it
//! (`consoles/<source>/<name>.sql`: one moved or deleted by hand leaves the list, one put
//! there by hand joins it), and their runners (made when a console's bar first opens,
//! their connections the bars'). A console follows its file ([`Consoles::follow`]): renamed
//! or moved within its source's folder — from the tab, the tree, a refactoring, or another
//! program (the disk's changes paired into a rename) — it's the same console (its id, its
//! settings, its runner with its results and history) under the file's new name; moved out
//! of the consoles' folder into the project it stays its source's, as an attached file
//! (said; elsewhere it's no console any more, said too); deleted, it leaves the list.
//!
//! A project's own `.sql` file can be attached to a source too (39n: [`Consoles::attach`],
//! kept in `<project dir>/sql-files.toml`): it then runs, completes and is checked as a
//! console, on a connection of its own. The attachment follows its file the same way
//! (renamed, moved — into the consoles' folder it's a console —, deleted) and goes when
//! it's detached ([`Consoles::detach`]).
//! Registered when the project opens ([`ConsolesService`]),
//! gone with it: every connection closed, a transaction left open rolled back (and said).
//! Another project's consoles are never seen here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use database::DataSourceId;
use database::console::{ConsoleAccess, ConsoleId, ConsoleSpec, ConsoleStore};
use kernel::{EventBus, ProjectId, Subscription};
use workbench_model::{ProjectContext, ProjectService};

use crate::history::Histories;
use crate::runner::Runner;
use crate::runtime::Runtime;

/// The list as read (`None`: not yet).
type Specs = Option<Vec<ConsoleSpec>>;

/// A project's consoles (see the module's doc).
pub struct Consoles {
    project: ProjectId,
    root: PathBuf,
    events: EventBus,
    runtime: Mutex<Runtime>,
    /// Its consoles (`consoles/consoles.toml`; one that couldn't be read is kept as it is,
    /// never written over).
    list: List,
    runners: Mutex<Vec<Runner>>,
    /// The console whose tab may close now (its transaction answered for).
    closing: Mutex<Option<ConsoleId>>,
    /// The tests' port (the app's: the explorer's, registered for the project).
    access: Mutex<Option<Arc<dyn ConsoleAccess>>>,
    notifier: Mutex<Arc<dyn workbench_model::Notifier>>,
    /// Its sources' query histories (kept in memory until a store is given: the project's).
    history: Arc<Histories>,
    /// Its sources' schemas for completion.
    indexes: Arc<crate::schema::Indexes>,
    /// Its consoles checked against those schemas.
    checks: Arc<crate::checks::Checks>,
    /// The project's SQL files attached to a source (`<project dir>/sql-files.toml`).
    files: List,
    /// Bumped by each attachment, detachment or move of one (their bars follow it).
    attachments: std::sync::atomic::AtomicU64,
}

/// A list kept in a file of its own: read once when first needed, written in the
/// background (the newest change last), never written over when it couldn't be read.
struct List {
    store: Mutex<Arc<dyn ConsoleStore>>,
    specs: Mutex<Specs>,
    broken: Mutex<Option<String>>,
    version: Arc<Mutex<u64>>,
    writing: Arc<Mutex<()>>,
}

impl Default for List {
    fn default() -> Self {
        Self {
            store: Mutex::new(Arc::new(database::console::NoConsoles)),
            specs: Mutex::default(),
            broken: Mutex::default(),
            version: Arc::default(),
            writing: Arc::default(),
        }
    }
}

impl List {
    fn set_store(&self, store: Arc<dyn ConsoleStore>) {
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) = store;
        *self.specs.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    fn loaded(&self, what: &str) -> MutexGuard<'_, Specs> {
        let mut specs = self.specs.lock().unwrap_or_else(PoisonError::into_inner);
        if specs.is_none() {
            let store = Arc::clone(&self.store.lock().unwrap_or_else(PoisonError::into_inner));
            let list = match store.load() {
                Ok(list) => list,
                Err(why) => {
                    tracing::warn!(list = %what, error = %why, "a list wasn't read: kept as it is");
                    *self.broken.lock().unwrap_or_else(PoisonError::into_inner) = Some(why);
                    Vec::new()
                }
            };
            *specs = Some(list);
        }
        specs
    }

    fn persist(&self, runtime: &Runtime, list: Vec<ConsoleSpec>, what: &'static str) {
        if self.broken.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
            return;
        }
        let version = {
            let mut v = self.version.lock().unwrap_or_else(PoisonError::into_inner);
            *v += 1;
            *v
        };
        let store = Arc::clone(&self.store.lock().unwrap_or_else(PoisonError::into_inner));
        let (latest, writing) = (Arc::clone(&self.version), Arc::clone(&self.writing));
        runtime.spawn(move || {
            let _writing = writing.lock().unwrap_or_else(PoisonError::into_inner);
            if *latest.lock().unwrap_or_else(PoisonError::into_inner) != version {
                return;
            }
            if let Err(why) = store.save(&list) {
                tracing::warn!(list = %what, error = %why, "a list wasn't written");
            }
        });
    }
}

impl std::fmt::Debug for Consoles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Consoles").field("project", &self.project).finish_non_exhaustive()
    }
}

impl Consoles {
    pub fn new(project: ProjectId, root: &Path, events: EventBus, runtime: Runtime) -> Arc<Self> {
        let consoles = Arc::new(Self {
            project,
            root: root.to_path_buf(),
            events,
            list: List::default(),
            runners: Mutex::default(),
            closing: Mutex::default(),
            access: Mutex::default(),
            notifier: Mutex::new(Arc::new(workbench_model::NoNotifier)),
            history: Arc::new(Histories::new(
                Arc::new(database::history::MemoryHistory::default()),
                runtime.clone(),
            )),
            indexes: crate::schema::Indexes::new(runtime.clone()),
            checks: crate::checks::Checks::new(runtime.clone()),
            runtime: Mutex::new(runtime),
            files: List::default(),
            attachments: std::sync::atomic::AtomicU64::new(0),
        });
        crate::checks::follow_indexes(&consoles);
        consoles
    }

    /// Its consoles' checks against their sources' schemas.
    pub fn checks(&self) -> &Arc<crate::checks::Checks> {
        &self.checks
    }

    /// Its sources' query histories.
    pub fn history(&self) -> Arc<Histories> {
        Arc::clone(&self.history)
    }

    /// Its sources' schemas for completion.
    pub fn indexes(&self) -> &Arc<crate::schema::Indexes> {
        &self.indexes
    }

    /// Source `source` has a console open (its bar's lease taken).
    pub fn open_on(&self, source: DataSourceId) -> bool {
        self.runners().iter().any(|r| r.source() == source && r.leases() > 0)
    }

    /// How long an unused index stays (the sources' idle close, else ten minutes).
    fn index_idle(&self) -> std::time::Duration {
        self.access()
            .and_then(|a| a.idle_close())
            .unwrap_or(std::time::Duration::from_secs(10 * 60))
    }

    pub fn project(&self) -> ProjectId {
        self.project
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn events(&self) -> &EventBus {
        &self.events
    }

    pub fn runtime(&self) -> Runtime {
        self.runtime.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Its work runs on `runtime` from now (the tests' and the snapshots' clocks; the
    /// runners made after).
    pub fn set_runtime(&self, runtime: Runtime) {
        self.history.set_runtime(runtime.clone());
        self.indexes.set_runtime(runtime.clone());
        self.checks.set_runtime(runtime.clone());
        *self.runtime.lock().unwrap_or_else(PoisonError::into_inner) = runtime;
    }

    /// The list is kept by `store` (the project's file), read again when next needed.
    pub fn set_store(&self, store: Arc<dyn ConsoleStore>) {
        self.list.set_store(store);
    }

    /// The attached SQL files are kept by `store` (the project's `sql-files.toml`), read
    /// again when next needed.
    pub fn set_files_store(&self, store: Arc<dyn ConsoleStore>) {
        self.files.set_store(store);
        self.bump_attachments();
    }

    /// A number changing with each attachment, detachment or move of one: the files' bars
    /// look again when it does.
    pub fn attachments(&self) -> u64 {
        self.attachments.load(std::sync::atomic::Ordering::Acquire)
    }

    fn bump_attachments(&self) {
        self.attachments.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.indexes.wake_app();
    }

    fn loaded_files(&self) -> MutexGuard<'_, Specs> {
        self.files.loaded("attached SQL files'")
    }

    fn persist_files(&self, list: Vec<ConsoleSpec>) {
        self.files.persist(&self.runtime(), list, "attached SQL files'");
    }

    /// Every SQL file of the project attached to a source.
    pub fn attached_files(&self) -> Vec<ConsoleSpec> {
        self.loaded_files().clone().unwrap_or_default()
    }

    /// The attachment of the project's SQL file `path` (`None`: not attached, or no
    /// project's SQL file).
    pub fn attached_of(&self, path: &Path) -> Option<ConsoleSpec> {
        let relative = database::console::attachable(&self.root, path)?;
        self.loaded_files().as_ref()?.iter().find(|s| s.file == relative).cloned()
    }

    /// The project's SQL file `path` attached to `source` (the same attachment kept when it
    /// already is; another source's: its database and schema let go, its connection
    /// closed — a transaction left open rolled back, said). `None`: not a SQL file of the
    /// project.
    pub fn attach(&self, path: &Path, source: DataSourceId) -> Option<ConsoleSpec> {
        let relative = database::console::attachable(&self.root, path)?;
        let production = self.access().is_some_and(|a| a.production(source));
        let (spec, all, retired) = {
            let mut files = self.loaded_files();
            let list = files.get_or_insert_with(Vec::new);
            match list.iter_mut().find(|s| s.file == relative) {
                Some(found) if found.source == source => return Some(found.clone()),
                Some(found) => {
                    found.source = source;
                    found.database = None;
                    found.schema = None;
                    found.tx = database::console::TxMode::default_for(production);
                    (found.clone(), list.clone(), true)
                }
                None => {
                    let mut spec = ConsoleSpec::attached(source, relative);
                    spec.tx = database::console::TxMode::default_for(production);
                    list.push(spec.clone());
                    (spec, list.clone(), false)
                }
            }
        };
        if retired {
            self.retire(spec.id);
        }
        tracing::info!(file = %spec.file.display(), %source, "a SQL file attached to a source");
        self.persist_files(all);
        self.bump_attachments();
        Some(spec)
    }

    /// The project's SQL file `path` detached from its source: it leaves the list, its
    /// connection closed (a transaction left open rolled back, said). The attachment it
    /// had, if any.
    pub fn detach(&self, path: &Path) -> Option<ConsoleSpec> {
        let relative = database::console::attachable(&self.root, path)?;
        let (gone, all) = {
            let mut files = self.loaded_files();
            let list = files.as_mut()?;
            let at = list.iter().position(|s| s.file == relative)?;
            (list.remove(at), list.clone())
        };
        self.retire(gone.id);
        self.checks.forget(self, path);
        tracing::info!(file = %gone.file.display(), "a SQL file detached from its source");
        self.persist_files(all);
        self.bump_attachments();
        Some(gone)
    }

    /// The sources are reached through `access` (the tests'; the app's is the explorer's).
    pub fn set_access(&self, access: Arc<dyn ConsoleAccess>) {
        *self.access.lock().unwrap_or_else(PoisonError::into_inner) = Some(access);
    }

    pub fn set_notifier(&self, notifier: Arc<dyn workbench_model::Notifier>) {
        *self.notifier.lock().unwrap_or_else(PoisonError::into_inner) = notifier;
    }

    pub fn notify(&self, notification: workbench_model::Notification) {
        let notifier = Arc::clone(&self.notifier.lock().unwrap_or_else(PoisonError::into_inner));
        notifier.show(notification);
    }

    /// The data sources' port: the tests', else the project's (the explorer's).
    pub fn access(&self) -> Option<Arc<dyn ConsoleAccess>> {
        let own = self.access.lock().unwrap_or_else(PoisonError::into_inner).clone();
        own.or_else(|| database::console::access(self.project))
    }

    /// The list, read now if it wasn't (a small file, read once).
    fn loaded(&self) -> MutexGuard<'_, Specs> {
        self.list.loaded("consoles'")
    }

    /// Every console kept.
    pub fn specs(&self) -> Vec<ConsoleSpec> {
        self.loaded().clone().unwrap_or_default()
    }

    /// Console `id`, or the attached file `id`.
    pub fn spec(&self, id: ConsoleId) -> Option<ConsoleSpec> {
        let console = self.loaded().as_ref().and_then(|l| l.iter().find(|s| s.id == id).cloned());
        console.or_else(|| self.loaded_files().as_ref()?.iter().find(|s| s.id == id).cloned())
    }

    /// The console whose file is `path` (one put there by hand joins the list), or the
    /// attachment of the project's SQL file `path`; `None`: neither.
    pub fn spec_of_path(&self, path: &Path) -> Option<ConsoleSpec> {
        let Some((source, name)) = database::console::identify(&self.root, path) else {
            return self.attached_of(path);
        };
        let mut specs = self.loaded();
        let list = specs.get_or_insert_with(Vec::new);
        let relative = database::console::relative_file(source, &name);
        if let Some(found) = list.iter().find(|s| s.source == source && s.file == relative) {
            return Some(found.clone());
        }
        let spec = self.new_spec(source, name);
        list.push(spec.clone());
        let list = list.clone();
        drop(specs);
        self.persist(list);
        Some(spec)
    }

    /// Source `source`'s consoles, as on disk: one whose file is gone leaves the list, a
    /// file put in its folder by hand joins it (by name).
    pub fn consoles_of(&self, source: DataSourceId) -> Vec<ConsoleSpec> {
        let folder = database::console::folder(&self.root).join(source.to_string());
        let on_disk: Vec<String> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter_map(|e| {
                        let path = e.path();
                        let sql = path
                            .extension()
                            .and_then(|x| x.to_str())
                            .is_some_and(|x| x.eq_ignore_ascii_case("sql"));
                        let name = path.file_stem()?.to_str()?.to_owned();
                        (sql && path.is_file()).then_some(name)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut specs = self.loaded();
        let list = specs.get_or_insert_with(Vec::new);
        let before = list.len();
        list.retain(|s| {
            s.source != source
                || on_disk.iter().any(|n| s.file == database::console::relative_file(source, n))
        });
        let mut changed = list.len() != before;
        for name in &on_disk {
            let file = database::console::relative_file(source, name);
            if !list.iter().any(|s| s.source == source && s.file == file) {
                list.push(self.new_spec(source, name.clone()));
                changed = true;
            }
        }
        let mut mine: Vec<ConsoleSpec> =
            list.iter().filter(|s| s.source == source).cloned().collect();
        let all = list.clone();
        drop(specs);
        if changed {
            self.persist(all);
        }
        mine.sort_by_key(|s| natural(&s.name));
        mine
    }

    /// A new console of `source` (`console`, `console_1`…), its file written with `text`.
    pub fn create(&self, source: DataSourceId, text: &str) -> std::io::Result<ConsoleSpec> {
        let taken: Vec<String> = self.consoles_of(source).into_iter().map(|s| s.name).collect();
        let name = database::console::next_name(taken.iter().map(String::as_str));
        let spec = self.new_spec(source, name);
        let path = spec.path(&self.root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, text)?;
        let all = {
            let mut specs = self.loaded();
            let list = specs.get_or_insert_with(Vec::new);
            list.push(spec.clone());
            list.clone()
        };
        self.persist(all);
        Ok(spec)
    }

    /// A new console of `source`: Manual on a production source, Auto elsewhere.
    fn new_spec(&self, source: DataSourceId, name: String) -> ConsoleSpec {
        let mut spec = ConsoleSpec::new(source, name);
        let production = self.access().is_some_and(|a| a.production(source));
        spec.tx = database::console::TxMode::default_for(production);
        spec
    }

    /// Console (or attached file) `id` changed by `change` (its schema, its modes), its
    /// list written.
    pub fn update(
        &self,
        id: ConsoleId,
        change: impl FnOnce(&mut ConsoleSpec),
    ) -> Option<ConsoleSpec> {
        let attached = !self.loaded().as_ref().is_some_and(|l| l.iter().any(|s| s.id == id));
        let (spec, all) = {
            let mut specs = if attached { self.loaded_files() } else { self.loaded() };
            let list = specs.as_mut()?;
            let spec = list.iter_mut().find(|s| s.id == id)?;
            let before = spec.clone();
            change(spec);
            if *spec == before {
                return Some(before);
            }
            (spec.clone(), list.clone())
        };
        if attached {
            self.persist_files(all);
        } else {
            self.persist(all);
        }
        Some(spec)
    }

    /// Console `id` renamed `name` (typed: checked first — [`database::console::check_name`]
    /// against its source's others): its file renamed on disk, then announced on the
    /// project's bus as any rename ([`workspace::events::FileRenamed`]), so it's followed the
    /// same way as one made from its tab or the tree — the same console (its id, settings and
    /// runner) under the new name, its tabs following. The new spec, or why not.
    pub fn rename(&self, id: ConsoleId, name: &str) -> Result<ConsoleSpec, RenameError> {
        // An attached file is the project's: renamed as any file, never from here.
        let spec = self.spec(id).filter(|s| !s.is_attached()).ok_or(RenameError::Gone)?;
        let others: Vec<String> = self
            .consoles_of(spec.source)
            .into_iter()
            .filter(|s| s.id != id)
            .map(|s| s.name)
            .collect();
        let name = database::console::check_name(name, others.iter().map(String::as_str))
            .map_err(RenameError::Name)?;
        if name == spec.name {
            return Ok(spec);
        }
        let from = spec.path(&self.root);
        let to = from.with_file_name(format!("{name}.sql"));
        // Another case only (`console` → `Console`): the same file on some systems.
        std::fs::rename(&from, &to).map_err(|e| RenameError::Io(e.to_string()))?;
        self.events.publish(&workspace::events::FileRenamed { from, to: to.clone() });
        // Followed (heard on the bus); a bus without the subscription (the tests') follows here.
        if self.spec(id).is_some_and(|s| s.path(&self.root) != to) {
            self.moved(&spec.path(&self.root), &to);
        }
        self.spec(id).ok_or(RenameError::Gone)
    }

    /// Console `id` deleted: its file removed, announced on the project's bus
    /// ([`workspace::events::EntriesDeleted`]: its saved tab closes, it leaves the list, its
    /// runner goes — a transaction left open rolled back, said).
    pub fn delete(&self, id: ConsoleId) -> Result<ConsoleSpec, RenameError> {
        let spec = self.spec(id).filter(|s| !s.is_attached()).ok_or(RenameError::Gone)?;
        let file = spec.path(&self.root);
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(RenameError::Io(e.to_string()));
            }
            _ => {}
        }
        self.events.publish(&workspace::events::EntriesDeleted { paths: vec![file.clone()] });
        if self.spec(id).is_some() {
            self.deleted(&[file]);
        }
        Ok(spec)
    }

    /// Follows the consoles' files on the project's bus (until the subscriptions go): a
    /// rename or move, whoever made it ([`FileRenamed`]); a deletion ([`EntriesDeleted`]);
    /// the disk's changes ([`EntriesChanged`]: another program's rename is announced as one).
    ///
    /// [`FileRenamed`]: workspace::events::FileRenamed
    /// [`EntriesDeleted`]: workspace::events::EntriesDeleted
    /// [`EntriesChanged`]: workspace::events::EntriesChanged
    pub fn follow(self: &Arc<Self>) -> Vec<Subscription> {
        use database::events::{DataChanged, DataSourcesChanged, SchemaInvalidated};
        use workspace::events::{EntriesChanged, EntriesDeleted, FileRenamed, ReindexRequested};
        let (renamed, deleted, changed) =
            (Arc::downgrade(self), Arc::downgrade(self), Arc::downgrade(self));
        // The schemas for completion: read again when stale, forgotten with the caches.
        let indexes = Arc::downgrade(&self.indexes);
        let (stale, structure, purged) = (indexes.clone(), indexes.clone(), indexes);
        let sources = Arc::downgrade(self);
        vec![
            self.events.subscribe(move |e: &SchemaInvalidated| {
                if let Some(indexes) = stale.upgrade() {
                    indexes.invalidate(e.source);
                }
            }),
            self.events.subscribe(move |e: &DataChanged| {
                if e.structure
                    && let Some(indexes) = structure.upgrade()
                {
                    indexes.invalidate(e.source);
                }
            }),
            self.events.subscribe(move |_: &DataSourcesChanged| {
                if let Some(consoles) = sources.upgrade()
                    && let Some(access) = consoles.access()
                {
                    consoles.indexes.sources_changed(access.as_ref());
                }
            }),
            self.events.subscribe(move |_: &ReindexRequested| {
                if let Some(indexes) = purged.upgrade() {
                    indexes.purge();
                }
            }),
            self.events.subscribe(move |e: &FileRenamed| {
                if let Some(consoles) = renamed.upgrade() {
                    consoles.moved(&e.from, &e.to);
                }
            }),
            self.events.subscribe(move |e: &EntriesDeleted| {
                if let Some(consoles) = deleted.upgrade() {
                    consoles.deleted(&e.paths);
                }
            }),
            self.events.subscribe(move |e: &EntriesChanged| {
                if let Some(consoles) = changed.upgrade() {
                    consoles.disk_changed(&e.paths);
                }
            }),
        ]
    }

    /// `path` is the consoles' folder, inside it, or holds it.
    fn touches(&self, path: &Path) -> bool {
        let folder = database::console::folder(&self.root);
        path.starts_with(&folder) || folder.starts_with(path)
    }

    /// Console `id`, its file now at `path` (told by its bar when the tab follows the file):
    /// followed if the bus hadn't said so yet. `None`: no console of this id is that file.
    pub fn follow_file(&self, id: ConsoleId, path: &Path) -> Option<ConsoleSpec> {
        let spec = self.spec(id)?;
        let file = spec.path(&self.root);
        if file != path {
            self.moved(&file, path);
        }
        self.spec(id).filter(|s| s.path(&self.root) == path)
    }

    /// An attached file is `path`, inside it, or holds it (`path` is anywhere in the
    /// project: only the attached files read are looked at).
    fn touches_files(&self, path: &Path) -> bool {
        let root = &self.root;
        self.loaded_files()
            .iter()
            .flatten()
            .any(|s| s.path(root).starts_with(path) || path.starts_with(s.path(root)))
    }

    /// `from` (a console's or an attached file, or a folder holding some) was moved to `to`:
    /// each follows — a console renamed within its source's folder is the same one, moved
    /// to another source's folder that source's (its database and schema let go), out of
    /// the consoles' folder into the project an attached file of the same source (said),
    /// elsewhere a plain file (it leaves the list, said); an attached file renamed or moved
    /// in the project stays attached, moved into a source's consoles' folder it's that
    /// source's console, out of the project (or no SQL file any more) it's detached (said).
    fn moved(&self, from: &Path, to: &Path) {
        let consoles = self.touches(from) || self.touches(to);
        if !consoles && !self.touches_files(from) {
            return;
        }
        let root = self.root.clone();
        let mut renamed = Vec::new();
        let mut retired = Vec::new();
        let mut kept_attached = Vec::new();
        let mut detached = Vec::new();
        let (all, files) = {
            let mut specs = self.loaded();
            let mut attached = self.loaded_files();
            let (Some(list), Some(files)) = (specs.as_mut(), attached.as_mut()) else { return };
            let (before, files_before) = (list.clone(), files.clone());
            let mut kept = Vec::with_capacity(list.len());
            let mut kept_files = Vec::with_capacity(files.len());
            let consoles_then_files = std::mem::take(list)
                .into_iter()
                .chain(std::mem::take(files))
                .collect::<Vec<ConsoleSpec>>();
            for mut spec in consoles_then_files {
                let Some(new) = workspace::clipboard::moved_path(&spec.path(&root), from, to)
                else {
                    if spec.is_attached() {
                        kept_files.push(spec);
                    } else {
                        kept.push(spec);
                    }
                    continue;
                };
                if let Some((source, name)) = database::console::identify(&root, &new) {
                    if source != spec.source {
                        spec.source = source;
                        spec.database = None;
                        spec.schema = None;
                        retired.push((spec.id, None));
                    }
                    spec.kind = database::console::ConsoleKind::Console;
                    spec.file = database::console::relative_file(source, &name);
                    spec.name = name;
                    renamed.push(spec.id);
                    kept.push(spec);
                } else if let Some(relative) = database::console::attachable(&root, &new) {
                    // A console moved into the project keeps its source (and everything).
                    if !spec.is_attached() {
                        kept_attached.push((spec.name.clone(), relative.clone(), spec.source));
                    }
                    let attached = ConsoleSpec::attached(spec.source, relative);
                    spec.kind = attached.kind;
                    spec.name = attached.name;
                    spec.file = attached.file;
                    renamed.push(spec.id);
                    kept_files.push(spec);
                } else if spec.is_attached() {
                    detached.push((spec.id, spec.name.clone()));
                } else {
                    retired.push((spec.id, Some((spec, new))));
                }
            }
            // One of the same file joined before (put there by hand, attached anew): the one
            // followed stays.
            let followed = |kept: &[ConsoleSpec]| -> Vec<PathBuf> {
                kept.iter().filter(|s| renamed.contains(&s.id)).map(|s| s.file.clone()).collect()
            };
            let (followed_consoles, followed_files) = (followed(&kept), followed(&kept_files));
            kept.retain(|s| renamed.contains(&s.id) || !followed_consoles.contains(&s.file));
            kept_files.retain(|s| renamed.contains(&s.id) || !followed_files.contains(&s.file));
            *list = kept;
            *files = kept_files;
            (
                (*list != before).then(|| list.clone()),
                (*files != files_before).then(|| files.clone()),
            )
        };
        let source_name = |source: DataSourceId| {
            self.access().and_then(|a| a.source(source)).map(|b| b.name).unwrap_or_default()
        };
        for (id, left) in retired {
            self.retire(id);
            if let Some((spec, file)) = left {
                let file = file.file_name().map(|n| n.to_string_lossy().into_owned());
                self.notify(workbench_model::Notification::info(i18n::tr!(
                    "console.left_folder",
                    name = spec.name,
                    file = file.unwrap_or_default(),
                    source = source_name(spec.source)
                )));
            }
        }
        for (id, name) in detached {
            self.retire(id);
            self.notify(workbench_model::Notification::info(i18n::tr!(
                "console.detached_on_move",
                name = name
            )));
        }
        for (name, file, source) in kept_attached {
            let file =
                file.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>();
            self.notify(workbench_model::Notification::info(i18n::tr!(
                "console.kept_attached",
                name = name,
                file = file.join("/"),
                source = source_name(source)
            )));
        }
        for id in &renamed {
            if let Some(runner) = self.runner_of(*id) {
                runner.renamed(self.context_of(*id));
            }
        }
        if let Some(all) = all {
            self.persist(all);
        }
        let files_changed = files.is_some();
        if let Some(files) = files {
            self.persist_files(files);
        }
        if files_changed || !renamed.is_empty() {
            self.bump_attachments();
        }
    }

    /// `paths` were deleted: their consoles and attached files leave their lists (their
    /// runners too).
    fn deleted(&self, paths: &[PathBuf]) {
        if !paths.iter().any(|p| self.touches(p) || self.touches_files(p)) {
            return;
        }
        let was_deleted =
            |s: &ConsoleSpec| workspace::delete::was_deleted(&s.path(&self.root), paths);
        let (gone, all) = {
            let mut specs = self.loaded();
            let Some(list) = specs.as_mut() else { return };
            let (gone, kept): (Vec<ConsoleSpec>, Vec<ConsoleSpec>) =
                std::mem::take(list).into_iter().partition(was_deleted);
            *list = kept;
            (gone, list.clone())
        };
        let (gone_files, files) = {
            let mut attached = self.loaded_files();
            let Some(list) = attached.as_mut() else { return };
            let (gone, kept): (Vec<ConsoleSpec>, Vec<ConsoleSpec>) =
                std::mem::take(list).into_iter().partition(was_deleted);
            *list = kept;
            (gone, list.clone())
        };
        for spec in gone.iter().chain(&gone_files) {
            self.retire(spec.id);
        }
        if !gone.is_empty() {
            self.persist(all);
        }
        if !gone_files.is_empty() {
            self.persist_files(files);
            self.bump_attachments();
        }
    }

    /// The disk's changes (another program's, the watcher's): a console's file gone with
    /// one `.sql` file new beside it (or, alone, anywhere) was renamed — announced as such
    /// ([`workspace::events::FileRenamed`]: the tabs, the tree and the consoles follow it);
    /// one gone with none is deleted (announced too: its saved tab closes). An attached
    /// file gone with one SQL file new in its folder was renamed: the attachment follows
    /// (the project's files are the app's to follow, so nothing's announced); one gone with
    /// none stays attached (it may come back: another branch checked out).
    fn disk_changed(&self, paths: &[PathBuf]) {
        let sql = |p: &Path| {
            p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                database::console::SQL_EXTENSIONS.iter().any(|e| x.eq_ignore_ascii_case(e))
            })
        };
        // A SQL file, or a folder (the lists' own files, written often, aren't looked at).
        let relevant = |p: &PathBuf| p.extension().is_none() || sql(p);
        if !paths.iter().any(relevant) {
            return;
        }
        self.files_disk_changed(paths, &sql);
        if !paths.iter().any(|p| self.touches(p) && relevant(p)) {
            return;
        }
        let known: Vec<PathBuf> = {
            let specs = self.loaded();
            specs.iter().flatten().map(|s| s.path(&self.root)).collect()
        };
        let gone: Vec<PathBuf> = known
            .iter()
            .filter(|k| paths.iter().any(|p| k.starts_with(p)) && !k.exists())
            .cloned()
            .collect();
        if gone.is_empty() {
            return;
        }
        let console_sql = |p: &Path| {
            p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("sql"))
        };
        let mut new: Vec<PathBuf> = paths
            .iter()
            .filter(|p| console_sql(p) && !known.contains(p) && p.is_file())
            .cloned()
            .collect();
        let mut pairs = Vec::new();
        let mut deleted = Vec::new();
        if gone.len() == 1 && new.len() == 1 {
            pairs.push((gone[0].clone(), new.remove(0)));
        } else {
            // In each folder: one gone and one new there is a rename; anything else is told
            // apart only as deletions (the new files join their folder's list by themselves).
            for old in &gone {
                let here = |p: &&PathBuf| p.parent() == old.parent();
                let new_here: Vec<&PathBuf> = new.iter().filter(here).collect();
                match (gone.iter().filter(here).count(), new_here.as_slice()) {
                    (1, [to]) => pairs.push((old.clone(), (*to).clone())),
                    _ => deleted.push(old.clone()),
                }
            }
        }
        for (from, to) in pairs {
            tracing::info!(from = %from.display(), to = %to.display(), "a console's file renamed on disk");
            self.events.publish(&workspace::events::FileRenamed { from, to });
        }
        if !deleted.is_empty() {
            self.events.publish(&workspace::events::EntriesDeleted { paths: deleted });
        }
    }

    /// The disk's changes to the attached files (see [`Self::disk_changed`]).
    fn files_disk_changed(&self, paths: &[PathBuf], sql: &dyn Fn(&Path) -> bool) {
        let known: Vec<PathBuf> = {
            let files = self.loaded_files();
            files.iter().flatten().map(|s| s.path(&self.root)).collect()
        };
        if known.is_empty() {
            return;
        }
        let gone: Vec<&PathBuf> = known
            .iter()
            .filter(|k| paths.iter().any(|p| k.starts_with(p)) && !k.exists())
            .collect();
        for old in &gone {
            let here = |p: &&PathBuf| p.parent() == old.parent();
            let new_here: Vec<&PathBuf> = paths
                .iter()
                .filter(|p| sql(p) && !known.contains(p) && p.is_file())
                .filter(here)
                .collect();
            let gone_here = gone.iter().filter(|p| p.parent() == old.parent()).count();
            if let (1, [to]) = (gone_here, new_here.as_slice()) {
                tracing::info!(from = %old.display(), to = %to.display(), "an attached SQL file renamed on disk");
                self.moved(old, to);
            }
        }
    }

    /// Console `id`'s runner forgotten (its node leaves the Services window): its
    /// transaction rolled back (said), its connection closed.
    fn retire(&self, id: ConsoleId) {
        let runner = {
            let mut runners = self.runners.lock().unwrap_or_else(PoisonError::into_inner);
            let at = runners.iter().position(|r| r.console() == id);
            at.map(|at| runners.remove(at))
        };
        let Some(runner) = runner else { return };
        if runner.in_transaction() {
            self.notify(workbench_model::Notification::warning(i18n::tr!(
                "console.rolled_back_on_move",
                name = runner.name()
            )));
            runner.abandon();
        } else {
            runner.disconnect();
        }
    }

    /// Writes `list` in the background (the newest change written last), unless the file
    /// couldn't be read (then it's left as it is).
    fn persist(&self, list: Vec<ConsoleSpec>) {
        self.list.persist(&self.runtime(), list, "consoles'");
    }

    /// Console `id`'s runner (made the first time).
    pub fn runner(&self, id: ConsoleId, source: DataSourceId) -> Runner {
        let mut runners = self.runners.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = runners.iter().find(|r| r.console() == id) {
            return found.clone();
        }
        let runner = Runner::new(id, source, self.runtime());
        runners.push(runner.clone());
        drop(runners);
        runner.set_context(self.context_of(id));
        runner
    }

    /// Where console `id`'s runner tells what it did: the project's bus, its file, the user.
    fn context_of(&self, id: ConsoleId) -> crate::runner::RunnerContext {
        let spec = self.spec(id);
        let file = spec.as_ref().map(|s| s.path(&self.root));
        let relative =
            file.as_ref().and_then(|f| f.strip_prefix(&self.root).ok()).map(Path::to_path_buf);
        crate::runner::RunnerContext {
            project: Some(self.project),
            events: Some(self.events.clone()),
            file,
            relative,
            name: spec.map(|s| s.name).unwrap_or_default(),
            notifier: Arc::clone(&self.notifier.lock().unwrap_or_else(PoisonError::into_inner)),
            history: Some(Arc::clone(&self.history)),
        }
    }

    /// Console `id`'s runner, if one was made.
    /// Console `id`'s tab may close without asking, once.
    pub fn allow_close(&self, id: ConsoleId) {
        *self.closing.lock().unwrap_or_else(PoisonError::into_inner) = Some(id);
    }

    /// Console `id`'s tab was allowed to close without asking (taken once).
    pub fn take_allowed_close(&self, id: ConsoleId) -> bool {
        let mut closing = self.closing.lock().unwrap_or_else(PoisonError::into_inner);
        let allowed = *closing == Some(id);
        if allowed {
            closing.take();
        }
        allowed
    }

    pub fn runner_of(&self, id: ConsoleId) -> Option<Runner> {
        self.runners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|r| r.console() == id)
            .cloned()
    }

    pub fn runners(&self) -> Vec<Runner> {
        self.runners.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Connections open now.
    pub fn connected(&self) -> usize {
        self.runners().iter().filter(|r| r.connected()).count()
    }

    pub fn busy(&self) -> bool {
        self.runners().iter().any(Runner::busy) || self.indexes.busy() || self.checks.busy()
    }

    /// Closes the connections unused for their idle time at `now` (how many), and tells
    /// the transactions open for too long (once each).
    pub fn sweep(&self, now: Instant) -> usize {
        for runner in self.runners() {
            runner.tell_long_transaction(now);
        }
        let due: Vec<Runner> = self.runners().into_iter().filter(|r| r.idle_at(now)).collect();
        for runner in &due {
            runner.disconnect();
        }
        // The schemas of sources with no console open, unused for a while.
        self.indexes.release_idle(now, self.index_idle(), |s| self.open_on(s));
        due.len()
    }

    /// When the next connection is due to close (`None`: none will).
    pub fn next_due(&self) -> Option<Instant> {
        let runners = self.runners();
        let idle = runners.iter().filter_map(Runner::idle_due);
        let indexes = self.indexes.next_due(self.index_idle(), |s| self.open_on(s));
        idle.chain(runners.iter().filter_map(Runner::long_due)).chain(indexes).min()
    }

    /// The project closes: every connection closed, a transaction left open rolled back
    /// (said).
    pub fn close_all(&self) {
        for runner in self.runners() {
            if runner.in_transaction() {
                let name = self.spec(runner.console()).map(|s| s.name).unwrap_or_default();
                self.notify(workbench_model::Notification::warning(i18n::tr!(
                    "console.rolled_back_on_close",
                    name = name
                )));
                runner.abandon();
            } else {
                runner.disconnect();
            }
        }
    }

    /// Clears what can be made again (their Output, their lists).
    pub fn purge(&self) {
        for runner in self.runners() {
            runner.purge();
        }
        self.history.purge();
        self.indexes.purge();
        self.checks.purge(self);
    }
}

/// Why a console wasn't renamed or deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameError {
    /// It's no longer a console of the project.
    Gone,
    Name(database::console::NameProblem),
    Io(String),
}

impl RenameError {
    /// In the user's words.
    pub fn text(&self) -> String {
        use database::console::NameProblem;
        match self {
            RenameError::Gone => i18n::t("console.gone"),
            RenameError::Name(NameProblem::Empty) => i18n::t("console.name_empty"),
            RenameError::Name(NameProblem::Invalid(c)) => {
                let c = if c.is_control() { format!("U+{:04X}", *c as u32) } else { c.to_string() };
                i18n::tr!("console.name_invalid", char = c)
            }
            RenameError::Name(NameProblem::Reserved) => i18n::t("console.name_reserved"),
            RenameError::Name(NameProblem::TooLong) => {
                i18n::tr!("console.name_too_long", max = database::console::NAME_MAX)
            }
            RenameError::Name(NameProblem::Taken) => i18n::t("console.name_taken"),
            RenameError::Io(why) => i18n::tr!("console.rename_failed", why = why.as_str()),
        }
    }
}

/// `console_10` after `console_2`: names compared with their numbers as numbers.
fn natural(name: &str) -> (String, u64) {
    match name.rsplit_once('_') {
        Some((stem, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
            (stem.to_lowercase(), n.parse().unwrap_or(0) + 1)
        }
        _ => (name.to_lowercase(), 0),
    }
}

/// The open projects' consoles.
static PROJECTS: Mutex<Vec<Arc<Consoles>>> = Mutex::new(Vec::new());

fn projects() -> MutexGuard<'static, Vec<Arc<Consoles>>> {
    PROJECTS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Every open project's consoles.
pub fn all() -> Vec<Arc<Consoles>> {
    projects().clone()
}

/// Project `id`'s consoles, while it's open.
pub fn consoles(id: ProjectId) -> Option<Arc<Consoles>> {
    projects().iter().find(|c| c.project == id).cloned()
}

/// Keeps `consoles` for its project until the guard returned is dropped (the project
/// closes): then they're forgotten, every connection closed.
pub fn register(consoles: Arc<Consoles>) -> Subscription {
    let id = consoles.project;
    {
        let mut all = projects();
        all.retain(|c| c.project != id);
        all.push(consoles);
    }
    Subscription::on_end(move || {
        let gone: Vec<Arc<Consoles>> = {
            let mut all = projects();
            let (gone, kept) = std::mem::take(&mut *all).into_iter().partition(|c| c.project == id);
            *all = kept;
            gone
        };
        for consoles in gone {
            consoles.close_all();
        }
        // Its queries' operations go with it.
        crate::runner::QUERIES.forget(id);
    })
}

/// Gives each project its (empty) consoles, closed with it.
#[derive(Debug, Default)]
pub struct ConsolesService;

impl ProjectService for ConsolesService {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        let consoles =
            Consoles::new(project.id, project.root, project.events.clone(), Runtime::pool());
        consoles.set_store(Arc::new(storage::TomlConsoles::new(project.root)));
        consoles.set_files_store(Arc::new(storage::TomlConsoles::sql_files(project.root)));
        consoles.history().set_store(Arc::new(storage::TomlQueryHistory::new(project.root)));
        consoles.set_notifier(Arc::clone(&project.notifier));
        consoles.indexes().set_wake(Arc::clone(&project.wake));
        let mut subscriptions = consoles.follow();
        subscriptions.push(register(consoles));
        subscriptions
    }

    fn busy(&self, id: ProjectId) -> bool {
        consoles(id).is_some_and(|c| c.busy())
    }

    /// The consoles' files are the user's: another program renaming one is followed.
    fn own_dirs(&self) -> &'static [&'static str] {
        &[database::console::DIR]
    }
}

/// Whether the idle watch is on (its job waiting on its pool).
static WATCHING: Mutex<bool> = Mutex::new(false);

/// Starts the idle watch, unless it's on: one job on the `db-console-idle` pool (one
/// thread), sleeping until the next connection is due, closing those due, ending once none
/// will be.
pub fn watch() {
    {
        let mut on = WATCHING.lock().unwrap_or_else(PoisonError::into_inner);
        if std::mem::replace(&mut *on, true) {
            return;
        }
    }
    static POOL: OnceLock<kernel::Pool> = OnceLock::new();
    let pool = POOL.get_or_init(|| kernel::Pool::new("db-console-idle", 1, kernel::work::IDLE));
    pool.spawn(kernel::Priority::Background, None, || {
        loop {
            let next = {
                let mut on = WATCHING.lock().unwrap_or_else(PoisonError::into_inner);
                let next = all().iter().filter_map(|c| c.next_due()).min();
                if next.is_none() {
                    *on = false;
                    return;
                }
                next
            };
            // At most a little while at a time: a sooner one may come meanwhile (a connection
            // left idle while an old transaction's hour runs).
            if let Some(at) = next {
                let wait = at.saturating_duration_since(Instant::now());
                std::thread::sleep(wait.min(std::time::Duration::from_secs(30)));
            }
            for consoles in all() {
                consoles.sweep(Instant::now());
            }
        }
    });
}

#[cfg(test)]
#[path = "../tests/unit/consoles.rs"]
mod tests;
