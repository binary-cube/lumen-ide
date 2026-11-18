//! The module's part following each open project: its catalog of `.env` files
//! ([`crate::catalog`]) kept up to date from the project's events, and the checks said on its
//! bus ([`DiagnosticsFound`], shown as the checkers' are):
//!
//! - an open document reading a variable no `.env` file defines nor documents: a warning,
//!   as it's typed (its newest version checked, on the module's thread);
//! - with `[dotenv] unused_keys`, a key of a `.env` file no code of the project reads: a
//!   note (the project's files read in the background, each again only once changed).
//!
//! The events are the project's own: another project's never reach it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use document::TextSnapshot;
use document::events::{DocumentChanged, DocumentClosed, DocumentOpened, FileSaved};
use i18n::t;
use kernel::{EventBus, Latest, Subscription};
use workbench_model::{
    Diagnostic, DiagnosticsFound, DiagnosticsScope, LanguageOf, ProjectContext, ProjectService,
    Severity,
};
use workspace::events::{
    CachesCleared, EntriesChanged, EntriesDeleted, EntriesRescan, FileRenamed, FilesWritten,
};

use crate::catalog::{self, Catalog};
use crate::reads::{FileReads, reads};
use crate::used::UsedKeys;
use crate::{checks, settings};

/// Who the diagnostics come from (kept apart from the other sources').
const SOURCE: &str = "dotenv";

#[derive(Debug, Default)]
pub struct EnvService {
    /// The open projects' checks (while they're open).
    projects: Mutex<Vec<(kernel::ProjectId, Weak<Checks>)>>,
}

impl ProjectService for EnvService {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        let checks = Checks::new(project);
        let mut projects = lock(&self.projects);
        projects.retain(|(_, c)| c.strong_count() > 0);
        projects.push((project.id, Arc::downgrade(&checks)));
        drop(projects);
        let on_read = Arc::downgrade(&checks);
        let catalog = Catalog::new(
            project.root.to_path_buf(),
            Arc::clone(&project.language_of),
            Arc::new(move || {
                if let Some(checks) = on_read.upgrade() {
                    checks.files_read();
                }
            }),
        );
        let _ = checks.catalog.set(Arc::clone(&catalog));
        let mut subscriptions = vec![catalog::keep(project.id, Arc::clone(&catalog))];
        subscriptions.extend(follow(project.events, &checks));
        subscriptions
    }

    /// Its `.env` files being read, or documents waiting to be checked.
    fn busy(&self, id: kernel::ProjectId) -> bool {
        let projects = lock(&self.projects);
        let checks = projects.iter().find(|(p, _)| *p == id).and_then(|(_, c)| c.upgrade());
        checks.is_some_and(|c| c.busy())
    }
}

/// The project's events, for its catalog and its checks.
fn follow(events: &EventBus, checks: &Arc<Checks>) -> Vec<Subscription> {
    let to = || Arc::clone(checks);
    vec![
        {
            let c = to();
            events.subscribe(move |e: &FileSaved| c.changed(std::slice::from_ref(&e.path)))
        },
        {
            let c = to();
            events.subscribe(move |e: &FilesWritten| c.changed(&e.paths))
        },
        {
            let c = to();
            events.subscribe(move |e: &FileRenamed| c.changed(&[e.from.clone(), e.to.clone()]))
        },
        {
            let c = to();
            events.subscribe(move |e: &EntriesDeleted| c.changed(&e.paths))
        },
        {
            let c = to();
            events.subscribe(move |e: &EntriesChanged| c.changed(&e.paths))
        },
        {
            let c = to();
            events.subscribe(move |_: &EntriesRescan| c.rescan())
        },
        {
            let c = to();
            events.subscribe(move |_: &CachesCleared| c.rescan())
        },
        {
            let c = to();
            events
                .subscribe(move |e: &DocumentOpened| c.opened(&e.path, &e.language, e.text.clone()))
        },
        {
            let c = to();
            events.subscribe(move |e: &DocumentChanged| c.edited(&e.path, e.text.clone()))
        },
        {
            let c = to();
            events.subscribe(move |e: &DocumentClosed| c.closed(&e.path))
        },
    ]
}

/// An open document checked: its language and its newest text.
#[derive(Debug, Clone)]
struct Open {
    language: String,
    text: TextSnapshot,
}

/// A project's checks.
struct Checks {
    root: PathBuf,
    marks: workspace::marks::ProjectMarks,
    events: EventBus,
    wake: Arc<dyn Fn() + Send + Sync>,
    language_of: LanguageOf,
    catalog: OnceLock<Arc<Catalog>>,
    /// The open documents whose code reads variables, by path.
    open: Mutex<HashMap<PathBuf, Open>>,
    /// The documents checked on the module's thread, the newest version of each.
    latest: OnceLock<Latest<PathBuf, PathBuf, ()>>,
    /// The documents asked to be checked, not checked yet.
    waiting: Mutex<HashSet<PathBuf>>,
    /// The files with diagnostics said (relative to the root): cleared once fine.
    reported: Mutex<HashSet<PathBuf>>,
    /// What the project's code reads (walked while unused keys are flagged).
    used: Mutex<UsedKeys>,
    /// The unused keys' walk is running, and asked again meanwhile.
    walking: Mutex<(bool, bool)>,
    /// Told when the settings change (unused keys on or off).
    _settings: Arc<dyn Fn() + Send + Sync>,
}

impl Checks {
    fn new(project: &ProjectContext<'_>) -> Arc<Self> {
        Arc::new_cyclic(|this: &Weak<Self>| {
            let told = Weak::clone(this);
            let settings_changed: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Some(checks) = told.upgrade() {
                    checks.unused_keys();
                }
            });
            settings::follow(&settings_changed);
            Self {
                root: project.root.to_path_buf(),
                marks: project.marks.clone(),
                events: project.events.clone(),
                wake: Arc::clone(&project.wake),
                language_of: Arc::clone(&project.language_of),
                catalog: OnceLock::new(),
                open: Mutex::default(),
                latest: OnceLock::new(),
                waiting: Mutex::default(),
                reported: Mutex::default(),
                used: Mutex::default(),
                walking: Mutex::default(),
                _settings: settings_changed,
            }
        })
    }

    /// Its `.env` files being read, documents waiting to be checked, the project walked.
    fn busy(&self) -> bool {
        self.catalog().is_some_and(|c| c.busy())
            || !lock(&self.waiting).is_empty()
            || lock(&self.walking).0
    }

    fn catalog(&self) -> Option<&Arc<Catalog>> {
        self.catalog.get()
    }

    /// Checks `path` on the module's thread (its newest version, once).
    fn request(self: &Arc<Self>, path: &Path) {
        let latest = self.latest.get_or_init(|| {
            let this = Arc::downgrade(self);
            Latest::new(
                crate::THREAD,
                move |path: PathBuf| {
                    if let Some(checks) = this.upgrade() {
                        checks.check(&path);
                        lock(&checks.waiting).remove(&path);
                    }
                },
                Arc::clone(&self.wake),
            )
        });
        lock(&self.waiting).insert(path.to_path_buf());
        latest.request(path.to_path_buf(), path.to_path_buf());
        // Its results are said on the bus: nothing to take back.
        let _ = latest.take();
    }

    fn opened(self: &Arc<Self>, path: &Path, language: &str, text: TextSnapshot) {
        if self.reads_variables(path, language) {
            let open = Open { language: language.to_owned(), text };
            lock(&self.open).insert(path.to_path_buf(), open);
            self.request(path);
        }
    }

    fn edited(self: &Arc<Self>, path: &Path, text: TextSnapshot) {
        let known = lock(&self.open).get_mut(path).map(|doc| doc.text = text).is_some();
        if known {
            self.request(path);
        }
    }

    fn closed(&self, path: &Path) {
        if lock(&self.open).remove(path).is_some() {
            self.say(path, Vec::new());
        }
    }

    /// Paths changed on disk: the catalog's folders read again; the unused keys' walk too.
    fn changed(self: &Arc<Self>, paths: &[PathBuf]) {
        let Some(catalog) = self.catalog() else { return };
        for path in paths {
            catalog.changed(path);
        }
        if settings::unused_keys() {
            self.walk_again();
        }
    }

    fn rescan(&self) {
        if let Some(catalog) = self.catalog() {
            catalog.rescan();
        }
    }

    /// A folder's `.env` files were read: every open document checked again, the unused
    /// keys too.
    fn files_read(self: &Arc<Self>) {
        let open: Vec<PathBuf> = lock(&self.open).keys().cloned().collect();
        for path in open {
            self.request(&path);
        }
        self.unused_keys();
    }

    /// The code of `path` (in `language`) reads variables whose definitions are checked.
    fn reads_variables(&self, path: &Path, language: &str) -> bool {
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        !FileReads::of(&code_intelligence::env::reads(), language, &name).flagged.is_empty()
    }

    /// The variables open document `path` reads that no `.env` file defines, said.
    fn check(&self, path: &Path) {
        let open = lock(&self.open).get(path).cloned();
        let (Some(open), Some(catalog)) = (open, self.catalog()) else { return };
        // Still being read: checked once it is.
        let Some(docs) = catalog.files_for(path) else { return };
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let file = FileReads::of(&code_intelligence::env::reads(), &open.language, &name);
        let text = open.text.slice(0..open.text.len());
        // A project without `.env` files: nothing to hold its code to.
        let undefined = match docs.is_empty() {
            true => Vec::new(),
            false => {
                let defined = |key: &str| {
                    docs.iter().any(|d| d.defined(key).is_some()) || std::env::var_os(key).is_some()
                };
                checks::undefined(&text, reads(&text, &file.flagged), &defined, &file.provided)
            }
        };
        let message = t("dotenv.undefined");
        let found = undefined
            .into_iter()
            .map(|r| diagnostic(&self.root, path, &text, r, Severity::Warning, &message))
            .collect();
        self.say(path, found);
    }

    /// The unused keys' notes: the project's code walked in the background while they're
    /// on, cleared once they're off.
    fn unused_keys(self: &Arc<Self>) {
        if settings::unused_keys() {
            self.walk_again();
            return;
        }
        let Some(catalog) = self.catalog() else { return };
        for doc in catalog.all().iter().flat_map(|docs| docs.iter()) {
            self.say(&doc.path, Vec::new());
        }
    }

    /// Walks the project again for what its code reads, then says the unused keys (once
    /// more if asked meanwhile).
    fn walk_again(self: &Arc<Self>) {
        {
            let mut walking = lock(&self.walking);
            if walking.0 {
                walking.1 = true;
                return;
            }
            walking.0 = true;
        }
        let this = Arc::clone(self);
        kernel::work::run(crate::THREAD, move || {
            loop {
                this.say_unused();
                let mut walking = lock(&this.walking);
                if !std::mem::take(&mut walking.1) {
                    walking.0 = false;
                    break;
                }
            }
        });
    }

    /// Each `.env` file's keys nothing in the project reads, said (the project walked for
    /// what its code reads: only what changed read again).
    fn say_unused(&self) {
        let Some(catalog) = self.catalog() else { return };
        let mut used = lock(&self.used);
        used.refresh(
            &self.root,
            &self.marks.get(),
            &self.language_of,
            &code_intelligence::env::reads(),
        );
        let read = used.keys();
        let message = t("dotenv.unused");
        for doc in catalog.all().iter().flat_map(|docs| docs.iter()).filter(|d| !d.documents) {
            let found = checks::unused(&doc.text, &doc.file, &|key| read.contains(key))
                .into_iter()
                .map(|r| diagnostic(&self.root, &doc.path, &doc.text, r, Severity::Info, &message))
                .collect();
            self.say(&doc.path, found);
        }
    }

    /// `path`'s diagnostics said (none: cleared, if any were said).
    fn say(&self, path: &Path, diagnostics: Vec<Diagnostic>) {
        let relative = path.strip_prefix(&self.root).unwrap_or(path).to_path_buf();
        let mut reported = lock(&self.reported);
        if diagnostics.is_empty() && !reported.remove(&relative) {
            return;
        }
        if !diagnostics.is_empty() {
            reported.insert(relative.clone());
        }
        drop(reported);
        self.events.publish(&DiagnosticsFound {
            source: SOURCE.to_owned(),
            scope: DiagnosticsScope::File(relative),
            diagnostics,
        });
        (self.wake)();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A diagnostic of `path` (under `root`) at `range` of its `text`.
fn diagnostic(
    root: &Path,
    path: &Path,
    text: &str,
    range: std::ops::Range<usize>,
    severity: Severity,
    message: &str,
) -> Diagnostic {
    let (line, column) = checks::position(text, range.start);
    let (end_line, end_column) = checks::position(text, range.end);
    Diagnostic {
        path: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
        line,
        column: Some(column),
        end: Some((end_line, end_column)),
        severity,
        message: message.to_owned(),
        source: t("dotenv.source"),
        rule: None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/service.rs"]
mod tests;
