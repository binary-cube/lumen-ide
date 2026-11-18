//! Extracting the active document's symbols in the background: the full parse of the file
//! (for the symbol index) no longer runs on the UI thread on every keystroke. Requests for
//! the same document are coalesced (only the latest version counts); the thread ends when
//! idle.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use code_intelligence::{Deprecated, LanguageIntelligence, LevelIssue, SymbolIndex};
use document::TextSnapshot;
use kernel::DocumentId;

/// What to analyse: a document version, where its symbols go, and the language level to
/// check it against.
pub struct Job {
    pub doc: DocumentId,
    pub version: u64,
    pub path: PathBuf,
    pub text: TextSnapshot,
    pub intel: Arc<dyn LanguageIntelligence>,
    pub index: Arc<RwLock<SymbolIndex>>,
    /// The chosen level (e.g. `"7.4"`), if the language has levels.
    pub level: Option<String>,
    /// The project's index is complete (built, not indexing): the document is also checked
    /// against it (an unknown class is only said then).
    pub index_ready: bool,
    /// The project's libraries are excluded: a class not found may just be in them.
    pub dependencies_left_out: bool,
}

/// A finished analysis: the version whose symbols are in the index, and the features
/// newer than the level.
#[derive(Debug)]
pub struct Analysis {
    pub doc: DocumentId,
    pub version: u64,
    pub level: Option<String>,
    pub issues: Vec<LevelIssue>,
    /// The deprecated names used in the text.
    pub deprecated: Vec<Deprecated>,
    /// What the text declares but never uses.
    pub unused: Vec<code_intelligence::Unused>,
    /// What the text needs the project to declare (e.g. `ext-http`).
    pub requirements: Vec<code_intelligence::Requirement>,
    /// What's wrong knowing the project (with the index complete).
    pub problems: Vec<code_intelligence::Problem>,
    /// Whether it was checked against the complete index.
    pub index_ready: bool,
}

pub struct SymbolWorker {
    jobs: kernel::Latest<DocumentId, Job, Option<Analysis>>,
    /// The last request sent (so the same version, level and index state aren't requested
    /// twice).
    requested: Option<(DocumentId, u64, Option<String>, bool)>,
}

impl std::fmt::Debug for SymbolWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymbolWorker").field("requested", &self.requested).finish_non_exhaustive()
    }
}

impl SymbolWorker {
    /// `wake`: wakes the UI loop when a result is ready.
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        let jobs = kernel::Latest::new(
            "symbols",
            |job: Job| {
                // A document that makes a language's analysis panic is skipped (said here);
                // the other documents keep being analysed.
                let (doc, path) = (job.doc, job.path.clone());
                let analysed =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| analyse(job)));
                if analysed.is_err() {
                    tracing::error!(%doc, path = %path.display(), "symbol analysis panicked");
                }
                analysed.ok()
            },
            wake,
        );
        Some(Self { jobs, requested: None })
    }

    /// Requests an analysis (ignored if the same version and level are already requested).
    pub fn request(&mut self, job: Job) {
        let key = (job.doc, job.version, job.level.clone(), job.index_ready);
        if self.requested.as_ref() == Some(&key) {
            return;
        }
        self.requested = Some(key);
        self.jobs.request(job.doc, job);
    }

    /// Requests an analysis again whatever was asked before (the document unchanged, the
    /// project's declarations changed: checked again against them).
    pub fn recheck(&mut self, job: Job) {
        self.requested = Some((job.doc, job.version, job.level.clone(), job.index_ready));
        self.jobs.request(job.doc, job);
    }

    /// The analyses finished since the last call.
    pub fn finished(&self) -> impl Iterator<Item = Analysis> {
        self.jobs.take().into_iter().flatten()
    }
}

/// One document's symbols (into the index) and its analyses (one parse: the language's).
fn analyse(job: Job) -> Analysis {
    let text = job.text.slice(0..job.text.len());
    let found = job.intel.analyse(&text, job.level.as_deref());
    let mut symbols = found.symbols;
    symbols.language = job.intel.language().to_string().into();
    // A panic while it was held mustn't leave the index locked out for good (the readers
    // skip a poisoned lock).
    job.index.clear_poison();
    let mut index = job.index.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    index.set_file(&job.path, symbols);
    drop(index);
    // Checked knowing the project once it's indexed whole (read, not locked out: other
    // readers go on).
    let checked = if job.index_ready {
        let index = job.index.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut checked =
            job.intel.check(&text, &code_intelligence::Types { index: &index, path: &job.path });
        if job.dependencies_left_out {
            checked.problems.retain(|p| p.kind != "undefined_class");
        }
        checked
    } else {
        code_intelligence::Checked::default()
    };
    let mut deprecated = found.deprecated;
    deprecated.extend(checked.deprecated);
    deprecated.sort_by_key(|d| d.range.start);
    Analysis {
        doc: job.doc,
        version: job.version,
        level: job.level,
        issues: found.issues,
        deprecated,
        unused: found.unused,
        requirements: found.requirements,
        problems: checked.problems,
        index_ready: job.index_ready,
    }
}
