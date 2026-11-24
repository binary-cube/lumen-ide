//! The editor's version control decorations, per project: the lines changed since the last
//! commit (the gutter's bars) and the blame annotations of the documents open, and the
//! branch shown beside the project's name. The committed texts and the blames are read in
//! the background (on the project's own pools, gone with it), the changed lines found on a
//! thread (only a document's latest version counts); a new `HEAD` (a commit, a checkout)
//! rereads them and is announced ([`HeadChanged`]). The workbench only asks and draws
//! ([`LineDecorator`]); the module's commands read the same state (the committed text to
//! revert to, the annotated commit) and show / hide the annotations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kernel::work::{self, Pool, Priority};
use kernel::{Cancel, DocumentId, EventBus, ProjectId};
use version_control::application::blame_annotations;
use version_control::{
    BlameHunk, ChangeKind, Commit, HeadChanged, LineChange, Repository, Revision,
};
use workbench_model::{
    DecoratorContext, DocumentDecorations, LineAnnotation, LineDecorator, LineMark, MarkKind,
    ProjectDecorations, ResourceUse, ShownDocument, WorkbenchHost,
};

use crate::gutter_diff::{GutterDiff, Job};

/// The committed versions read (the Task Manager's disk column, on Git's row).
static DISK: workbench_model::DiskIo = workbench_model::DiskIo::new();

/// Each open project's decorations (held by the workbench: gone with the project).
static PROJECTS: workbench_model::PerProject<Shared> = workbench_model::PerProject::new();

/// The width of the annotation column, in characters (`2026-10-20  Author name`).
const ANNOTATION_CHARS: usize = 30;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Decorates the documents of the projects in a repository.
#[derive(Debug)]
pub(crate) struct GitDecorator;

impl LineDecorator for GitDecorator {
    fn open(&self, project: &DecoratorContext<'_>) -> Option<Box<dyn ProjectDecorations>> {
        let repo = project.repository.clone()?;
        let shared = Arc::new(Shared {
            repo,
            events: project.events.clone(),
            wake: Arc::clone(&project.wake),
            cancel: Cancel::new(),
            reads: Pool::new("git-read", work::default_limit(), work::IDLE),
            blames: Pool::new("git-blame", 2, work::IDLE),
            state: Mutex::default(),
            dirty: AtomicBool::new(false),
            looking: AtomicBool::new(false),
            again: AtomicBool::new(false),
        });
        PROJECTS.insert(project.id, &shared);
        shared.look();
        Some(Box::new(GitDecorations { project: project.id, shared, diff: None }))
    }
}

/// A project's decorations, as the workbench holds them.
pub(crate) struct GitDecorations {
    project: ProjectId,
    shared: Arc<Shared>,
    /// The thread finding the documents' changed lines (started on the first request).
    diff: Option<GutterDiff>,
}

impl std::fmt::Debug for GitDecorations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitDecorations").field("project", &self.project).finish_non_exhaustive()
    }
}

impl Drop for GitDecorations {
    /// The project closed: what's still to be read is dropped, not read.
    fn drop(&mut self) {
        self.shared.cancel.cancel();
        PROJECTS.remove(self.project);
    }
}

/// What the workbench's object, the background reads and the module's commands share.
struct Shared {
    repo: Arc<dyn Repository>,
    events: EventBus,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Cancelled with the project.
    cancel: Cancel,
    /// The committed texts of the open documents (their gutter bars).
    reads: Pool,
    /// Blames (the annotations): heavier, two at a time.
    blames: Pool,
    state: Mutex<State>,
    /// Something read in the background since the last poll.
    dirty: AtomicBool,
    /// The repository's state is being read; another look was asked for meanwhile.
    looking: AtomicBool,
    again: AtomicBool,
}

#[derive(Debug, Default)]
struct State {
    /// `HEAD`'s id when the committed texts were read (a commit / checkout changes it).
    head: Option<String>,
    /// The branch shown.
    branch: Option<String>,
    /// `HEAD` on a commit, not a branch.
    detached: bool,
    /// An operation left in progress (a rebase, a merge), shown with the branch.
    operation: Option<version_control::Operation>,
    /// The repository's state was read at least once (the first read announces nothing).
    known: bool,
    /// A new `HEAD` found in the background, announced from the UI thread at the next poll.
    announce: Option<HeadChanged>,
    docs: HashMap<DocumentId, Doc>,
    /// When a document was last asked for (any: the editor drew). A document not asked for
    /// [`UNASKED`] before it is off screen; time with nothing drawn (a window left standing)
    /// doesn't count.
    last_asked: Option<std::time::Instant>,
}

/// A document's git state.
#[derive(Debug, Default)]
struct Doc {
    /// The committed text and the `HEAD` it was read at.
    base: Option<(String, Option<Arc<str>>)>,
    /// The `HEAD` whose text is being read.
    pending: Option<String>,
    /// The changed lines (and their marks), for the document version they were found for.
    changes: Vec<LineChange>,
    marks: Arc<Vec<LineMark>>,
    version: Option<u64>,
    /// The document version whose changed lines are being found (in the background).
    requested: Option<u64>,
    /// The blame annotations are shown in the gutter.
    annotate: bool,
    /// The blame of the committed file (read in the background), and the `HEAD` it's for.
    blame: Option<(String, Arc<Vec<BlameHunk>>)>,
    blame_pending: bool,
    /// Per line, its annotation; and the document version they match.
    annotations: Arc<Vec<Option<LineAnnotation>>>,
    annotations_version: Option<u64>,
    /// When it was last asked for (`sync`: its tab shown, or kept current).
    asked: Option<std::time::Instant>,
}

/// How long a document may go unasked (its tab off screen) before what was read for it goes.
const UNASKED: std::time::Duration = std::time::Duration::from_secs(300);

impl Doc {
    /// What was read for it let go (the committed text, the blame, its annotations), its
    /// bars and its annotate switch kept: asked again, it's read again in the background.
    fn let_go(&mut self) {
        if self.base.is_none() && self.blame.is_none() && self.pending.is_none() {
            return;
        }
        self.base = None;
        self.pending = None;
        self.blame = None;
        self.blame_pending = false;
        self.annotations = Arc::new(Vec::new());
        self.annotations_version = None;
        // Found again from the text read next (the bars shown meanwhile are these).
        self.version = None;
        self.requested = None;
    }

    /// The memory it holds (bytes): the committed text, the changed lines, the blame and
    /// its annotations.
    fn memory(&self) -> u64 {
        use std::mem::size_of;
        let base = self
            .base
            .as_ref()
            .map_or(0, |(head, text)| head.capacity() + text.as_ref().map_or(0, |t| t.len()));
        let changes = self.changes.capacity() * size_of::<LineChange>()
            + self.marks.capacity() * size_of::<LineMark>();
        let blame = self.blame.as_ref().map_or(0, |(head, hunks)| {
            head.capacity()
                + hunks.capacity() * size_of::<BlameHunk>()
                + hunks
                    .iter()
                    .map(|h| {
                        h.commit.id.capacity()
                            + h.commit.author.capacity()
                            + h.commit.summary.capacity()
                    })
                    .sum::<usize>()
        });
        let annotations = self.annotations.capacity() * size_of::<Option<LineAnnotation>>()
            + self.annotations.iter().flatten().map(|a| a.text.capacity()).sum::<usize>();
        (base + changes + blame + annotations) as u64
    }

    /// The commit annotated on `line` (0-based).
    fn commit_on(&self, line: usize) -> Option<&Commit> {
        let (_, blame) = self.blame.as_ref()?;
        let index = version_control::blame_lines(&self.changes, blame, line + 1).pop()??;
        Some(&blame.get(index)?.commit)
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Something was read: the workbench polls.
    fn read_done(&self) {
        self.dirty.store(true, Ordering::Release);
        (self.wake)();
    }

    /// Reads `HEAD`, the branch and an operation in progress on a background thread: what
    /// changes is taken at the next poll. Asked again meanwhile: read once more after.
    fn look(self: &Arc<Self>) {
        if self.looking.swap(true, Ordering::AcqRel) {
            self.again.store(true, Ordering::Release);
            return;
        }
        let (weak, repo) = (Arc::downgrade(self), Arc::clone(&self.repo));
        self.reads.spawn(Priority::Interactive, Some(&self.cancel), move || {
            let (head, branch) = (repo.head_id(), repo.branch());
            let (detached, operation) = (repo.detached(), repo.operation());
            let Some(shared) = weak.upgrade() else { return };
            let changed = {
                let mut state = shared.lock();
                let changed = !state.known
                    || head != state.head
                    || branch != state.branch
                    || detached != state.detached
                    || operation != state.operation;
                if changed && state.known {
                    // A commit, a checkout, a reset: the branches as they are now.
                    crate::refs_store::moved(repo.root());
                    let root = repo.root().to_path_buf();
                    let (head, branch) = (head.clone(), branch.clone());
                    state.announce = Some(HeadChanged { root, head, branch });
                }
                state.known = true;
                state.head = head;
                state.branch = branch;
                state.detached = detached;
                state.operation = operation;
                changed
            };
            shared.looking.store(false, Ordering::Release);
            if changed {
                shared.read_done();
            }
            if shared.again.swap(false, Ordering::AcqRel) {
                shared.look();
            }
        });
    }

    /// Reads the file's text at `HEAD` on a background thread.
    fn read_base(self: &Arc<Self>, head: &str, path: String, doc: DocumentId) {
        let (weak, repo, head) = (Arc::downgrade(self), Arc::clone(&self.repo), head.to_owned());
        self.reads.spawn(Priority::Interactive, Some(&self.cancel), move || {
            let text = repo.text_at(&Revision::Head, &path);
            if let Some(text) = &text {
                DISK.read(text.len() as u64);
            }
            let Some(shared) = weak.upgrade() else { return };
            {
                let mut state = shared.lock();
                let Some(d) = state.docs.get_mut(&doc) else { return };
                d.base = Some((head, text.map(Arc::from)));
                d.pending = None;
                d.version = None;
                d.requested = None;
            }
            shared.read_done();
        });
    }

    /// Reads the file's blame on a background thread.
    fn read_blame(self: &Arc<Self>, head: &str, path: String, doc: DocumentId) {
        let (weak, repo, head) = (Arc::downgrade(self), Arc::clone(&self.repo), head.to_owned());
        self.blames.spawn(Priority::Interactive, Some(&self.cancel), move || {
            let hunks = repo.blame(&path);
            let Some(shared) = weak.upgrade() else { return };
            {
                let mut state = shared.lock();
                let Some(d) = state.docs.get_mut(&doc) else { return };
                d.blame = Some((head, Arc::new(hunks)));
                d.blame_pending = false;
                d.annotations_version = None;
            }
            shared.read_done();
        });
    }
}

impl ProjectDecorations for GitDecorations {
    /// Reads the committed text when missing (in the background) and finds the changed
    /// lines again when the document changed; the blame too while it's annotated.
    fn sync(&mut self, shown: &ShownDocument<'_>) {
        let shared = Arc::clone(&self.shared);
        let mut state = shared.lock();
        let State { head, docs, last_asked, .. } = &mut *state;
        let Some(head) = head.as_deref() else { return };
        let now = std::time::Instant::now();
        *last_asked = Some(now);
        let doc = docs.entry(shown.id).or_default();
        doc.asked = Some(now);
        let fresh = doc.base.as_ref().is_some_and(|(h, _)| h == head);
        let reading = doc.pending.as_deref() == Some(head);
        if !fresh && !reading {
            let Some(path) = version_control::relative_path(shared.repo.root(), shown.path) else {
                return;
            };
            shared.read_base(head, path, shown.id);
            doc.pending = Some(head.to_owned());
        }
        if doc.annotate {
            let stale = doc.blame.as_ref().is_none_or(|(h, _)| h != head);
            if stale
                && !doc.blame_pending
                && let Some(path) = version_control::relative_path(shared.repo.root(), shown.path)
            {
                shared.read_blame(head, path, shown.id);
                doc.blame_pending = true;
            }
        }
        let version = shown.version;
        if doc.annotate
            && doc.version == Some(version)
            && doc.annotations_version != Some(version)
            && let Some((_, blame)) = &doc.blame
        {
            doc.annotations = Arc::new(annotations(&doc.changes, blame, shown.text.line_count()));
            doc.annotations_version = Some(version);
        }
        if doc.version == Some(version) || doc.requested == Some(version) {
            return;
        }
        if let Some((_, base)) = &doc.base {
            // Found on a thread: the bars follow a moment later, typing never waits.
            let job =
                Job { doc: shown.id, version, base: base.clone(), text: shown.text.snapshot() };
            doc.requested = Some(version);
            if self.diff.is_none() {
                self.diff = GutterDiff::start(Arc::clone(&shared.wake));
            }
            match &self.diff {
                Some(worker) => worker.request(job),
                None => tracing::warn!("can't start the gutter diff"),
            }
        }
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.shared.dirty.swap(false, Ordering::AcqRel);
        let announce = self.shared.lock().announce.take();
        if let Some(event) = announce {
            self.shared.events.publish(&event);
        }
        let diffed = self.diff.as_ref().map(GutterDiff::take).unwrap_or_default();
        if diffed.is_empty() {
            return changed;
        }
        let mut state = self.shared.lock();
        for (id, version, changes) in diffed {
            // Only for the version last asked for (a newer one is already requested).
            if let Some(doc) = state.docs.get_mut(&id).filter(|d| d.requested == Some(version)) {
                doc.marks = Arc::new(changes.iter().map(mark).collect());
                doc.changes = changes;
                doc.version = Some(version);
                changed = true;
            }
        }
        changed
    }

    /// A new `HEAD` (commit, checkout) rereads the committed texts; the branch shown is
    /// refreshed too, with an operation left in progress (another program's: SourceTree's,
    /// the terminal's). Looked at in the background: what changed comes with a poll.
    fn vcs_changed(&mut self) -> bool {
        self.shared.look();
        false
    }

    fn keep(&mut self, open: &[DocumentId]) {
        let mut state = self.shared.lock();
        state.docs.retain(|id, _| open.contains(id));
        // Not asked for a while the others were (its tab off screen): the committed text and
        // the blame go, read again when it's next asked for; the bars and the annotate
        // switch stay.
        let Some(last) = state.last_asked else { return };
        for doc in state.docs.values_mut() {
            if doc.asked.is_some_and(|at| last.saturating_duration_since(at) > UNASKED) {
                doc.let_go();
            }
        }
    }

    fn decorations(&self, doc: DocumentId) -> Option<DocumentDecorations> {
        let state = self.shared.lock();
        let d = state.docs.get(&doc)?;
        Some(DocumentDecorations {
            marks: Arc::clone(&d.marks),
            annotations: d.annotate.then(|| Arc::clone(&d.annotations)),
        })
    }

    fn annotation_chars(&self, doc: DocumentId) -> usize {
        let state = self.shared.lock();
        // A file outside the repository (nothing read for it) has none.
        let shown = state.head.is_some()
            && state
                .docs
                .get(&doc)
                .is_some_and(|d| d.annotate && (d.base.is_some() || d.pending.is_some()));
        if shown { ANNOTATION_CHARS } else { 0 }
    }

    /// The commit (id, author, date, message).
    fn annotation_tip(&self, doc: DocumentId, line: usize) -> Option<String> {
        let state = self.shared.lock();
        let commit = state.docs.get(&doc).filter(|d| d.annotate)?.commit_on(line)?;
        let (y, m, d) = crate::local_time::date(commit.time);
        Some(format!(
            "{} · {} · {y}-{m:02}-{d:02}\n{}",
            commit.short(),
            commit.author,
            commit.summary
        ))
    }

    /// The file compared with its last commit (as before the change actions, when they
    /// decline).
    fn mark_clicked(&self, doc: DocumentId, line: usize) -> Option<command_model::CommandId> {
        let state = self.shared.lock();
        let marked = state.docs.get(&doc)?.marks.iter().any(|m| m.lines.start == line);
        marked.then_some(crate::COMPARE_HEAD)
    }

    fn settled(&self, doc: DocumentId, version: u64) -> bool {
        let state = self.shared.lock();
        if !state.known {
            return false;
        }
        state
            .docs
            .get(&doc)
            .is_none_or(|d| d.pending.is_none() && (d.base.is_none() || d.version == Some(version)))
    }

    fn label(&self) -> Option<String> {
        let state = self.shared.lock();
        Some(label(state.branch.as_deref()?, state.detached, state.operation.as_ref()))
    }
}

/// What the top bar says of the repository: the branch; an operation in progress with the
/// branch it works on and its step (`feature · rebase 3/7`); `HEAD` on a commit said so.
pub(crate) fn label(
    branch: &str,
    detached: bool,
    operation: Option<&version_control::Operation>,
) -> String {
    use version_control::OperationKind as Kind;
    let Some(op) = operation else {
        return if detached {
            i18n::tr!("git.label_detached", at = branch)
        } else {
            branch.to_owned()
        };
    };
    let on = op.branch.as_deref().unwrap_or(branch);
    let what = match op.kind {
        Kind::Rebase => match op.step {
            Some((step, of)) => i18n::tr!("git.label_rebase_step", step = step, of = of),
            None => i18n::t("git.label_rebase"),
        },
        Kind::Merge => i18n::t("git.label_merge"),
        Kind::CherryPick => i18n::t("git.label_cherry_pick"),
        Kind::Revert => i18n::t("git.label_revert"),
        Kind::Bisect => i18n::t("git.label_bisect"),
        Kind::ApplyMailbox => i18n::t("git.label_apply"),
    };
    format!("{on} · {what}")
}

/// A changed group of lines' mark in the gutter.
fn mark(change: &LineChange) -> LineMark {
    let kind = match change.kind {
        ChangeKind::Added => MarkKind::Added,
        ChangeKind::Modified => MarkKind::Modified,
        ChangeKind::Deleted => MarkKind::Deleted,
    };
    LineMark { lines: change.lines.clone(), kind }
}

/// Per current line, its annotation in the gutter (from the blame use case): the date
/// and author, how old, whether it starts its commit's block.
fn annotations(
    changes: &[LineChange],
    blame: &[BlameHunk],
    line_count: usize,
) -> Vec<Option<LineAnnotation>> {
    blame_annotations(changes, blame, line_count)
        .into_iter()
        .map(|line| {
            let line = line?;
            let (y, m, d) = line.date;
            let author: String = line.author.chars().take(ANNOTATION_CHARS - 13).collect();
            Some(LineAnnotation {
                text: format!("{y}-{m:02}-{d:02}  {author}"),
                age: line.age,
                first: line.first,
            })
        })
        .collect()
}

/// The active project's decorations, while it's open.
fn project(host: &dyn WorkbenchHost) -> Option<Arc<Shared>> {
    let id = host.project_id()?;
    PROJECTS.get(id)
}

/// The active file's committed text (at `HEAD`, as the gutter compares it), once read.
pub(crate) fn committed_text(host: &dyn WorkbenchHost) -> Option<Arc<str>> {
    let shared = project(host)?;
    let doc = host.active_document()?;
    let state = shared.lock();
    let head = state.head.as_ref()?;
    let (at, text) = state.docs.get(&doc)?.base.as_ref()?;
    (at == head).then(|| text.clone()).flatten()
}

/// The commit (its id) annotated on the active line, while the annotations are shown.
pub(crate) fn annotated_commit(host: &dyn WorkbenchHost) -> Option<String> {
    let shared = project(host)?;
    let doc = host.active_document()?;
    let line = host.active_file()?.line;
    let state = shared.lock();
    Some(state.docs.get(&doc).filter(|d| d.annotate)?.commit_on(line)?.id.clone())
}

/// Shows / hides the blame annotations of the active file.
pub(crate) fn toggle_annotations(host: &dyn WorkbenchHost) -> bool {
    set_annotations(host, |on| !on)
}

/// Shows the blame annotations of the active file.
pub(crate) fn show_annotations(host: &dyn WorkbenchHost) -> bool {
    set_annotations(host, |_| true)
}

fn set_annotations(host: &dyn WorkbenchHost, wanted: impl FnOnce(bool) -> bool) -> bool {
    let (Some(shared), Some(doc)) = (project(host), host.active_document()) else {
        return false;
    };
    if host.active_file().is_none() {
        return false;
    }
    let mut state = shared.lock();
    let d = state.docs.entry(doc).or_default();
    d.annotate = wanted(d.annotate);
    d.annotations_version = None;
    true
}

/// What the decorations cost: the reads' threads and disk, what the open documents' git
/// state holds (every project's).
#[derive(Debug)]
pub(crate) struct DecorationsCost;

/// Some project is open (its decorations followed).
pub(crate) fn any_project() -> bool {
    !PROJECTS.all().is_empty()
}

impl workbench_model::ResourceReporter for DecorationsCost {
    fn report(&self) -> Vec<ResourceUse> {
        // No project: nothing read, nothing held.
        if !any_project() {
            return Vec::new();
        }
        let mut part = ResourceUse::new(i18n::t("tasks.git"), crate::MANIFEST.id.as_str());
        part.threads = vec!["git-read", "git-blame", "gutter-diff"];
        part.group = workbench_model::resources::PROGRAMS;
        DISK.report(&mut part);
        let projects = PROJECTS.all();
        let held: u64 =
            projects.iter().map(|s| s.lock().docs.values().map(Doc::memory).sum::<u64>()).sum();
        part.memory = (held > 0).then_some(held);
        vec![part]
    }
}

#[cfg(test)]
#[path = "../tests/unit/decorations.rs"]
mod tests;
