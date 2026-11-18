//! The user's extractors (39o2): `*.extractor` templates ([`extractors::template`]) in the
//! project's own folder (`<project dir>/extractors/`: the project's) and in the config
//! folder (`extractors/`: every project's), read in the background on the `extractors` pool
//! and handed to the catalog as the project's "Scripted" ones; read again when they change on
//! disk (the project's folder is heard by the project's watcher — [`DIR`] is one of the
//! project's own folders —, the config folder by a watcher of its own while a project is
//! open). A template with an error is listed with it (its line and column), and its file in
//! the project gets the error in the editor ([`DiagnosticsFound`]); the others work.
//! "Go to Extractors Directory" makes the project's folder, with a few examples the first
//! time ([`crate::examples`]), and opens one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use extractors::template::{EXTENSION, TemplateExtractor};
use kernel::{EventBus, ProjectId, Subscription};
use workbench_model::{Diagnostic, DiagnosticsFound, DiagnosticsScope, Severity};

/// The folder of the templates, in the project's own folder and in the config folder.
pub const DIR: &str = "extractors";

/// At most this many templates are read from a folder.
pub const MAX_FILES: usize = 200;

/// The diagnostics' source (a template's errors).
pub const SOURCE: &str = "extractor-templates";

/// Where a template comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The project's own folder.
    Project,
    /// The config folder (every project's).
    User,
}

impl Origin {
    fn id(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

/// The templates of folder `dir` (sorted by their file's name; none if it isn't there).
pub fn read_dir(dir: &Path, origin: Origin) -> Vec<TemplateExtractor> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == EXTENSION) && p.is_file())
        .collect();
    paths.sort();
    paths.truncate(MAX_FILES);
    paths
        .into_iter()
        .map(|path| {
            let name = kernel::paths::file_name(&path);
            let id = format!("template:{}:{name}", origin.id());
            let text = match std::fs::metadata(&path) {
                Ok(m) if m.len() as usize > extractors::template::MAX_LEN => {
                    "x".repeat(extractors::template::MAX_LEN + 1)
                }
                _ => std::fs::read_to_string(&path).unwrap_or_else(|_| {
                    // Not text: an error said like the others' (its first byte isn't).
                    "{{".to_owned()
                }),
            };
            TemplateExtractor::new(id, &path, &text)
        })
        .collect()
}

/// A project's templates (see the module's doc).
pub struct Templates {
    project: ProjectId,
    root: PathBuf,
    /// `<root>/<project dir>/extractors`.
    own: PathBuf,
    /// The config folder's, if there's one.
    user: Option<PathBuf>,
    events: EventBus,
    /// A reading is asked while one runs: it runs again after.
    again: AtomicBool,
    reading: AtomicBool,
    /// The project's files whose errors were said (to clear them once fixed or gone).
    reported: Mutex<BTreeSet<PathBuf>>,
}

impl std::fmt::Debug for Templates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Templates").field("own", &self.own).finish_non_exhaustive()
    }
}

/// The projects' templates open now (the config folder's watcher reads them all again).
static OPEN: Mutex<Vec<Weak<Templates>>> = Mutex::new(Vec::new());

impl Templates {
    /// Project `project` (at `root`)'s templates, and every project's in `user`.
    pub fn new(
        project: ProjectId,
        root: &Path,
        user: Option<PathBuf>,
        events: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            project,
            root: root.to_path_buf(),
            own: own_dir(root),
            user,
            events,
            again: AtomicBool::new(false),
            reading: AtomicBool::new(false),
            reported: Mutex::new(BTreeSet::new()),
        })
    }

    pub fn own_dir(&self) -> &Path {
        &self.own
    }

    /// Reads them now (on the calling thread: a pool's), handing them to the catalog and
    /// saying the project's errors.
    pub fn read_now(&self) {
        let mut all = read_dir(&self.own, Origin::Project);
        let own_count = all.len();
        if let Some(user) = &self.user {
            all.extend(read_dir(user, Origin::User));
        }
        self.report(&all[..own_count]);
        let list: Vec<Arc<dyn extractors::Extractor>> =
            all.into_iter().map(|t| Arc::new(t) as Arc<dyn extractors::Extractor>).collect();
        extractors::catalog().set_scripted(self.project, list);
    }

    /// The project's templates' errors said on its bus (a clean one's cleared).
    fn report(&self, own: &[TemplateExtractor]) {
        let mut reported = self.reported.lock().unwrap_or_else(PoisonError::into_inner);
        let mut now = BTreeSet::new();
        for template in own {
            let Some(path) = extractors::Extractor::origin(template) else { continue };
            let relative = path.strip_prefix(&self.root).unwrap_or(path).to_path_buf();
            let diagnostics: Vec<Diagnostic> = template
                .error()
                .map(|e| Diagnostic {
                    path: relative.clone(),
                    line: e.line.saturating_sub(1),
                    column: Some(e.column.saturating_sub(1)),
                    end: None,
                    severity: Severity::Error,
                    message: e.message.clone(),
                    source: i18n::t("extract.template_source"),
                    rule: None,
                })
                .into_iter()
                .collect();
            if diagnostics.is_empty() && !reported.contains(&relative) {
                continue;
            }
            if !diagnostics.is_empty() {
                now.insert(relative.clone());
            }
            self.events.publish(&DiagnosticsFound {
                source: SOURCE.to_owned(),
                scope: DiagnosticsScope::File(relative),
                diagnostics,
            });
        }
        // Gone since (deleted, renamed): their errors too.
        for gone in reported.difference(&now).filter(|p| !present(own, &self.root, p)) {
            self.events.publish(&DiagnosticsFound {
                source: SOURCE.to_owned(),
                scope: DiagnosticsScope::File(gone.clone()),
                diagnostics: Vec::new(),
            });
        }
        *reported = now;
    }

    /// Read again in the background (once more if asked while reading).
    pub fn reload(self: &Arc<Self>) {
        if self.reading.swap(true, Ordering::AcqRel) {
            self.again.store(true, Ordering::Release);
            return;
        }
        let this = Arc::clone(self);
        kernel::work::run("extractors", move || {
            loop {
                this.again.store(false, Ordering::Release);
                this.read_now();
                if !this.again.load(Ordering::Acquire) {
                    break;
                }
            }
            this.reading.store(false, Ordering::Release);
            // Asked between the last look and now: read once more.
            if this.again.swap(false, Ordering::AcqRel) {
                this.reload();
            }
        });
    }

    /// Whether a reading runs (the snapshots wait for it).
    pub fn busy(&self) -> bool {
        self.reading.load(Ordering::Acquire)
    }

    /// `path` is one of the project's templates' (or their folder).
    pub fn concerns(&self, path: &Path) -> bool {
        path.starts_with(&self.own) || self.own.starts_with(path)
    }

    /// Follows the project's changes to its folder (read again), while kept.
    pub fn follow(self: &Arc<Self>) -> Vec<Subscription> {
        use workspace::events::{
            EntriesChanged, EntriesDeleted, EntriesRescan, FileRenamed, FilesWritten,
        };
        let weak = Arc::downgrade(self);
        let on = move |paths: &[&Path]| {
            if let Some(this) = weak.upgrade()
                && paths.iter().any(|p| this.concerns(p))
            {
                this.reload();
            }
        };
        let (a, b, c, d) = (on.clone(), on.clone(), on.clone(), on);
        let weak = Arc::downgrade(self);
        vec![
            self.events.subscribe(move |e: &EntriesChanged| a(&as_paths(&e.paths))),
            self.events.subscribe(move |e: &FilesWritten| b(&as_paths(&e.paths))),
            self.events.subscribe(move |e: &EntriesDeleted| c(&as_paths(&e.paths))),
            self.events.subscribe(move |e: &FileRenamed| d(&[e.from.as_path(), e.to.as_path()])),
            self.events.subscribe(move |_: &EntriesRescan| {
                if let Some(this) = weak.upgrade() {
                    this.reload();
                }
            }),
        ]
    }

    /// Kept among the open ones (the config folder's watcher reads them again), until dropped.
    pub fn register(self: &Arc<Self>) {
        let mut open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
        open.retain(|w| w.strong_count() > 0);
        open.push(Arc::downgrade(self));
    }

    pub fn project(&self) -> ProjectId {
        self.project
    }
}

fn as_paths(paths: &[PathBuf]) -> Vec<&Path> {
    paths.iter().map(PathBuf::as_path).collect()
}

/// One of `own` is the file at `relative` (to `root`).
fn present(own: &[TemplateExtractor], root: &Path, relative: &Path) -> bool {
    own.iter().any(|t| {
        extractors::Extractor::origin(t)
            .is_some_and(|p| p.strip_prefix(root).unwrap_or(p) == relative)
    })
}

/// The project's templates' folder: `<root>/<project dir>/extractors`.
pub fn own_dir(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join(DIR)
}

/// The config folder's templates' folder (every project's), if there's a config folder.
pub fn user_dir() -> Option<PathBuf> {
    storage::config_dir().map(|d| d.join(DIR))
}

/// Every open project's templates read again (the config folder changed).
pub fn reload_all() {
    let open: Vec<Arc<Templates>> = OPEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter_map(Weak::upgrade)
        .collect();
    for templates in open {
        templates.reload();
    }
}

/// The open projects' templates of project `id`, if it's open.
pub fn of(id: ProjectId) -> Option<Arc<Templates>> {
    OPEN.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter_map(Weak::upgrade)
        .find(|t| t.project == id)
}

/// The config folder's watcher, while a project is open: the projects that hold it.
struct UserWatch {
    _watcher: Box<dyn workspace::FileWatcher>,
    holders: usize,
}

static USER_WATCH: Mutex<Option<UserWatch>> = Mutex::new(None);

/// The config folder `dir` watched (once, for every project): what changes there is read
/// again by every open project. The watch ends when the last subscription returned goes.
pub fn watch_user_dir(dir: &Path) -> Option<Subscription> {
    if !dir.is_dir() {
        return None;
    }
    let mut watch = USER_WATCH.lock().unwrap_or_else(PoisonError::into_inner);
    if watch.is_none() {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(reload_all);
        let watcher = <fs_local::LocalWatching as workspace::FileWatching>::watch(
            &fs_local::LocalWatching,
            dir,
            workspace::WatchedDirs::default(),
            wake,
        )?;
        *watch = Some(UserWatch { _watcher: watcher, holders: 0 });
    }
    if let Some(w) = watch.as_mut() {
        w.holders += 1;
    }
    Some(Subscription::on_end(|| {
        let mut watch = USER_WATCH.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(w) = watch.as_mut() {
            w.holders = w.holders.saturating_sub(1);
            if w.holders == 0 {
                *watch = None;
            }
        }
    }))
}

#[cfg(test)]
#[path = "../tests/unit/templates.rs"]
mod tests;
