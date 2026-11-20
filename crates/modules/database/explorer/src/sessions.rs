//! The open sources of a project: one [`SessionWorker`] each, made when a source is first
//! opened (nothing before: an empty table per project), kept by the project's identity in
//! this module's table — registered when the project opens ([`SessionsService`]) and gone
//! with it, every session closed (the [`Subscription::on_end`] guard among the project's
//! subscriptions). Another project's sources are never seen here.
//!
//! An open session unused for the `[database] idle_close` minutes is closed
//! ([`Sessions::sweep`]); its worker stays, and its next request opens it again. The app's
//! watch over them is one job on its own one-thread pool (`db-idle`), only while some
//! session is open, waiting until the next one is due.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use database::identity::FileKey;
use database::layouts::{GridLayout, LayoutKey, LayoutStore};
use database::{DataSource, DataSourceId, DbError, Driver, Secret};
use kernel::{EventBus, ProjectId, Subscription};
use workbench_model::{ProjectContext, ProjectService};

use crate::catalog::Catalog;
use crate::credentials::Credentials;
use crate::runtime::Runtime;
use crate::worker::{OnConnect, SessionWorker, Source};

/// A project's open sources (see the module's doc).
pub struct Sessions {
    project: ProjectId,
    root: PathBuf,
    events: EventBus,
    runtime: Runtime,
    /// Called when a session opens (the app's idle watch); none in the tests.
    on_connect: Option<OnConnect>,
    workers: Mutex<Vec<SessionWorker>>,
    /// The files opened by themselves (a SQLite file, a CSV shown as data), by which file
    /// they are (a link in the path never makes two): each its own source (a new identity
    /// per file and project), and the views using it.
    files: Mutex<HashMap<FileKey, OpenFile>>,
    /// Where each file-based source's sessions open its file: the path it had, then where
    /// a rename or move took it (a session opening again reaches it there).
    file_targets: Arc<Mutex<HashMap<DataSourceId, PathBuf>>>,
    /// The grids' layouts, by file and table: those of the project's files also written
    /// to its folder (beyond its session), the others kept while it's open.
    layouts: Arc<Mutex<Layouts>>,
    /// Its exports, for the Task Manager.
    activities: Arc<workbench_model::Activities>,
    /// The data sources configured (the project's and the global ones).
    catalog: Catalog,
    /// Finds a source's driver by id (the registered ones; the tests give theirs).
    drivers: Mutex<Drivers>,
    /// Where the sources' passwords are kept (memory only until the app gives the system's).
    credentials: Mutex<Arc<Credentials>>,
    /// Tells the user what happened (the project's notifications; none in the tests unless
    /// they give theirs).
    notifier: Mutex<Arc<dyn workbench_model::Notifier>>,
    /// The "Database" window's selection.
    selected: Mutex<Option<Selected>>,
    /// The consoles with a transaction open, and on which source (heard on the project's
    /// bus: `TransactionChanged`): the tree marks them, a write refused explains itself.
    console_tx: Arc<Mutex<HashMap<database::console::ConsoleId, DataSourceId>>>,
    _console_tx: Mutex<Option<Subscription>>,
}

/// What the "Database" window has selected: a source, its schema, its table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub source: DataSourceId,
    pub schema: Option<String>,
    pub table: Option<database::TableRef>,
}

/// A file shown by itself: its path as first opened, its source's id, the views using it,
/// its driver's id.
#[derive(Debug, Clone)]
struct OpenFile {
    path: PathBuf,
    id: DataSourceId,
    users: usize,
    driver: String,
}

/// Finds a driver by its id.
pub type Drivers = Arc<dyn Fn(&str) -> Option<Arc<dyn Driver>> + Send + Sync>;

/// The drivers registered by the modules ([`database::drivers`]).
fn registered() -> Drivers {
    Arc::new(|id| database::drivers::find(id).map(|f| (f.create)()))
}

/// The grids' layouts (see [`Sessions::keep_layout`]).
#[derive(Default)]
struct Layouts {
    /// The project's files' layouts, newest last (as the store has them).
    kept: Vec<(LayoutKey, GridLayout)>,
    /// Files outside the project: only while it's open.
    outside: HashMap<(PathBuf, database::TableRef), GridLayout>,
    /// Where they're written (none: nothing written, a file outside any project).
    store: Option<Arc<dyn LayoutStore>>,
    /// How long changes wait for the next (a column dragged wider: written once).
    quiet: Duration,
    /// Bumped by each change; a write job is waiting or writing.
    version: u64,
    writing: bool,
}

impl std::fmt::Debug for Sessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sessions").field("project", &self.project).finish_non_exhaustive()
    }
}

impl Sessions {
    pub fn new(
        project: ProjectId,
        root: &Path,
        events: EventBus,
        runtime: Runtime,
        on_connect: Option<OnConnect>,
    ) -> Arc<Self> {
        let catalog = Catalog::new(Arc::new(database::NoStore), runtime.clone(), events.clone());
        Arc::new(Self {
            project,
            root: root.to_path_buf(),
            events,
            runtime,
            on_connect,
            workers: Mutex::default(),
            files: Mutex::default(),
            file_targets: Arc::default(),
            layouts: Arc::default(),
            activities: Arc::new(workbench_model::Activities::new(Instant::now)),
            catalog,
            drivers: Mutex::new(registered()),
            credentials: Mutex::new(Arc::new(Credentials::default())),
            notifier: Mutex::new(Arc::new(workbench_model::NoNotifier)),
            selected: Mutex::default(),
            console_tx: Arc::default(),
            _console_tx: Mutex::default(),
        })
        .with_transactions()
    }

    /// The consoles' transactions followed from the bus (opened, ended).
    fn with_transactions(self: Arc<Self>) -> Arc<Self> {
        let open = Arc::clone(&self.console_tx);
        let subscription =
            self.events.subscribe(move |e: &database::events::TransactionChanged| {
                let mut open = open.lock().unwrap_or_else(PoisonError::into_inner);
                if e.open {
                    open.insert(e.console, e.source);
                } else {
                    open.remove(&e.console);
                }
            });
        *self._console_tx.lock().unwrap_or_else(PoisonError::into_inner) = Some(subscription);
        self
    }

    /// A console of source `id` has a transaction open (it may hold locks on it).
    /// (A console on another database of its server too: it may hold locks there.)
    pub fn console_transaction(&self, id: DataSourceId) -> bool {
        let id = self.catalog.base_id(id);
        let open = self.console_tx.lock().unwrap_or_else(PoisonError::into_inner);
        open.values().any(|s| self.catalog.base_id(*s) == id)
    }

    /// Why a write on source `id` was refused, in words: a console's transaction holding
    /// it when one is open, else the engine's.
    pub fn refusal(&self, id: Option<DataSourceId>, e: &DbError) -> String {
        match e {
            DbError::InUse(_) if id.is_some_and(|id| self.console_transaction(id)) => {
                i18n::t("database.console_tx_in_use")
            }
            other => crate::grid_source::message(other),
        }
    }

    /// The user is told through `notifier` from now on (the project's).
    pub fn set_notifier(&self, notifier: Arc<dyn workbench_model::Notifier>) {
        *self.notifier.lock().unwrap_or_else(PoisonError::into_inner) = notifier;
    }

    /// Tells the user `notification` (a toast of the project's): what the "Database"
    /// window, the data sources' modal and the data views say once they did something.
    pub fn notify(&self, notification: workbench_model::Notification) {
        let notifier = Arc::clone(&self.notifier.lock().unwrap_or_else(PoisonError::into_inner));
        notifier.show(notification);
    }

    /// The sources' passwords.
    pub fn credentials(&self) -> Arc<Credentials> {
        Arc::clone(&self.credentials.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The passwords are kept by `credentials` from now on (the app's: the system's).
    pub fn set_credentials(&self, credentials: Arc<Credentials>) {
        *self.credentials.lock().unwrap_or_else(PoisonError::into_inner) = credentials;
    }

    /// The data sources configured.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Which file a path names (the catalog's: sources and opened files matched alike).
    pub fn identities(&self) -> &crate::identity::FileIdentities {
        self.catalog.identities()
    }

    /// The drivers are found by `drivers` (the tests' own).
    pub fn set_drivers(&self, drivers: Drivers) {
        *self.drivers.lock().unwrap_or_else(PoisonError::into_inner) = drivers;
    }

    /// The driver of id `id`, made (nothing runs until it connects).
    pub fn driver(&self, id: &str) -> Option<Arc<dyn Driver>> {
        let drivers = self.drivers.lock().unwrap_or_else(PoisonError::into_inner).clone();
        drivers(id)
    }

    pub fn project(&self) -> ProjectId {
        self.project
    }

    /// The project's root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Its exports, running and the last ended (the Task Manager's operations).
    pub fn activities(&self) -> &workbench_model::Activities {
        &self.activities
    }

    /// Where its jobs run.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<SessionWorker>> {
        self.workers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `source`'s worker: the one already open, or a new one (its session opened by its
    /// first request, with `driver` and `secret`; a file-based source's file found from
    /// the project's root).
    pub fn open(
        &self,
        source: &DataSource,
        driver: Arc<dyn Driver>,
        secret: Secret,
    ) -> SessionWorker {
        self.open_with(source, driver, Arc::new(move || secret.clone()))
    }

    /// `source`'s worker, its password read by `secret` each time the session opens (on
    /// the pool's thread: a keychain may make it wait). Its startup script, if any, runs
    /// right after.
    pub fn open_with(
        &self,
        source: &DataSource,
        driver: Arc<dyn Driver>,
        secret: Arc<dyn Fn() -> Secret + Send + Sync>,
    ) -> SessionWorker {
        let dialect = driver.dialect();
        let capabilities = driver.capabilities();
        let connect = self.connect_fn(source, driver, secret, None);
        self.attach(Source {
            id: source.id,
            connect,
            dialect,
            capabilities,
            read_only: source.read_only,
        })
    }

    /// What opens a session of `source` (on the calling thread: a background one): its
    /// password read by `secret` then, its startup script run right after; a file-based
    /// source's file found from the project's root, a read-only one opened so by the driver
    /// too, `database` (another one of the same server) instead of the source's.
    pub fn connect_fn(
        &self,
        source: &DataSource,
        driver: Arc<dyn Driver>,
        secret: Arc<dyn Fn() -> Secret + Send + Sync>,
        database: Option<&str>,
    ) -> crate::worker::Connect {
        let mut target = source.target.clone();
        let file = source.file_in(&self.root);
        if let Some(file) = &file {
            target.database = file.to_string_lossy().into_owned();
            self.file_targets
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(source.id, file.clone());
        } else if let Some(database) = database {
            target.database = database.to_owned();
        }
        let (targets, id, follows) = (Arc::clone(&self.file_targets), source.id, file.is_some());
        // A read-only source is opened so by the driver too (when it can: a file).
        if source.read_only {
            target.options.insert(database::Target::READ_ONLY.to_owned(), "true".to_owned());
        }
        Arc::new(move || {
            let secret = secret();
            // A file renamed or moved since: opened where it is now.
            let mut target = target.clone();
            if follows
                && let Some(now) = targets.lock().unwrap_or_else(PoisonError::into_inner).get(&id)
            {
                target.database = now.to_string_lossy().into_owned();
            }
            let mut session = driver.connect(&target, &secret)?;
            if let Some(script) = target.options.get(database::Target::STARTUP) {
                run_startup(session.as_mut(), script)?;
            }
            Ok(session)
        })
    }

    /// The file a file-based source `id` opens (as found when its worker was made).
    pub fn file_of(&self, id: DataSourceId) -> Option<PathBuf> {
        self.file_targets.lock().unwrap_or_else(PoisonError::into_inner).get(&id).cloned()
    }

    /// The function reading configured source `source`'s password, as it's kept (none
    /// asked for a driver without one).
    pub fn secret_fn(
        &self,
        source: &DataSource,
        driver: &dyn Driver,
    ) -> Arc<dyn Fn() -> Secret + Send + Sync> {
        let credentials = self.credentials();
        let (needs, keyed) = (driver.connection().has_secret(), source.clone());
        Arc::new(move || {
            if needs { credentials.get(&keyed).unwrap_or_default() } else { Secret::default() }
        })
    }

    /// What the "Database" window has selected (its source, schema, table), for the
    /// commands run from the palette.
    pub fn selected(&self) -> Option<Selected> {
        self.selected.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_selected(&self, selected: Option<Selected>) {
        *self.selected.lock().unwrap_or_else(PoisonError::into_inner) = selected;
    }

    /// `source`'s worker, its driver found among those registered
    /// ([`database::drivers`]: the modules' contributions); an error if none has its id.
    pub fn open_source(
        &self,
        source: &DataSource,
        secret: Secret,
    ) -> Result<SessionWorker, database::DbError> {
        let driver = self.driver(&source.driver).ok_or_else(|| {
            database::DbError::Unsupported(i18n::tr!(
                "database.why_driver",
                driver = source.driver.as_str()
            ))
        })?;
        Ok(self.open(source, driver, secret))
    }

    /// Configured source `id`'s worker (the one open, or a new one: nothing connects until
    /// it's asked something); an error if no source has that id or its driver is gone.
    /// Its grids' layouts follow its structure's changes.
    pub fn source_worker(
        self: &Arc<Self>,
        id: DataSourceId,
    ) -> Result<SessionWorker, database::DbError> {
        if let Some(worker) = self.get(id) {
            return Ok(worker);
        }
        // A database of a source's server: the source's session, on that database.
        let database = self.catalog.database_of(id);
        let base = database.as_ref().map_or(id, |(source, _)| *source);
        let source = self.catalog.get(base).ok_or_else(|| {
            database::DbError::Unsupported(i18n::t("database.why_source_removed"))
        })?;
        // A file shown by itself too goes through its own source's worker (its id).
        if let Some(file) = source.file_in(&self.root) {
            let key = self.identities().key(&file);
            let known =
                self.files.lock().unwrap_or_else(PoisonError::into_inner).get(&key).map(|f| f.id);
            if let Some(worker) = known.and_then(|id| self.get(id)) {
                return Ok(worker);
            }
        }
        let driver = self.driver(&source.driver).ok_or_else(|| {
            database::DbError::Unsupported(i18n::tr!(
                "database.why_driver",
                driver = source.driver.as_str()
            ))
        })?;
        // The password read when the session opens (in the background), as kept.
        let secret = self.secret_fn(&source, driver.as_ref());
        let worker = match &database {
            None => self.open_with(&source, driver, secret),
            Some((_, name)) => {
                let (dialect, capabilities) = (driver.dialect(), driver.capabilities());
                let connect = self.connect_fn(&source, driver, secret, Some(name));
                self.attach(Source {
                    id,
                    connect,
                    dialect,
                    capabilities,
                    read_only: source.read_only,
                })
            }
        };
        let sessions = Arc::downgrade(self);
        let file = source.file_in(&self.root);
        worker.on_altered(Arc::new(move |changes| {
            if let Some(sessions) = sessions.upgrade() {
                match &file {
                    Some(file) => sessions.follow_layouts(file, changes),
                    None => sessions.follow_source_layouts(id, changes),
                }
            }
        }));
        Ok(worker)
    }

    /// The files shown by themselves now (a SQLite file, a CSV opened from the project):
    /// their path, their source's id and their driver's, by path.
    pub fn open_files(&self) -> Vec<(PathBuf, DataSourceId, String)> {
        let files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out: Vec<(PathBuf, DataSourceId, String)> =
            files.values().map(|f| (f.path.clone(), f.id, f.driver.clone())).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// File `path`'s worker, opened by itself with `driver` (a SQLite file, a CSV), its
    /// target's `options` given (a CSV's sample): one per file, shared by the views showing
    /// it, closed when the last [`FileLease`] goes. An error if no driver has that id.
    pub fn open_file(
        self: &Arc<Self>,
        path: &Path,
        driver: &str,
        options: &[(&str, String)],
    ) -> Result<(SessionWorker, FileLease), database::DbError> {
        // A file configured as a source is that source (its id, its read-only).
        let configured = self.catalog.source_of_file_with(&self.root, path, |s| s.driver == driver);
        let key = self.identities().key(path);
        let (id, first) = {
            let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
            let fresh = configured.as_ref().map_or_else(DataSourceId::new, |s| s.id);
            let entry = files.entry(key).or_insert_with(|| OpenFile {
                path: path.to_path_buf(),
                id: fresh,
                users: 0,
                driver: driver.to_owned(),
            });
            entry.users += 1;
            (entry.id, entry.users == 1)
        };
        let lease = FileLease { sessions: Arc::downgrade(self), id };
        if first {
            // The "Database" window lists the files shown by themselves.
            self.events
                .publish(&database::events::DataSourcesChanged { scope: database::Scope::Project });
        }
        if let Some(worker) = self.get(id) {
            return Ok((worker, lease));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut source = DataSource::new(name, driver, database::Scope::Project);
        source.id = id;
        source.file = Some(path.to_path_buf());
        source.secrets = database::SecretPolicy::Never;
        source.read_only = configured.is_some_and(|c| c.read_only);
        for (key, value) in options {
            source.target.options.insert((*key).to_owned(), value.clone());
        }
        let worker = self.open_source(&source, Secret::new(""))?;
        // The file's grids' layouts follow its structure's changes (before they're said).
        let (sessions, file) = (Arc::downgrade(self), path.to_path_buf());
        worker.on_altered(Arc::new(move |changes| {
            if let Some(sessions) = sessions.upgrade() {
                sessions.follow_layouts(&file, changes);
            }
        }));
        Ok((worker, lease))
    }

    /// The project's bus (what a commit changed is said there; the disk's changes too).
    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// The layouts are kept in `store` too (the project's folder), each change written
    /// once `quiet` passed without another; those it had are read now.
    pub fn set_layout_store(&self, store: Arc<dyn LayoutStore>, quiet: Duration) {
        let kept = store.load();
        let mut layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        layouts.kept = kept;
        layouts.store = Some(store);
        layouts.quiet = quiet;
    }

    /// Where file `path`'s layouts are known by (`None`: outside the project).
    fn layout_key(&self, path: &Path, table: &database::TableRef) -> Option<LayoutKey> {
        let source = database::layouts::file_source(&self.root, path)?;
        Some(LayoutKey { source, table: table.clone() })
    }

    /// The layout kept for `table` of file `path`.
    pub fn layout(&self, path: &Path, table: &database::TableRef) -> Option<GridLayout> {
        let layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        if layouts.store.is_some()
            && let Some(key) = self.layout_key(path, table)
        {
            return layouts.kept.iter().rev().find(|(k, _)| *k == key).map(|(_, l)| l.clone());
        }
        layouts.outside.get(&(path.to_path_buf(), table.clone())).cloned()
    }

    /// Keeps `layout` for `table` of file `path`: a project's file's written to its folder
    /// in the background (once changes stop for a moment; unchanged: nothing), another's
    /// kept while the project is open.
    pub fn keep_layout(&self, path: &Path, table: &database::TableRef, layout: GridLayout) {
        let mut layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        let key = self.layout_key(path, table).filter(|_| layouts.store.is_some());
        let Some(key) = key else {
            layouts.outside.insert((path.to_path_buf(), table.clone()), layout);
            return;
        };
        if layouts.kept.last().is_some_and(|(k, l)| *k == key && *l == layout) {
            return;
        }
        database::layouts::keep(&mut layouts.kept, key, layout);
        layouts.version += 1;
        if std::mem::replace(&mut layouts.writing, true) {
            return;
        }
        let shared = Arc::clone(&self.layouts);
        let quiet = layouts.quiet;
        self.runtime.spawn(move || write_layouts(&shared, quiet));
    }

    /// The layouts of file `path`'s tables following its structure's `changes` (a column
    /// or a table renamed keeps its layout; a dropped one loses it), written as any change.
    pub fn follow_layouts(&self, path: &Path, changes: &[database::SchemaChange]) {
        let mut layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        // The file's own, outside the project: followed in place.
        let outside: Vec<(LayoutKey, GridLayout)> = layouts
            .outside
            .iter()
            .filter(|((p, _), _)| p == path)
            .map(|((_, t), l)| (LayoutKey { source: String::new(), table: t.clone() }, l.clone()))
            .collect();
        if !outside.is_empty() {
            let mut moved = outside;
            database::layouts::follow(&mut moved, "", changes);
            layouts.outside.retain(|(p, _), _| p != path);
            for (key, layout) in moved {
                layouts.outside.insert((path.to_path_buf(), key.table), layout);
            }
        }
        let Some(source) = database::layouts::file_source(&self.root, path) else { return };
        if layouts.store.is_none()
            || !database::layouts::follow(&mut layouts.kept, &source, changes)
        {
            return;
        }
        layouts.version += 1;
        if std::mem::replace(&mut layouts.writing, true) {
            return;
        }
        let shared = Arc::clone(&self.layouts);
        let quiet = layouts.quiet;
        self.runtime.spawn(move || write_layouts(&shared, quiet));
    }

    /// Where configured source `id`'s layouts are known by (its id).
    fn source_key(id: DataSourceId, table: &database::TableRef) -> LayoutKey {
        LayoutKey { source: id.to_string(), table: table.clone() }
    }

    /// The layout kept for `table` of configured source `id`.
    pub fn source_layout(
        &self,
        id: DataSourceId,
        table: &database::TableRef,
    ) -> Option<GridLayout> {
        let layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        let key = Self::source_key(id, table);
        if layouts.store.is_some() {
            return layouts.kept.iter().rev().find(|(k, _)| *k == key).map(|(_, l)| l.clone());
        }
        layouts.outside.get(&(PathBuf::from(key.source), table.clone())).cloned()
    }

    /// Keeps `layout` for `table` of configured source `id` (as a file's, by its id).
    pub fn keep_source_layout(
        &self,
        id: DataSourceId,
        table: &database::TableRef,
        layout: GridLayout,
    ) {
        let mut layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        let key = Self::source_key(id, table);
        if layouts.store.is_none() {
            layouts.outside.insert((PathBuf::from(key.source), table.clone()), layout);
            return;
        }
        if layouts.kept.last().is_some_and(|(k, l)| *k == key && *l == layout) {
            return;
        }
        database::layouts::keep(&mut layouts.kept, key, layout);
        self.write_layouts_later(layouts);
    }

    /// Configured source `id`'s layouts following its structure's `changes`.
    pub fn follow_source_layouts(&self, id: DataSourceId, changes: &[database::SchemaChange]) {
        let mut layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        if layouts.store.is_none()
            || !database::layouts::follow(&mut layouts.kept, &id.to_string(), changes)
        {
            return;
        }
        self.write_layouts_later(layouts);
    }

    /// A change of the kept layouts written in the background (once they stop changing).
    fn write_layouts_later(&self, mut layouts: std::sync::MutexGuard<'_, Layouts>) {
        layouts.version += 1;
        if std::mem::replace(&mut layouts.writing, true) {
            return;
        }
        let shared = Arc::clone(&self.layouts);
        let quiet = layouts.quiet;
        drop(layouts);
        self.runtime.spawn(move || write_layouts(&shared, quiet));
    }

    /// `from` (a file, or a folder holding some) was renamed or moved to `to`: what's kept
    /// of its files follows — the sessions open on them are closed, to open again at the new
    /// path with the next request, a file shown by itself is known by its new path, a
    /// configured source's file is changed (its name too, when it was the file's), the
    /// grids' layouts are kept under the new path.
    pub fn file_moved(&self, from: &Path, to: &Path) {
        use workspace::clipboard::moved_path;
        let reopened: Vec<DataSourceId> = {
            let mut targets = self.file_targets.lock().unwrap_or_else(PoisonError::into_inner);
            targets
                .iter_mut()
                .filter_map(|(id, path)| {
                    *path = moved_path(path, from, to)?;
                    Some(*id)
                })
                .collect()
        };
        // Their sessions open again at the new path at the next request (a CSV's reads its
        // file by its path).
        for id in reopened {
            if let Some(worker) = self.get(id) {
                worker.close_now();
            }
        }
        // Shown by themselves: known by their new path (their key worked out outside the lock).
        let moved: Vec<(FileKey, PathBuf)> = {
            let files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
            files
                .iter()
                .filter_map(|(k, f)| Some((k.clone(), moved_path(&f.path, from, to)?)))
                .collect()
        };
        let rekeyed: Vec<(FileKey, PathBuf, FileKey)> = moved
            .into_iter()
            .map(|(old, new)| (old, new.clone(), self.identities().key(&new)))
            .collect();
        let shown = !rekeyed.is_empty();
        if shown {
            let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
            for (old, path, key) in rekeyed {
                if let Some(mut entry) = files.remove(&old) {
                    entry.path = path;
                    files.insert(key, entry);
                }
            }
        }
        // Configured sources of the file.
        let name_of = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
        for source in self.catalog.all() {
            let Some(file) = source.file_in(&self.root) else { continue };
            let Some(new) = moved_path(&file, from, to) else { continue };
            let mut changed = source.clone();
            if name_of(&file).is_some_and(|n| n == source.name)
                && let Some(name) = name_of(&new)
            {
                changed.name = name;
            }
            changed.file = Some(database::source::stored_path(&new, &self.root));
            if let Err(why) = self.catalog.update(changed) {
                tracing::warn!(error = %why, "a data source's file didn't follow its rename");
            }
        }
        // The layouts kept by the file's path.
        let mut layouts = self.layouts.lock().unwrap_or_else(PoisonError::into_inner);
        let mut changed = false;
        for (key, _) in &mut layouts.kept {
            let Some(new) = moved_path(&self.root.join(&key.source), from, to) else { continue };
            if let Some(source) = database::layouts::file_source(&self.root, &new) {
                key.source = source;
                changed = true;
            }
        }
        let outside: Vec<(PathBuf, database::TableRef)> = layouts
            .outside
            .keys()
            .filter(|(p, _)| moved_path(p, from, to).is_some())
            .cloned()
            .collect();
        for (path, table) in outside {
            if let (Some(layout), Some(new)) = (
                layouts.outside.remove(&(path.clone(), table.clone())),
                moved_path(&path, from, to),
            ) {
                layouts.outside.insert((new, table), layout);
            }
        }
        if changed && layouts.store.is_some() {
            self.write_layouts_later(layouts);
        } else {
            drop(layouts);
        }
        // The "Database" window lists the files shown by themselves.
        if shown {
            self.events
                .publish(&database::events::DataSourcesChanged { scope: database::Scope::Project });
        }
    }

    /// The views using file `path`'s source (by any way to the file).
    pub fn file_users(&self, path: &Path) -> usize {
        let key = self.identities().key(path);
        self.files.lock().unwrap_or_else(PoisonError::into_inner).get(&key).map_or(0, |f| f.users)
    }

    /// A view stopped using the file of source `id`: its source closed with the last one.
    fn release_file(&self, id: DataSourceId) {
        let gone = {
            let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
            let key = files.iter().find(|(_, f)| f.id == id).map(|(k, _)| k.clone());
            match key.as_ref().and_then(|k| Some((k, files.get_mut(k)?))) {
                Some((_, entry)) if entry.users > 1 => {
                    entry.users -= 1;
                    None
                }
                Some((key, _)) => {
                    let key = key.clone();
                    files.remove(&key).map(|f| f.id)
                }
                None => None,
            }
        };
        if let Some(id) = gone {
            self.close(id);
            self.events
                .publish(&database::events::DataSourcesChanged { scope: database::Scope::Project });
        }
    }

    /// The worker of `source` (made from it if there's none yet).
    pub fn attach(&self, source: Source) -> SessionWorker {
        let mut workers = self.lock();
        if let Some(worker) = workers.iter().find(|w| w.source().id == source.id) {
            return worker.clone();
        }
        let name = self.catalog.get(source.id).map(|s| s.name).unwrap_or_default();
        let worker = SessionWorker::new(
            source,
            self.runtime.clone(),
            Some(self.events.clone()),
            self.on_connect.clone(),
        );
        // What the user waits on (a grid's page, its total, a commit) listed as the
        // project's operations, with the exports.
        worker.track(crate::worker::Tracking {
            activities: Arc::clone(&self.activities),
            project: self.project,
            source: name,
        });
        workers.push(worker.clone());
        worker
    }

    pub fn get(&self, id: DataSourceId) -> Option<SessionWorker> {
        self.lock().iter().find(|w| w.source().id == id).cloned()
    }

    /// The source is closed and forgotten (removed, or its settings changed), and so are
    /// its server's other databases.
    pub fn close(&self, id: DataSourceId) {
        let gone: Vec<SessionWorker> = {
            let mut workers = self.lock();
            let (gone, kept) = std::mem::take(&mut *workers).into_iter().partition(|w| {
                let of = w.source().id;
                of == id || self.catalog.database_of(of).is_some_and(|(base, _)| base == id)
            });
            *workers = kept;
            gone
        };
        for worker in gone {
            worker.close_now();
        }
    }

    /// Every session closed, every worker forgotten (the project closes).
    pub fn close_all(&self) {
        let workers = std::mem::take(&mut *self.lock());
        for worker in workers {
            worker.close_now();
        }
    }

    /// Closes the sessions unused for `idle` at `now`; how many.
    pub fn sweep(&self, now: Instant, idle: Duration) -> usize {
        let due: Vec<SessionWorker> = self
            .lock()
            .iter()
            .filter(|w| {
                w.idle_since().is_some_and(|since| now.saturating_duration_since(since) >= idle)
            })
            .cloned()
            .collect();
        for worker in &due {
            worker.close_now();
        }
        due.len()
    }

    /// When the next session is due to close (`None`: none is open). One at work is due
    /// `idle` from `now` at the earliest.
    pub fn next_close(&self, now: Instant, idle: Duration) -> Option<Instant> {
        self.lock()
            .iter()
            .filter(|w| w.connected() || w.busy())
            .map(|w| w.idle_since().unwrap_or(now) + idle)
            .min()
    }

    /// Sessions open now.
    pub fn connected(&self) -> usize {
        self.lock().iter().filter(|w| w.connected()).count()
    }

    /// Sources with a worker.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    pub fn busy(&self) -> bool {
        self.lock().iter().any(SessionWorker::busy)
    }
}

/// Runs a source's startup `script` (its statements, one per line or `;`) on a session
/// just opened; one failing fails the connection (said, as the engine said it).
fn run_startup(session: &mut dyn database::Session, script: &str) -> Result<(), database::DbError> {
    let never = database::Cancel::default();
    for statement in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        session.execute(statement, &mut database::Collect::default(), &never)?;
    }
    Ok(())
}

/// Writes the layouts once they stop changing for `quiet` (a write job: one at a time,
/// writing what's newest when it wakes, again while changes came during a write).
fn write_layouts(shared: &Mutex<Layouts>, quiet: Duration) {
    loop {
        let seen = shared.lock().unwrap_or_else(PoisonError::into_inner).version;
        if !quiet.is_zero() {
            std::thread::sleep(quiet);
        }
        let (version, kept, store) = {
            let mut layouts = shared.lock().unwrap_or_else(PoisonError::into_inner);
            // Changed while it waited: wait again.
            if layouts.version != seen {
                continue;
            }
            let Some(store) = layouts.store.clone() else {
                layouts.writing = false;
                return;
            };
            (layouts.version, layouts.kept.clone(), store)
        };
        if let Err(e) = store.save(&kept) {
            tracing::warn!(error = %e, "the grids' layouts weren't written");
        }
        let mut layouts = shared.lock().unwrap_or_else(PoisonError::into_inner);
        if layouts.version == version {
            layouts.writing = false;
            return;
        }
    }
}

/// A view's use of a file's source ([`Sessions::open_file`]): the source closes when the
/// last one is dropped (the views showing the file closed).
#[derive(Debug)]
pub struct FileLease {
    sessions: Weak<Sessions>,
    /// The file's source (whatever path opened it, wherever the file moved since).
    id: DataSourceId,
}

impl Drop for FileLease {
    fn drop(&mut self) {
        if let Some(sessions) = self.sessions.upgrade() {
            sessions.release_file(self.id);
        }
    }
}

/// The open projects' sources, by the project's identity.
static PROJECTS: Mutex<Vec<Arc<Sessions>>> = Mutex::new(Vec::new());

fn projects() -> std::sync::MutexGuard<'static, Vec<Arc<Sessions>>> {
    PROJECTS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Every open project's sources.
pub fn all() -> Vec<Arc<Sessions>> {
    projects().clone()
}

/// Project `id`'s sources, while it's open.
pub fn sessions(id: ProjectId) -> Option<Arc<Sessions>> {
    projects().iter().find(|s| s.project == id).cloned()
}

/// Keeps `sessions` for its project until the guard returned is dropped (the project
/// closes): then they're forgotten and every session is closed.
pub fn register(sessions: Arc<Sessions>) -> Subscription {
    let id = sessions.project;
    // Its changes of the global list reach the other projects (each says it on its bus).
    sessions.catalog().on_global(Arc::new(move |list| {
        for other in all().into_iter().filter(|s| s.project != id) {
            other.catalog().take_global(list);
        }
    }));
    {
        let mut all = projects();
        all.retain(|s| s.project != id);
        all.push(sessions);
    }
    Subscription::on_end(move || {
        let gone: Vec<Arc<Sessions>> = {
            let mut all = projects();
            let (gone, kept) = std::mem::take(&mut *all).into_iter().partition(|s| s.project == id);
            *all = kept;
            gone
        };
        for sessions in gone {
            sessions.close_all();
        }
    })
}

/// How long the grids' layout changes wait for the next before they're written.
pub const LAYOUT_QUIET: Duration = Duration::from_millis(600);

/// Gives each project its (empty) table of sources, closed with it.
#[derive(Debug, Default)]
pub struct SessionsService;

impl ProjectService for SessionsService {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        let watch: OnConnect = Arc::new(watch);
        let sessions = Sessions::new(
            project.id,
            project.root,
            project.events.clone(),
            Runtime::pool(),
            Some(watch),
        );
        // The grids' layouts, in the project's folder.
        sessions
            .set_layout_store(Arc::new(storage::TomlGridLayouts::new(project.root)), LAYOUT_QUIET);
        // The data sources: the project's, and every project's in the config folder (read
        // when the "Database" window first asks).
        sessions.catalog().set_store(Arc::new(storage::TomlDataSources::new(project.root)));
        // Their passwords: the project's vaults and the keychain (reached only when a
        // source asks, in the background).
        sessions.set_credentials(Arc::new(Credentials::system(project.root)));
        // What the window and the views say, as the project's notifications.
        sessions.set_notifier(Arc::clone(&project.notifier));
        // The query consoles reach the sources through it.
        let consoles = database::console::provide(
            project.id,
            Arc::new(crate::console::ExplorerConsoles::new(&sessions)),
        );
        // A file renamed or moved (whoever did it): what's kept of it follows.
        let weak = Arc::downgrade(&sessions);
        let moves = project.events.subscribe(move |e: &workspace::events::FileRenamed| {
            if let Some(sessions) = weak.upgrade() {
                sessions.file_moved(&e.from, &e.to);
            }
        });
        vec![register(sessions), consoles, moves]
    }

    fn busy(&self, id: ProjectId) -> bool {
        sessions(id).is_some_and(|s| s.busy())
    }
}

/// The idle setting changed: the watch starts now if sessions are open (it would only
/// on the next connection otherwise).
pub fn rewatch() {
    if all().iter().any(|s| s.connected() > 0) {
        watch();
    }
}

/// Whether the idle watch is on (its job waiting on its pool).
static WATCHING: Mutex<bool> = Mutex::new(false);

/// Starts the idle watch, unless it's on: one job on the `db-idle` pool (one thread),
/// sleeping until the next session is due, closing those due, ending once none is open.
fn watch() {
    {
        let mut on = WATCHING.lock().unwrap_or_else(PoisonError::into_inner);
        if std::mem::replace(&mut *on, true) {
            return;
        }
    }
    static POOL: OnceLock<kernel::Pool> = OnceLock::new();
    let pool = POOL.get_or_init(|| kernel::Pool::new("db-idle", 1, kernel::work::IDLE));
    pool.spawn(kernel::Priority::Background, None, || {
        loop {
            let next = {
                let mut on = WATCHING.lock().unwrap_or_else(PoisonError::into_inner);
                let now = Instant::now();
                let next = crate::settings::idle_close().and_then(|idle| {
                    let all: Vec<Arc<Sessions>> = projects().clone();
                    all.iter().filter_map(|s| s.next_close(now, idle)).min()
                });
                if next.is_none() {
                    // Nothing open (or never closed): a session opening starts it again.
                    *on = false;
                    return;
                }
                next
            };
            if let Some(at) = next {
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
            }
            if let Some(idle) = crate::settings::idle_close() {
                let all: Vec<Arc<Sessions>> = projects().clone();
                for sessions in all {
                    sessions.sweep(Instant::now(), idle);
                }
            }
        }
    });
}

#[cfg(test)]
#[path = "../tests/unit/sessions.rs"]
mod tests;
