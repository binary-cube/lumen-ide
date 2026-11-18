//! Each open project's servers: started at the first document of their language (one per
//! server and root — the project's, unless the server's root markers say a folder of it),
//! told about the documents as the editor announces them on the project's bus and about the
//! files that change on disk, stopped when unused (by their drivers), when their language's
//! engine stops being theirs, and when the project closes. Their diagnostics are announced
//! on the project's bus; the edits they ask for are made on the UI thread ([`Self::poll`]).
//! "Clear and Rebuild Caches" (`CachesCleared`) stops the servers that write in their own
//! folder of the project's data folder (`${cache}`), removes those folders and starts them
//! again for the documents open.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use code_intelligence::servers::{Engine, Kept, WorkspaceChange};
use document::TextSnapshot;
use document::events::{DocumentChanged, DocumentClosed, DocumentOpened, FileSaved};
use kernel::{DocumentId, EventBus, ProjectId, Subscription};
use lsp_client::client::{Client, Config, Event, FileEvent, RestartPolicy, Timeouts};
use lsp_client::launch::{Launcher, ProcessLauncher};
use lsp_client::spec::{Launch, ServerSpec};
use workbench_model::{DiagnosticsFound, DiagnosticsScope, ProjectContext};
use workspace::events::{
    CachesCleared, EntriesChanged, EntriesDeleted, FileRenamed, FilesWritten, PathMarksChanged,
    ProjectBroughtForward, ProjectLeft,
};

use crate::catalog;
use crate::settings;

/// How long the editor's changes are gathered before they're sent.
pub const DEBOUNCE: Duration = Duration::from_millis(25);

/// A server running for the project.
pub struct Running {
    /// Its identity (UUID v4): the Task Manager's actions and the log find it by it.
    pub id: u128,
    pub spec: ServerSpec,
    pub root: PathBuf,
    pub client: Arc<Client>,
    /// Its bundled version, if it's bundled.
    pub version: Option<String>,
    /// The languages of the documents it was given.
    pub languages: BTreeSet<String>,
    /// Its own folder in the project's data folder, if its description names it.
    pub cache: Option<PathBuf>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running")
            .field("name", &self.spec.name)
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// A document open in the project's editor.
#[derive(Debug, Clone)]
struct OpenDoc {
    path: PathBuf,
    language: String,
    version: u64,
    text: TextSnapshot,
    /// The server it's open in.
    server: Option<u128>,
}

#[derive(Debug, Default)]
struct State {
    docs: HashMap<DocumentId, OpenDoc>,
    servers: Vec<Running>,
    /// What the servers asked to change, for the UI thread.
    edits: Vec<(Option<String>, WorkspaceChange)>,
    /// Why a language has no server (the first server's reason), for the settings.
    missing: HashMap<String, String>,
    /// Servers stopped from the Task Manager: not started again until a restart.
    stopped: BTreeSet<String>,
    /// The servers' folders are being cleared: nothing starts until they're gone.
    purging: bool,
    /// Behind another project's window, or left: dropping this ends the wait to stop its
    /// servers.
    behind: Option<kernel::work::Later>,
    /// Stopped for being behind: its documents go back to servers when it comes forward.
    parked: bool,
}

/// What starts a server (a process; a test's fake in memory).
pub type LaunchWith = Box<dyn Fn(&ServerSpec, &Launch, &Path) -> Box<dyn Launcher> + Send + Sync>;

static LAUNCH: Mutex<Option<LaunchWith>> = Mutex::new(None);

/// How long a question waits for its answer (tests: shorter, to see a timeout).
static REQUEST_TIMEOUT: Mutex<Option<std::time::Duration>> = Mutex::new(None);

/// Questions to the servers started from now on time out after `timeout` (tests).
pub fn request_timeout(timeout: std::time::Duration) {
    if let Ok(mut t) = REQUEST_TIMEOUT.lock() {
        *t = Some(timeout);
    }
}

/// How long a server without documents waits before it stops, whatever the setting says
/// (measurements: seconds instead of the setting's minutes).
static IDLE_AFTER: Mutex<Option<std::time::Duration>> = Mutex::new(None);

/// Servers started from now on stop after `idle` without documents (measurements, tests).
pub fn idle_after(idle: std::time::Duration) {
    if let Ok(mut t) = IDLE_AFTER.lock() {
        *t = Some(idle);
    }
}

/// How long a project behind another's waits before its servers stop, whatever the setting
/// says (tests).
static BEHIND_AFTER: Mutex<Option<Duration>> = Mutex::new(None);

/// A project behind another's stops its servers after `after` (tests).
pub fn behind_after(after: Duration) {
    if let Ok(mut t) = BEHIND_AFTER.lock() {
        *t = Some(after);
    }
}

/// How long a project's window stays left (another program in front, minimized) before its
/// servers stop: at least this, longer if the setting says so (the one the user comes back
/// to would start its servers again).
const LEFT_AFTER: Duration = Duration::from_secs(600);

/// [`LEFT_AFTER`], whatever it and the setting say (tests).
static LEFT_FORCED: Mutex<Option<Duration>> = Mutex::new(None);

/// A project whose window was left stops its servers after `after` (`None`: as said by
/// [`LEFT_AFTER`] and the setting; tests).
pub fn left_after(after: Option<Duration>) {
    if let Ok(mut t) = LEFT_FORCED.lock() {
        *t = after;
    }
}

/// Servers are started by `launch` (tests: a fake server in memory).
pub fn launch_with(launch: LaunchWith) {
    if let Ok(mut l) = LAUNCH.lock() {
        *l = Some(launch);
    }
}

fn launcher(spec: &ServerSpec, launch: &Launch, root: &Path) -> Box<dyn Launcher> {
    if let Ok(l) = LAUNCH.lock()
        && let Some(with) = l.as_ref()
    {
        return with(spec, launch, root);
    }
    Box::new(ProcessLauncher { launch: launch.clone(), dir: root.to_path_buf() })
}

/// A project's servers.
pub struct ProjectServers {
    /// The server whose log the next "Show Log" shows (chosen in the Task Manager).
    log_choice: Mutex<Option<u128>>,
    pub id: ProjectId,
    pub root: PathBuf,
    events: EventBus,
    marks: workspace::marks::ProjectMarks,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The project's own settings (the settings window finds the project by them).
    settings: Option<Arc<settings_model::SettingsService>>,
    /// Tells the user what a server asks to be shown (`show-messages`).
    notifier: Arc<dyn workbench_model::Notifier>,
    state: Mutex<State>,
    me: Weak<ProjectServers>,
}

impl std::fmt::Debug for ProjectServers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectServers").field("root", &self.root).finish_non_exhaustive()
    }
}

/// The open projects' servers, by their identity.
static PROJECTS: workbench_model::PerProject<ProjectServers> = workbench_model::PerProject::new();

/// Project `id`'s servers, if it's open.
pub fn project(id: ProjectId) -> Option<Arc<ProjectServers>> {
    PROJECTS.get(id)
}

/// Every open project's servers.
pub fn all() -> Vec<Arc<ProjectServers>> {
    PROJECTS.all()
}

/// The settings changed: every project's documents go to the engines chosen now.
pub fn refresh_all() {
    for project in all() {
        project.refresh();
    }
}

/// Whether a server named `name` runs in any open project.
pub fn any_running(name: &str) -> bool {
    all().iter().any(|p| p.servers(|s| s.iter().any(|r| r.spec.name == name && !r.client.ended())))
}

/// The servers given secret `id` start again (with it as it's now).
pub fn restart_using_secret(id: &str) {
    for project in all() {
        let ids: Vec<u128> = project.servers(|s| {
            s.iter()
                .filter(|r| r.spec.secret_options.values().any(|v| v == id))
                .map(|r| r.id)
                .collect()
        });
        for id in ids {
            project.restart(id);
        }
    }
}

/// The source the diagnostics of server `name` are announced as.
pub fn source(name: &str) -> String {
    code_intelligence::servers::diagnostics_source(name)
}

impl ProjectServers {
    /// Starts following project `context`: its documents and its files' changes. The
    /// subscriptions hold it; when they end (the project closes), its servers stop.
    pub fn attach(context: &ProjectContext<'_>) -> (Arc<Self>, Vec<Subscription>) {
        let servers = Arc::new_cyclic(|me| Self {
            log_choice: Mutex::default(),
            id: context.id,
            root: context.root.to_path_buf(),
            events: context.events.clone(),
            marks: context.marks.clone(),
            wake: Arc::clone(&context.wake),
            settings: context.settings.clone(),
            notifier: Arc::clone(&context.notifier),
            state: Mutex::default(),
            me: me.clone(),
        });
        PROJECTS.insert(context.id, &servers);
        // Left until its window comes forward (opened with the app behind: it may never).
        servers.left();
        let on = |servers: &Arc<Self>| Arc::clone(servers);
        let (opened, changed, closed, saved) =
            (on(&servers), on(&servers), on(&servers), on(&servers));
        let (written, entries, deleted, renamed) =
            (on(&servers), on(&servers), on(&servers), on(&servers));
        let (ended, purged, forward, remarked) =
            (on(&servers), on(&servers), on(&servers), on(&servers));
        let left = on(&servers);
        let id = context.id;
        let subscriptions = vec![
            context.events.subscribe(move |e: &DocumentOpened| opened.opened(e)),
            context.events.subscribe(move |e: &DocumentChanged| changed.changed(e)),
            context.events.subscribe(move |e: &DocumentClosed| closed.closed(e.document)),
            context.events.subscribe(move |e: &FileSaved| saved.saved(&e.path)),
            context.events.subscribe(move |e: &FilesWritten| {
                written.disk(e.paths.iter().map(|p| (p.clone(), FileEvent::Changed)).collect());
            }),
            context.events.subscribe(move |e: &EntriesChanged| {
                let events = e.paths.iter().map(|p| {
                    (p.clone(), if p.exists() { FileEvent::Created } else { FileEvent::Deleted })
                });
                entries.disk(events.collect());
            }),
            context.events.subscribe(move |e: &EntriesDeleted| {
                deleted.disk(e.paths.iter().map(|p| (p.clone(), FileEvent::Deleted)).collect());
            }),
            context.events.subscribe(move |e: &FileRenamed| {
                renamed.disk(vec![
                    (e.from.clone(), FileEvent::Deleted),
                    (e.to.clone(), FileEvent::Created),
                ]);
            }),
            context.events.subscribe(move |_: &CachesCleared| purged.purge_caches()),
            context.events.subscribe(move |_: &PathMarksChanged| remarked.marks_changed()),
            context.events.subscribe(move |_: &ProjectBroughtForward| forward.brought_forward()),
            context.events.subscribe(move |_: &ProjectLeft| left.left()),
            Subscription::on_end(move || {
                ended.close();
                PROJECTS.remove(id);
            }),
        ];
        (servers, subscriptions)
    }

    /// Whether `settings` are this project's own.
    pub fn uses(&self, settings: &settings_model::SettingsService) -> bool {
        self.settings.as_deref().is_some_and(|s| std::ptr::eq(s, settings))
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut state)
    }

    fn opened(&self, e: &DocumentOpened) {
        self.with(|s| {
            let doc = OpenDoc {
                path: e.path.clone(),
                language: e.language.clone(),
                version: e.version,
                text: e.text.clone(),
                server: None,
            };
            s.docs.insert(e.document, doc);
            self.route(s, e.document);
        });
    }

    fn changed(&self, e: &DocumentChanged) {
        self.with(|s| {
            let Some(doc) = s.docs.get_mut(&e.document) else { return };
            doc.version = e.version;
            doc.text = e.text.clone();
            if let Some(server) = doc.server.and_then(|id| s.servers.iter().find(|r| r.id == id)) {
                server.client.change(e.document, e.version, e.text.clone());
            }
        });
    }

    fn closed(&self, document: DocumentId) {
        self.with(|s| {
            let Some(doc) = s.docs.remove(&document) else { return };
            if let Some(server) = doc.server.and_then(|id| s.servers.iter().find(|r| r.id == id)) {
                server.client.close(document);
            }
        });
    }

    fn saved(&self, path: &Path) {
        self.with(|s| {
            for doc in s.docs.values().filter(|d| d.path == path) {
                if let Some(server) =
                    doc.server.and_then(|id| s.servers.iter().find(|r| r.id == id))
                {
                    server.client.saved(path.to_path_buf());
                }
            }
        });
    }

    /// Files changed on disk: each server hears those its patterns name.
    fn disk(&self, changes: Vec<(PathBuf, FileEvent)>) {
        let marks = self.marks.get();
        let excluded = |path: &Path| {
            path.strip_prefix(&self.root)
                .is_ok_and(|r| marks.is_excluded(&r.to_string_lossy().replace('\\', "/")))
        };
        self.with(|s| {
            for server in &s.servers {
                let heard: Vec<(PathBuf, FileEvent)> = changes
                    .iter()
                    .filter(|(path, _)| {
                        path.strip_prefix(&server.root).is_ok_and(|relative| {
                            let relative = relative.to_string_lossy().replace('\\', "/");
                            !kernel::paths::in_dependencies(path)
                                && !excluded(path)
                                && server.spec.watches(&relative)
                        })
                    })
                    .cloned()
                    .collect();
                server.client.watched(heard);
            }
        });
    }

    /// Opens document `document` in its language's server, starting it if needed (when the
    /// language's engine is a server's).
    fn route(&self, s: &mut State, document: DocumentId) {
        let Some(doc) = s.docs.get(&document).cloned() else { return };
        if s.purging || s.parked {
            return;
        }
        let config = settings::config();
        let choice = config.choice(&doc.language);
        if !choice.engine.uses_server() {
            return;
        }
        let catalog = catalog::catalog();
        if catalog.for_language(&doc.language).next().is_none() {
            return;
        }
        // Ended ones (unused for long, crashed for good) make room for new ones.
        s.servers.retain(|r| !r.client.ended());
        let bundle = catalog::bundle();
        let chosen =
            catalog.choose(&doc.language, choice.server.as_deref(), &config.disabled, |spec| {
                catalog::places(bundle.as_deref(), spec, &self.root, config.max_memory)
            });
        let (spec, launch) = match chosen {
            Ok(found) => found,
            Err(missing) => {
                let why = missing.map_or_else(|| "turned off".to_owned(), |m| m.to_string());
                s.missing.insert(doc.language.clone(), why);
                return;
            }
        };
        s.missing.remove(&doc.language);
        if s.stopped.contains(&spec.name) {
            return;
        }
        let root = lsp_client::spec::root_for(&doc.path, &spec.root_markers, &self.root);
        let index = match s.servers.iter().position(|r| r.spec.name == spec.name && r.root == root)
        {
            Some(i) => i,
            None => match self.start(spec, &launch, root, bundle.as_deref()) {
                Some(running) => {
                    s.servers.push(running);
                    s.servers.len() - 1
                }
                None => return,
            },
        };
        let server = &mut s.servers[index];
        server.languages.insert(doc.language.clone());
        let language_id = server.spec.language_id(&doc.language, &doc.path);
        server.client.open(document, doc.path.clone(), language_id, doc.version, doc.text.clone());
        let id = server.id;
        if let Some(doc) = s.docs.get_mut(&document) {
            doc.server = Some(id);
        }
    }

    /// Starts server `spec` at `root`.
    fn start(
        &self,
        spec: ServerSpec,
        launch: &Launch,
        root: PathBuf,
        bundle: Option<&lsp_client::bundle::Bundle>,
    ) -> Option<Running> {
        let id = kernel::id::new_uuid();
        let config = settings::config();
        let excluded = self.excluded_paths();
        let launch = &launch.excluding(&spec, &excluded);
        // Its own folders (its log, what it generates): in the project's data folder, in the
        // app's.
        for dir in launch.cache.iter().chain(&launch.data) {
            if let Err(e) = std::fs::create_dir_all(dir) {
                tracing::warn!(server = %spec.name, "its folder {} can't be made: {e}", dir.display());
            }
        }
        let client_config = Config {
            name: spec.name.clone(),
            root: root.clone(),
            project_root: self.root.clone(),
            initialization_options: launch.initialization_options.clone(),
            settings: launch.settings.clone(),
            contents: launch.contents.clone(),
            completion_kinds: launch.completion_kinds.clone(),
            on_initialized: launch.on_initialized.clone(),
            debounce: DEBOUNCE,
            timeouts: Timeouts {
                request: REQUEST_TIMEOUT
                    .lock()
                    .ok()
                    .and_then(|t| *t)
                    .unwrap_or_else(|| Timeouts::default().request),
                ..Timeouts::default()
            },
            restart: RestartPolicy::default(),
            idle: IDLE_AFTER.lock().ok().and_then(|t| *t).or(config.idle),
            trace: settings::tracing_flag(),
            secret_options: launch.secret_options.clone(),
            // Read on its thread when it starts (never here: the keychain may wait).
            secrets: (!launch.secret_options.is_empty()).then(crate::licence::store),
            show_messages: launch.show_messages,
            wake: Arc::clone(&self.wake),
        };
        let me = self.me.clone();
        let events = self.events.clone();
        let source = source(&spec.name);
        let notifier = Arc::clone(&self.notifier);
        let server_name = spec.name.clone();
        let on_event = Arc::new(move |event: Event| match event {
            Event::Diagnostics { path, diagnostics } => {
                events.publish(&DiagnosticsFound {
                    source: source.clone(),
                    scope: DiagnosticsScope::File(path),
                    diagnostics,
                });
            }
            Event::DiagnosticsCleared => {
                events.publish(&DiagnosticsFound {
                    source: source.clone(),
                    scope: DiagnosticsScope::Project,
                    diagnostics: Vec::new(),
                });
            }
            Event::ApplyEdit { label, change } => {
                if let Some(me) = me.upgrade() {
                    me.with(|s| s.edits.push((label, change)));
                }
            }
            Event::Ended => {
                if let Some(me) = me.upgrade() {
                    me.with(|s| {
                        s.servers.retain(|r| r.id != id);
                        for doc in s.docs.values_mut().filter(|d| d.server == Some(id)) {
                            doc.server = None;
                        }
                    });
                }
            }
            Event::Changed => {}
            Event::Message { kind, text } => {
                let title = i18n::tr!("lsp.server_message", name = server_name.as_str());
                let notification = match kind {
                    lsp_client::client::MessageKind::Error => {
                        workbench_model::Notification::error(title)
                    }
                    lsp_client::client::MessageKind::Warning => {
                        workbench_model::Notification::warning(title)
                    }
                    lsp_client::client::MessageKind::Info => {
                        workbench_model::Notification::info(title)
                    }
                };
                notifier.show(notification.with_detail(text));
            }
        });
        let version = catalog::bundled_version(bundle, &spec);
        match Client::start(client_config, launcher(&spec, launch, &root), on_event) {
            Ok(client) => {
                tracing::info!(server = %spec.name, root = %root.display(), "language server started");
                // Its log kept on disk too (`<project>/<project dir>/lsp/logs/<server>.log`),
                // opened off this thread.
                let log = self.root.join(kernel::brand::PROJECT_DIR).join("lsp").join("logs");
                let name: String = spec
                    .name
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' })
                    .collect();
                let log = log.join(format!("{name}.log"));
                let status = Arc::clone(client.status());
                kernel::work::run("lsp-log", move || status.keep_log_in(&log));
                Some(Running {
                    id,
                    spec,
                    root,
                    client: Arc::new(client),
                    version,
                    languages: BTreeSet::new(),
                    cache: launch.cache.clone(),
                })
            }
            Err(e) => {
                tracing::warn!(server = %spec.name, "a language server's thread didn't start: {e}");
                None
            }
        }
    }

    /// The documents go to the engines chosen now: those whose language's engine isn't a
    /// server any more (or whose server is turned off) are closed in it, those whose is are
    /// opened; servers left without documents and not chosen stop.
    pub fn refresh(&self) {
        self.with(|s| {
            let config = settings::config();
            let catalog = catalog::catalog();
            let ids: Vec<DocumentId> = s.docs.keys().copied().collect();
            for document in ids {
                let Some(doc) = s.docs.get(&document).cloned() else { continue };
                let choice = config.choice(&doc.language);
                let current = doc.server.and_then(|id| s.servers.iter().find(|r| r.id == id));
                let wanted = choice.engine.uses_server()
                    && current.is_none_or(|r| {
                        !config.disabled.contains(&r.spec.name)
                            && choice.server.as_ref().is_none_or(|n| *n == r.spec.name)
                    });
                match (current, wanted) {
                    (Some(server), false) => {
                        server.client.close(document);
                        if let Some(d) = s.docs.get_mut(&document) {
                            d.server = None;
                        }
                        if choice.engine.uses_server() {
                            self.route(s, document);
                        }
                    }
                    (None, true) => self.route(s, document),
                    _ => {}
                }
            }
            // Servers no document uses and nothing chooses: stopped now.
            let used: BTreeSet<u128> = s.docs.values().filter_map(|d| d.server).collect();
            s.servers.retain(|r| {
                let chosen = r.languages.iter().any(|l| {
                    let choice = config.choice(l);
                    choice.engine.uses_server()
                        && choice.server.as_ref().is_none_or(|n| *n == r.spec.name)
                }) && !config.disabled.contains(&r.spec.name)
                    && catalog.named(&r.spec.name).is_some();
                used.contains(&r.id) || chosen
            });
        });
    }

    /// What the user excluded, as paths from the project's root.
    fn excluded_paths(&self) -> Vec<String> {
        use workspace::marks::PathMark;
        let marks = self.marks.get();
        marks
            .iter()
            .filter(|(_, mark)| *mark == PathMark::Excluded)
            .map(|(path, _)| path.to_owned())
            .collect()
    }

    /// The user's marks changed: the servers told what to leave out start again with it
    /// (none of them takes it while running); their documents go back to them.
    fn marks_changed(&self) {
        let gone = self.with(|s| {
            let (gone, kept): (Vec<Running>, Vec<Running>) = std::mem::take(&mut s.servers)
                .into_iter()
                .partition(|r| r.spec.excluded_paths.is_some());
            s.servers = kept;
            for doc in s.docs.values_mut() {
                if doc.server.is_some_and(|id| gone.iter().any(|r| r.id == id)) {
                    doc.server = None;
                }
            }
            gone
        });
        if gone.is_empty() {
            return;
        }
        for server in &gone {
            server.client.stop();
        }
        self.with(|s| {
            let waiting: Vec<DocumentId> =
                s.docs.iter().filter(|(_, d)| d.server.is_none()).map(|(id, _)| *id).collect();
            for document in waiting {
                self.route(s, document);
            }
        });
    }

    /// This project's window came forward: it's served again, the others wait to stop.
    fn brought_forward(&self) {
        for other in all().iter().filter(|p| p.id != self.id) {
            other.went_behind();
        }
        self.with(|s| {
            s.behind = None;
            if std::mem::take(&mut s.parked) {
                let waiting: Vec<DocumentId> =
                    s.docs.iter().filter(|(_, d)| d.server.is_none()).map(|(id, _)| *id).collect();
                for document in waiting {
                    self.route(s, document);
                }
            }
        });
        (self.wake)();
    }

    /// Another project's window is in front: after the idle time (none: never), this one's
    /// servers stop until it comes forward. Waits on its own thread, ended by coming forward.
    fn went_behind(&self) {
        let forced = BEHIND_AFTER.lock().ok().and_then(|t| *t);
        if let Some(after) = forced.or(settings::config().idle) {
            self.park_after(after);
        }
    }

    /// Its window lost the keyboard: after a long while (the idle time, at least
    /// [`LEFT_AFTER`]; no idle time: never), its servers stop until it comes forward.
    fn left(&self) {
        let forced = LEFT_FORCED.lock().ok().and_then(|t| *t);
        let after = forced.or_else(|| settings::config().idle.map(|idle| idle.max(LEFT_AFTER)));
        if let Some(after) = after {
            self.park_after(after);
        }
    }

    /// Its servers stop after `after`, unless they do sooner already or it comes forward.
    fn park_after(&self, after: Duration) {
        let me = self.me.clone();
        self.with(|s| {
            kernel::work::Later::sooner(&mut s.behind, "lsp-behind", after, move || {
                if let Some(me) = me.upgrade() {
                    me.park();
                }
            });
        });
    }

    /// Still behind: its servers stop, its documents answered by the built-in engine.
    fn park(&self) {
        let gone = self.with(|s| {
            if s.behind.is_none() || s.parked {
                return Vec::new();
            }
            s.parked = true;
            for doc in s.docs.values_mut() {
                doc.server = None;
            }
            std::mem::take(&mut s.servers)
        });
        for server in &gone {
            server.client.stop();
        }
        drop(gone);
        (self.wake)();
    }

    /// The project closed: its servers stop.
    fn close(&self) {
        let servers = self.with(|s| {
            s.behind = None;
            s.docs.clear();
            std::mem::take(&mut s.servers)
        });
        drop(servers);
    }

    /// "Clear and Rebuild Caches": the servers writing in their own folder stop, the project's
    /// servers' folders (`<root>/<project dir>/lsp/`) are removed and the documents those
    /// servers had start them again — off the UI thread (a server takes a moment to
    /// stop; the folder can't go before), nothing started meanwhile.
    pub fn purge_caches(&self) {
        let gone = self.with(|s| {
            s.purging = true;
            let (gone, kept): (Vec<Running>, Vec<Running>) =
                std::mem::take(&mut s.servers).into_iter().partition(|r| r.cache.is_some());
            s.servers = kept;
            for doc in s.docs.values_mut() {
                if doc.server.is_some_and(|id| gone.iter().any(|r| r.id == id)) {
                    doc.server = None;
                }
            }
            gone
        });
        let me = self.me.clone();
        let folder = self.root.join(kernel::brand::PROJECT_DIR).join("lsp");
        let purge = move || {
            for server in &gone {
                server.client.stop();
            }
            let until = std::time::Instant::now() + Duration::from_secs(10);
            while gone.iter().any(|r| !r.client.ended()) && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(gone);
            if folder.exists()
                && let Err(e) = std::fs::remove_dir_all(&folder)
            {
                tracing::warn!(
                    "the language servers' folder {} can't be removed: {e}",
                    folder.display()
                );
            }
            if let Some(me) = me.upgrade() {
                me.with(|s| {
                    s.purging = false;
                    let waiting: Vec<DocumentId> = s
                        .docs
                        .iter()
                        .filter(|(_, d)| d.server.is_none())
                        .map(|(id, _)| *id)
                        .collect();
                    for document in waiting {
                        me.route(s, document);
                    }
                });
                (me.wake)();
            }
            tracing::info!("the language servers' folders were cleared");
        };
        kernel::work::run("lsp-purge", purge);
    }

    /// The edits the servers asked for since the last look.
    pub fn take_edits(&self) -> Vec<(Option<String>, WorkspaceChange)> {
        self.with(|s| std::mem::take(&mut s.edits))
    }

    /// The client serving document `document` (its server's, whatever its state).
    pub fn client_of(&self, document: DocumentId) -> Option<Arc<Client>> {
        self.with(|s| {
            let id = s.docs.get(&document)?.server?;
            s.servers.iter().find(|r| r.id == id).map(|r| Arc::clone(&r.client))
        })
    }

    /// What answers language `language`'s documents now: its engine, the built-in one while
    /// none of its servers is ready.
    pub fn served(&self, language: &str) -> Engine {
        let choice = settings::config().choice(language);
        if !choice.engine.uses_server() {
            return Engine::BuiltIn;
        }
        let ready = self.with(|s| {
            s.servers.iter().any(|r| {
                r.languages.contains(language)
                    && matches!(
                        r.client.status().state(),
                        lsp_client::status::State::Ready | lsp_client::status::State::Idle
                    )
            })
        });
        if ready { choice.engine } else { Engine::BuiltIn }
    }

    /// What the built-in engine keeps answering for `language`, as its running servers'
    /// descriptions say (none running: nothing — it answers everything anyway).
    pub fn kept(&self, language: &str) -> Kept {
        self.with(|s| {
            s.servers
                .iter()
                .filter(|r| r.languages.contains(language))
                .map(|r| Kept::from_names(r.spec.built_in.iter().map(String::as_str)))
                .fold(Kept::default(), Kept::union)
        })
    }

    /// The languages the server answering `language` answers too (its description's),
    /// `language` included.
    pub fn languages_with(&self, language: &str) -> Vec<String> {
        let mut all: Vec<String> = self.with(|s| {
            s.servers
                .iter()
                .filter(|r| r.languages.contains(language))
                .flat_map(|r| r.spec.languages.iter().cloned())
                .collect()
        });
        all.push(language.to_owned());
        all.sort();
        all.dedup();
        all
    }

    /// The servers running, as `f` sees them.
    pub fn servers<R>(&self, f: impl FnOnce(&[Running]) -> R) -> R {
        self.with(|s| f(&s.servers))
    }

    /// Why `language` has no server, if it hasn't.
    pub fn missing(&self, language: &str) -> Option<String> {
        self.with(|s| s.missing.get(language).cloned())
    }

    /// Server `id` started again (its crashes forgotten).
    /// Server `id`'s log is the one the next "Show Log" shows.
    pub fn choose_log(&self, id: u128) {
        if let Ok(mut choice) = self.log_choice.lock() {
            *choice = Some(id);
        }
    }

    pub(crate) fn take_log_choice(&self) -> Option<u128> {
        self.log_choice.lock().ok().and_then(|mut choice| choice.take())
    }

    pub fn restart(&self, id: u128) -> bool {
        self.with(|s| {
            let Some(server) = s.servers.iter().find(|r| r.id == id) else { return false };
            s.stopped.remove(&server.spec.name);
            server.client.restart();
            true
        })
    }

    /// Server `id` stopped (its documents answered by the built-in engine) until it's
    /// restarted (from the settings or a new start).
    pub fn stop(&self, id: u128) -> bool {
        let stopped = self.with(|s| {
            let i = s.servers.iter().position(|r| r.id == id)?;
            let server = s.servers.remove(i);
            s.stopped.insert(server.spec.name.clone());
            for doc in s.docs.values_mut().filter(|d| d.server == Some(id)) {
                doc.server = None;
            }
            Some(server)
        });
        stopped.is_some()
    }

    /// The servers `uses` says let go (their files are about to be removed): stopped, their
    /// documents answered by the built-in engine, not marked stopped (they start again once
    /// what they need is back). Their clients, to wait for them to end.
    pub fn let_go(&self, uses: &dyn Fn(&ServerSpec) -> bool) -> Vec<Arc<Client>> {
        self.with(|s| {
            let (gone, kept): (Vec<Running>, Vec<Running>) =
                std::mem::take(&mut s.servers).into_iter().partition(|r| uses(&r.spec));
            s.servers = kept;
            for doc in s.docs.values_mut() {
                if doc.server.is_some_and(|id| gone.iter().any(|r| r.id == id)) {
                    doc.server = None;
                }
            }
            gone.into_iter()
                .map(|r| {
                    r.client.stop();
                    r.client
                })
                .collect()
        })
    }

    /// Every server of the project started again (the stopped ones too).
    pub fn restart_all(&self) {
        let stopped = self.with(|s| {
            for server in &s.servers {
                server.client.restart();
            }
            !std::mem::take(&mut s.stopped).is_empty()
        });
        if stopped {
            self.refresh();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/projects.rs"]
mod tests;
