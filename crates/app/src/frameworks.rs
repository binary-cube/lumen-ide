//! The frameworks of the open project: detected by their modules (from `composer.json`,
//! `package.json`…), corrected by the project's choices (`<project dir>/project.toml`,
//! `[frameworks]`), at most one per family. Only the active ones have their support
//! created; it goes away with the project.
//!
//! Each active one has its own generation: the project's bus tells which files changed,
//! and only a framework watching one of them ([`FrameworkSupport::watches`]) counts it —
//! its model is then made again in the background ([`FrameworkSupport::prepare`], one at a
//! time, the changes meanwhile merged), never on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use code_intelligence::{
    Framework, FrameworkContext, FrameworkSupport, LanguageIntelligence, Project,
};
use kernel::LanguageId;
use settings_model::ProjectFrameworksStore;

/// The work threads making the frameworks' models (named in the Task Manager).
pub const FRAMEWORKS_THREAD: &str = "frameworks";

/// An answer on the UI thread longer than this is counted as slow (Task Manager).
pub const BUDGET: std::time::Duration = std::time::Duration::from_millis(20);

/// [`Active::prepared`] before its model was first made.
const NONE_PREPARED: u64 = u64::MAX;

/// An active framework: its support for the project and its own generation.
#[derive(Debug, Clone)]
pub struct Active {
    pub id: &'static str,
    pub name: &'static str,
    pub support: Arc<dyn FrameworkSupport>,
    /// Its own generation ([`FrameworkContext::own`]).
    own: Arc<AtomicU64>,
    /// The generation its model was last made for in the background ([`NONE_PREPARED`]:
    /// not yet): what the UI thread asks with, so it finds the model made and never makes
    /// it (reading every route file after a save) — the newer one comes when it's ready.
    prepared: Arc<AtomicU64>,
    /// Its model is being made (one at a time: a change meanwhile makes it again after).
    preparing: Arc<AtomicBool>,
    /// Its answers on the UI thread longer than [`BUDGET`].
    slow: Arc<AtomicU64>,
}

impl Active {
    fn new(framework: &dyn Framework, support: Arc<dyn FrameworkSupport>) -> Self {
        Self {
            id: framework.id(),
            name: framework.name(),
            support,
            own: Arc::default(),
            prepared: Arc::new(AtomicU64::new(NONE_PREPARED)),
            preparing: Arc::default(),
            slow: Arc::default(),
        }
    }

    /// The generation the UI thread asks with: the model's last made in the background (its
    /// own before the first is made).
    pub fn answering(&self) -> u64 {
        match self.prepared.load(Ordering::Acquire) {
            NONE_PREPARED => self.own(),
            prepared => prepared,
        }
    }

    pub fn own(&self) -> u64 {
        self.own.load(Ordering::Relaxed)
    }

    pub fn slow(&self) -> u64 {
        self.slow.load(Ordering::Relaxed)
    }

    /// An answer that took `took` on the UI thread: counted (and said) when over budget.
    pub fn answered(&self, took: std::time::Duration) {
        if took > BUDGET {
            self.slow.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(framework = self.id, ?took, "a framework's answer was slow");
        }
    }
}

/// What the frameworks' background work reads the project with: its root, its languages'
/// levels and intelligence (the disk read, never the editor's tabs), its generations.
pub struct Background {
    pub root: PathBuf,
    pub levels: Vec<(LanguageId, String)>,
    pub languages: Vec<Arc<dyn LanguageIntelligence>>,
    /// The project's file generations (any file, files made or removed).
    pub generations: Arc<dyn Fn() -> (u64, u64) + Send + Sync>,
    /// The project's index: what a framework knows the class of goes there for the
    /// languages' inference (`FrameworkSupport::named_types`).
    pub index: Arc<std::sync::RwLock<code_intelligence::SymbolIndex>>,
}

impl std::fmt::Debug for Background {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Background").field("root", &self.root).finish_non_exhaustive()
    }
}

impl Background {
    /// `f` with framework `active`'s context over the project (no document: its model).
    fn with_context<R>(&self, active: &Active, f: impl FnOnce(&FrameworkContext<'_>) -> R) -> R {
        self.with_document(active, None, f)
    }

    /// `f` with framework `active`'s context over document `doc` (its text, the one read
    /// for its path) or none.
    fn with_document<R>(
        &self,
        active: &Active,
        doc: Option<&Document>,
        f: impl FnOnce(&FrameworkContext<'_>) -> R,
    ) -> R {
        let (generation, structure) = (self.generations)();
        let open = |path: &Path| doc.filter(|d| d.path == path).map(|d| d.text.to_string());
        crate::project_disk::with_project(
            Some(&self.root),
            &self.levels,
            &self.languages,
            &open,
            |project| {
                f(&FrameworkContext {
                    project,
                    path: doc.map_or(Path::new(""), |d| d.path.as_path()),
                    language: doc.map_or("", |d| d.language.as_str()),
                    text: doc.map_or("", |d| &*d.text),
                    offset: 0,
                    generation,
                    structure,
                    own: active.own(),
                })
            },
        )
    }
}

/// A document the frameworks check: its file, its language, its text.
#[derive(Debug, Clone)]
pub struct Document {
    pub path: PathBuf,
    pub language: String,
    pub text: Arc<str>,
}

/// Whether `found` differs from what was last told for `key` (kept as told; a file's first
/// look is always told, so what an earlier check left is replaced).
fn changed(
    told: &Mutex<Told>,
    key: (String, PathBuf),
    found: &[workbench_model::Diagnostic],
) -> bool {
    let mut told = told.lock().unwrap_or_else(PoisonError::into_inner);
    if told.get(&key).is_some_and(|before| before.as_slice() == found) {
        return false;
    }
    told.insert(key, found.to_vec());
    true
}

/// `problems` found in `text` (file `relative`) as the editor's diagnostics.
fn diagnostics(
    name: &str,
    relative: &Path,
    text: &str,
    problems: &[code_intelligence::FrameworkProblem],
) -> Vec<workbench_model::Diagnostic> {
    let at = |offset: usize| {
        let offset = text.floor_char_boundary(offset.min(text.len()));
        let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
        (kernel::text::line_of(text, offset), text[line_start..offset].chars().count() as u32)
    };
    problems
        .iter()
        .map(|p| {
            let (line, column) = at(p.range.start);
            workbench_model::Diagnostic {
                path: relative.to_path_buf(),
                line,
                column: Some(column),
                end: Some(at(p.range.end)),
                severity: match p.level {
                    code_intelligence::ProblemLevel::Error => workbench_model::Severity::Error,
                    code_intelligence::ProblemLevel::Warning => workbench_model::Severity::Warning,
                },
                message: p.message.clone(),
                source: name.to_owned(),
                rule: None,
            }
        })
        .collect()
}

/// `active`'s model made again in the background, unless it's being made: then once more
/// after (it sees the newer generation).
fn prepare(active: &Active, background: &Arc<Background>) {
    if active.preparing.swap(true, Ordering::AcqRel) {
        return;
    }
    let (active, background) = (active.clone(), Arc::clone(background));
    kernel::work::run(FRAMEWORKS_THREAD, move || {
        loop {
            let own = active.own();
            let named = background.with_context(&active, |at| {
                active.support.prepare(at);
                active.prepared.store(at.own, Ordering::Release);
                active.support.named_types(at)
            });
            if !named.is_empty()
                && let Ok(mut index) = background.index.write()
            {
                for (table, names) in named {
                    index.set_named(table, names);
                }
            }
            active.preparing.store(false, Ordering::Release);
            // A change came while it was made: again, unless another run took it.
            if active.own() == own || active.preparing.swap(true, Ordering::AcqRel) {
                break;
            }
        }
    });
}

/// What follows the project's changes: the paths changed (`true`: every file may have).
type Changed = Arc<dyn Fn(&[&Path], bool) + Send + Sync>;

/// The diagnostics last announced, by source and file (the files checked: the documents
/// edited).
type Told = std::collections::HashMap<(String, PathBuf), Vec<workbench_model::Diagnostic>>;

#[derive(Debug, Default)]
pub struct ProjectFrameworks {
    /// (id, where it was detected from).
    pub detected: Vec<(String, String)>,
    /// The project's choices: (id, on).
    pub choices: Vec<(String, bool)>,
    /// The active frameworks (shared with what follows the project's changes).
    active: Arc<Mutex<Vec<Active>>>,
    /// How their models are made in the background (set by [`Self::follow`]).
    background: Option<Arc<Background>>,
    /// The document to check next, and whether a check runs (the latest asked wins).
    to_check: Arc<Mutex<Option<(Document, kernel::EventBus)>>>,
    checking: Arc<AtomicBool>,
    /// What was last announced for each framework and file (announced again only when it
    /// changes: a keystroke that changes nothing of it rebuilds no marks).
    told: Arc<Mutex<Told>>,
    /// The project's bus followed (gone with these).
    subscriptions: Vec<kernel::Subscription>,
}

impl ProjectFrameworks {
    /// The project's frameworks, with its choices from `store`.
    pub fn load(
        store: &dyn ProjectFrameworksStore,
        root: &Path,
        frameworks: &[Arc<dyn Framework>],
        project: &Project<'_>,
    ) -> Self {
        // The detectors read the same few files (`composer.json`, `package.json`): once.
        let cache = std::cell::RefCell::new(std::collections::HashMap::new());
        let read = |path: &Path| -> Option<String> {
            cache
                .borrow_mut()
                .entry(path.to_path_buf())
                .or_insert_with(|| (project.read)(path))
                .clone()
        };
        let detecting = Project { read: &read, ..*project };
        let mut this = Self {
            detected: frameworks
                .iter()
                .filter_map(|f| Some((f.id().to_owned(), f.detect(&detecting)?)))
                .collect(),
            choices: store.load(root),
            ..Self::default()
        };
        this.resolve(frameworks, project);
        for (id, source) in &this.detected {
            tracing::info!(framework = %id, %source, "framework detected");
        }
        this
    }

    /// Turns framework `id` on or off for the project (saved), e.g. "it's Laravel, not
    /// Symfony". Turning one on takes the others of its family off.
    pub fn set(
        &mut self,
        store: &dyn ProjectFrameworksStore,
        root: &Path,
        frameworks: &[Arc<dyn Framework>],
        project: &Project<'_>,
        id: &str,
        on: bool,
    ) {
        self.choices =
            code_intelligence::choose_framework(frameworks, &self.detected, &self.choices, id, on);
        if let Err(e) = store.save(root, &self.choices) {
            tracing::warn!("can't save the project's frameworks: {e}");
        }
        self.resolve(frameworks, project);
    }

    /// The active ones (those still active kept, with their caches and generation); a new
    /// one's model made in the background.
    fn resolve(&mut self, frameworks: &[Arc<dyn Framework>], project: &Project<'_>) {
        let ids = code_intelligence::active_frameworks(frameworks, &self.detected, &self.choices);
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = std::mem::take(&mut *active);
        let mut new = Vec::new();
        *active = ids
            .into_iter()
            .filter_map(|id| {
                if let Some(kept) = previous.iter().find(|p| p.id == id) {
                    return Some(kept.clone());
                }
                let framework = frameworks.iter().find(|f| f.id() == id)?;
                let made = Active::new(framework.as_ref(), framework.activate(project));
                new.push(made.clone());
                Some(made)
            })
            .collect();
        drop(active);
        if let Some(background) = &self.background {
            for active in &new {
                prepare(active, background);
            }
        }
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.active().iter().any(|a| a.id == id)
    }

    /// The active frameworks, in order (cheap: their supports are shared).
    pub fn active(&self) -> Vec<Active> {
        self.active.lock().map(|a| a.clone()).unwrap_or_default()
    }

    /// Checks `doc` in the background — every active framework's problems in it, announced
    /// on `events` as its diagnostics (they replace its previous ones); a newer document
    /// asked meanwhile is checked instead of the ones waiting.
    pub fn check(&self, doc: Document, events: &kernel::EventBus) {
        let Some(background) = self.background.clone() else { return };
        if let Ok(mut next) = self.to_check.lock() {
            *next = Some((doc, events.clone()));
        }
        if self.checking.swap(true, Ordering::AcqRel) {
            return;
        }
        let (active, next, checking, told) = (
            Arc::clone(&self.active),
            Arc::clone(&self.to_check),
            Arc::clone(&self.checking),
            Arc::clone(&self.told),
        );
        kernel::work::run(FRAMEWORKS_THREAD, move || {
            loop {
                let taken = next.lock().ok().and_then(|mut n| n.take());
                let Some((doc, events)) = taken else {
                    checking.store(false, Ordering::Release);
                    // One asked as it ended: checked by another run, unless one took it.
                    let waiting = next.lock().is_ok_and(|n| n.is_some());
                    if !waiting || checking.swap(true, Ordering::AcqRel) {
                        break;
                    }
                    continue;
                };
                let Ok(relative) = doc.path.strip_prefix(&background.root) else { continue };
                let frameworks = active.lock().map(|a| a.clone()).unwrap_or_default();
                for framework in &frameworks {
                    let problems = background.with_document(framework, Some(&doc), |at| {
                        framework.support.diagnostics(at)
                    });
                    let source = format!("framework-{}", framework.id);
                    let found = diagnostics(framework.name, relative, &doc.text, &problems);
                    if !changed(&told, (source.clone(), relative.to_path_buf()), &found) {
                        continue;
                    }
                    events.publish(&workbench_model::DiagnosticsFound {
                        source,
                        scope: workbench_model::DiagnosticsScope::File(relative.to_path_buf()),
                        diagnostics: found,
                    });
                }
            }
        });
    }

    /// Follows the project's changes on `events`: a framework watching a file that changed
    /// counts it (its own generation) and has its model made again in the background — as
    /// each active one has now.
    pub fn follow(&mut self, events: &kernel::EventBus, background: Background) {
        use workspace::events::{
            EntriesChanged, EntriesDeleted, EntriesRescan, FileRenamed, FilesWritten,
        };
        let background = Arc::new(background);
        let on = {
            let (active, background) = (Arc::clone(&self.active), Arc::clone(&background));
            Arc::new(move |paths: &[&Path], all: bool| {
                let active = active.lock().map(|a| a.clone()).unwrap_or_default();
                let relative: Vec<String> = paths
                    .iter()
                    .filter_map(|p| p.strip_prefix(&background.root).ok())
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .collect();
                for framework in &active {
                    let patterns = framework.support.watches();
                    if all || relative.iter().any(|r| code_intelligence::watched(patterns, r)) {
                        framework.own.fetch_add(1, Ordering::Relaxed);
                        prepare(framework, &background);
                    }
                }
            })
        };
        let paths = |on: &Changed| {
            let on = Arc::clone(on);
            move |paths: &[PathBuf]| {
                on(&paths.iter().map(PathBuf::as_path).collect::<Vec<_>>(), false)
            }
        };
        let on: Changed = on;
        let (saved, written, changed, deleted, renamed, rescan) =
            (Arc::clone(&on), paths(&on), paths(&on), paths(&on), Arc::clone(&on), Arc::clone(&on));
        self.subscriptions = vec![
            events.subscribe(move |e: &document::events::FileSaved| saved(&[&e.path], false)),
            events.subscribe(move |e: &FilesWritten| written(&e.paths)),
            events.subscribe(move |e: &EntriesChanged| changed(&e.paths)),
            events.subscribe(move |e: &EntriesDeleted| deleted(&e.paths)),
            events.subscribe(move |e: &FileRenamed| renamed(&[&e.from, &e.to], false)),
            events.subscribe(move |_: &EntriesRescan| rescan(&[], true)),
        ];
        for active in self.active() {
            prepare(&active, &background);
        }
        self.background = Some(background);
    }
}

#[cfg(test)]
#[path = "../tests/unit/frameworks.rs"]
mod tests;
