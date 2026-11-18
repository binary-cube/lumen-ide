//! A project's consoles checked against their sources' schemas (plan 39k): an unknown table,
//! an unknown column on a sure table, an ambiguous column — only what's certain
//! ([`database::schema_check`]) — told through the project's diagnostics
//! ([`workbench_model::DiagnosticsFound`], source [`SOURCE`]), so the app shows them as its
//! own (squiggles, the scroll strip's marks, the status bar).
//!
//! A console's text is checked in the background (the pool `db-schema-index`) once typing
//! pauses ([`kernel::queue::Coalescing`]), never on each key, on the source's index as built
//! (asked for when there's none: nothing is told until it comes); again when the index
//! changes (read again, columns come) and when the text does. What a console's session made
//! (its temporary tables, an attached database) is never called unknown. A console closed
//! takes its findings with it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use database::schema_check::{self, CheckRequest, Problem};
use database::sql::{TextPosition, lex};
use kernel::queue::{Batch, Coalescing};
use workbench_model::{Diagnostic, DiagnosticsFound, DiagnosticsScope, Severity};

use crate::consoles::Consoles;
use crate::runtime::Runtime;

/// The diagnostics' source (apart from the server's errors after a run).
pub const SOURCE: &str = "sql-schema";

/// How long typing must pause before a text is checked.
const PAUSE: Duration = Duration::from_millis(400);
/// The longest a text waits while typing goes on.
const MAX_WAIT: Duration = Duration::from_secs(3);

/// A console's text as last told, and what was told of it.
struct Doc {
    text: Arc<str>,
    version: u64,
    /// The findings told (to tell only a change).
    told: Option<Vec<Diagnostic>>,
}

struct State {
    docs: HashMap<PathBuf, Doc>,
    queue: Coalescing<PathBuf>,
    /// A job is taking the queue's batches.
    draining: bool,
}

/// A project's consoles' checks (see the module's doc).
pub struct Checks {
    state: Mutex<State>,
    runtime: Mutex<Runtime>,
    pause: Mutex<Duration>,
    /// Checked texts (the tests count them).
    runs: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for Checks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checks").finish_non_exhaustive()
    }
}

impl Checks {
    pub fn new(runtime: Runtime) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                docs: HashMap::new(),
                queue: Coalescing::new(PAUSE, MAX_WAIT, MAX_WAIT),
                draining: false,
            }),
            runtime: Mutex::new(runtime),
            pause: Mutex::new(PAUSE),
            runs: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Its work runs on `runtime` from now (the tests').
    pub fn set_runtime(&self, runtime: Runtime) {
        *self.runtime.lock().unwrap_or_else(PoisonError::into_inner) = runtime;
    }

    /// Typing must pause `pause` before a text is checked (the tests': none).
    pub fn set_pause(&self, pause: Duration) {
        *self.pause.lock().unwrap_or_else(PoisonError::into_inner) = pause;
        let mut state = self.lock();
        state.queue = Coalescing::new(pause, MAX_WAIT.max(pause), MAX_WAIT.max(pause));
    }

    /// How many texts were checked.
    pub fn runs(&self) -> usize {
        self.runs.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Console file `path` now holds `text` (version `version`): checked once typing pauses.
    pub fn schedule(&self, consoles: &Arc<Consoles>, path: &Path, text: Arc<str>, version: u64) {
        let now = self.runtime().now();
        {
            let mut state = self.lock();
            match state.docs.get_mut(path) {
                Some(doc) if doc.version == version => return,
                Some(doc) => {
                    doc.text = text;
                    doc.version = version;
                }
                None => {
                    state.docs.insert(path.to_path_buf(), Doc { text, version, told: None });
                }
            }
            state.queue.push(path.to_path_buf(), now);
        }
        self.drain(consoles);
    }

    /// Every console known checked again (an index or columns arrived).
    pub fn recheck(&self, consoles: &Arc<Consoles>) {
        let now = self.runtime().now();
        {
            let mut state = self.lock();
            if state.docs.is_empty() {
                return;
            }
            state.queue.push_all(now);
        }
        self.drain(consoles);
    }

    /// Console file `path` closed (or moved): its findings go.
    pub fn forget(&self, consoles: &Consoles, path: &Path) {
        let told = self.lock().docs.remove(path).and_then(|d| d.told);
        if told.is_some_and(|t| !t.is_empty()) {
            publish(consoles, path, Vec::new());
        }
    }

    /// Everything forgotten ("Clear and Rebuild Caches"): what's open is checked again when
    /// next told.
    pub fn purge(&self, consoles: &Consoles) {
        let docs: Vec<(PathBuf, Option<Vec<Diagnostic>>)> =
            self.lock().docs.drain().map(|(p, d)| (p, d.told)).collect();
        for (path, told) in docs {
            if told.is_some_and(|t| !t.is_empty()) {
                publish(consoles, &path, Vec::new());
            }
        }
    }

    /// A text is waiting or being checked (the snapshots wait for it).
    pub fn busy(&self) -> bool {
        let state = self.lock();
        state.draining || !state.queue.is_empty()
    }

    fn runtime(&self) -> Runtime {
        self.runtime.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Takes the queue's batches on the pool, until it's empty.
    fn drain(&self, consoles: &Arc<Consoles>) {
        {
            let mut state = self.lock();
            if std::mem::replace(&mut state.draining, true) {
                return;
            }
        }
        let weak = Arc::downgrade(consoles);
        let runtime = self.runtime();
        runtime.spawn_index(move || {
            let Some(consoles) = weak.upgrade() else { return };
            consoles.checks().run_queue(&consoles);
        });
    }

    fn run_queue(&self, consoles: &Arc<Consoles>) {
        let runtime = self.runtime();
        loop {
            let now = runtime.now();
            let batch = {
                let mut state = self.lock();
                match state.queue.take(now) {
                    Some(batch) => batch,
                    None => match state.queue.ready_at(now) {
                        Some(at) => {
                            drop(state);
                            let wait = at.saturating_duration_since(now);
                            std::thread::sleep(wait.min(MAX_WAIT));
                            continue;
                        }
                        None => {
                            state.draining = false;
                            return;
                        }
                    },
                }
            };
            let paths: Vec<PathBuf> = match batch {
                Batch::All => self.lock().docs.keys().cloned().collect(),
                Batch::Keys(keys) => keys,
            };
            for path in paths {
                self.check_one(consoles, &path);
            }
        }
    }

    /// Checks `path`'s text as last told, and tells what changed.
    fn check_one(&self, consoles: &Arc<Consoles>, path: &Path) {
        let Some((text, version)) =
            self.lock().docs.get(path).map(|d| (Arc::clone(&d.text), d.version))
        else {
            return;
        };
        self.runs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(found) = findings(consoles, path, &text) else { return };
        let mut state = self.lock();
        let Some(doc) = state.docs.get_mut(path) else { return };
        // A newer text meanwhile: its own check comes.
        if doc.version != version {
            return;
        }
        if doc.told.as_ref() == Some(&found) {
            return;
        }
        let first = doc.told.is_none();
        doc.told = Some(found.clone());
        drop(state);
        if first && found.is_empty() {
            return;
        }
        publish(consoles, path, found);
        consoles.indexes().wake_app();
    }
}

/// Console `path`'s findings in `text` (`None`: nothing can be said yet — no index, not a
/// console, no SQL).
fn findings(consoles: &Arc<Consoles>, path: &Path, text: &str) -> Option<Vec<Diagnostic>> {
    let spec = consoles.spec_of_path(path)?;
    let access = consoles.access()?;
    let brief = access.source(spec.source)?;
    if !brief.dialect {
        return Some(Vec::new());
    }
    let indexes = consoles.indexes();
    let database = spec.database.as_deref();
    let now = consoles.runtime().now();
    let index = indexes.index(access.as_ref(), spec.source, database, now)?;
    let flavor = brief.flavor;
    let tokens = lex(text, flavor);
    let (known, attached) = consoles.runner_of(spec.id).map(|r| r.made()).unwrap_or_default();
    let found = schema_check::check(&CheckRequest {
        text,
        tokens: &tokens,
        flavor,
        index: &index,
        known: &known,
        attached,
    });
    if !found.missing.is_empty() {
        indexes.request(spec.source, database, &found.missing);
    }
    let relative = path.strip_prefix(consoles.root()).ok()?.to_path_buf();
    Some(
        found
            .problems
            .iter()
            .map(|p| {
                // A quoted name: its word, inside the quote.
                let start = match text.get(p.range.clone()).and_then(|t| t.chars().next()) {
                    Some('"' | '`' | '[') => p.range.start + 1,
                    _ => p.range.start,
                };
                let at = TextPosition::of(text, start);
                Diagnostic {
                    path: relative.clone(),
                    line: at.line as u32,
                    column: Some(at.column as u32),
                    end: None,
                    severity: if p.problem.is_warning() {
                        Severity::Warning
                    } else {
                        Severity::Error
                    },
                    message: message(&p.problem),
                    source: i18n::t("console.check_source"),
                    rule: None,
                }
            })
            .collect(),
    )
}

/// What a problem says.
pub fn message(problem: &Problem) -> String {
    match problem {
        Problem::UnknownTable { name } => {
            i18n::tr!("console.check_unknown_table", name = name.as_str())
        }
        Problem::UnknownColumn { column, table } => i18n::tr!(
            "console.check_unknown_column",
            column = column.as_str(),
            table = table.as_str()
        ),
        Problem::Ambiguous { column, tables } => i18n::tr!(
            "console.check_ambiguous",
            column = column.as_str(),
            tables = tables.join(", ").as_str()
        ),
    }
}

/// Tells `path`'s findings (replacing those told before) on the project's bus.
fn publish(consoles: &Consoles, path: &Path, diagnostics: Vec<Diagnostic>) {
    let Ok(relative) = path.strip_prefix(consoles.root()) else { return };
    consoles.events().publish(&DiagnosticsFound {
        source: SOURCE.to_owned(),
        scope: DiagnosticsScope::File(relative.to_path_buf()),
        diagnostics,
    });
}

/// Rechecks every console of `consoles` when its indexes change (held weakly: the checks
/// never keep the project's consoles alive).
pub fn follow_indexes(consoles: &Arc<Consoles>) {
    let weak: Weak<Consoles> = Arc::downgrade(consoles);
    consoles.indexes().on_arrival(Arc::new(move || {
        if let Some(consoles) = weak.upgrade() {
            consoles.checks().recheck(&consoles);
        }
    }));
}

#[cfg(test)]
#[path = "../tests/unit/checks.rs"]
mod tests;
