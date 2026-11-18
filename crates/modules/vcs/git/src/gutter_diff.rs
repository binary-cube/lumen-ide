//! The lines changed since the last commit (the gutter bars), found in the background: the
//! diff against the committed text no longer runs on the UI thread on every keystroke.
//! Requests for the same document are coalesced (only the latest version counts); the
//! thread ends when idle.

use std::sync::Arc;

use document::TextSnapshot;
use kernel::DocumentId;
use version_control::LineChange;
use version_control::application::changed_lines;

/// What to compare: a document version with its committed text (`None`: not committed,
/// every line is new).
pub(crate) struct Job {
    pub(crate) doc: DocumentId,
    pub(crate) version: u64,
    pub(crate) base: Option<Arc<str>>,
    pub(crate) text: TextSnapshot,
}

/// The changed lines of a document version.
pub(crate) type Diffed = (DocumentId, u64, Vec<LineChange>);

pub(crate) struct GutterDiff {
    jobs: kernel::Latest<DocumentId, Job, Diffed>,
}

impl std::fmt::Debug for GutterDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GutterDiff").finish_non_exhaustive()
    }
}

impl GutterDiff {
    /// `wake`: wakes the UI loop when a result is ready.
    pub(crate) fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        let jobs = kernel::Latest::new(
            "gutter-diff",
            |job: Job| {
                let text = job.text.slice(0..job.text.len());
                let lines = job.text.line_count();
                (job.doc, job.version, changed_lines(job.base.as_deref(), &text, lines))
            },
            wake,
        );
        Some(Self { jobs })
    }

    pub(crate) fn request(&self, job: Job) {
        self.jobs.request(job.doc, job);
    }

    /// The results found since the last call.
    pub(crate) fn take(&self) -> Vec<Diffed> {
        self.jobs.take()
    }
}
