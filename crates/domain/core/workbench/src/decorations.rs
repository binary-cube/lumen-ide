//! Line decorations: what a module draws beside a document's lines — marks in the gutter
//! (the lines changed since the last commit) and an annotation per line (who changed it
//! last, when) — computed by it in the background for the documents shown, and a short
//! label for the project (the branch). The app opens the registered [`LineDecorator`]s with
//! each project, tells them what's shown, and draws what they have: it knows nothing of how
//! they're made.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use kernel::{DocumentId, EventBus, ProjectId};

/// How a group of lines is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkKind {
    Added,
    Modified,
    /// Lines were removed before `lines.start` (`lines` is empty).
    Deleted,
}

/// A mark beside lines of the current text (0-based, end exclusive): a bar in the gutter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMark {
    pub lines: Range<usize>,
    pub kind: MarkKind,
}

/// A line's annotation in the gutter: its text and how old it is (0 = the newest, 1 = the
/// oldest; newer lines are drawn more prominently).
#[derive(Debug, Clone, PartialEq)]
pub struct LineAnnotation {
    pub text: String,
    pub age: f32,
    /// The first line of its block (the text is drawn only there).
    pub first: bool,
}

/// A document's decorations, as last computed (shared: taking them copies nothing).
#[derive(Debug, Clone, Default)]
pub struct DocumentDecorations {
    /// The marks (sorted).
    pub marks: Arc<Vec<LineMark>>,
    /// Per line, its annotation, while they're shown (`None`: hidden).
    pub annotations: Option<Arc<Vec<Option<LineAnnotation>>>>,
}

/// A document open in the editor, as a decorator sees it.
#[derive(Debug, Clone, Copy)]
pub struct ShownDocument<'a> {
    pub id: DocumentId,
    pub path: &'a Path,
    pub version: u64,
    /// Its text (a snapshot of it is cheap and usable on a thread).
    pub text: &'a document::TextBuffer,
}

/// What a decorator is opened with, for a project.
pub struct DecoratorContext<'a> {
    /// The project's identity (new at each opening).
    pub id: ProjectId,
    pub root: &'a Path,
    /// The project's events.
    pub events: &'a EventBus,
    /// Wakes the app from a background thread: it then calls [`ProjectDecorations::poll`].
    pub wake: Arc<dyn Fn() + Send + Sync>,
    /// The project's repository, if it is in one.
    pub repository: Option<Arc<dyn version_control::Repository>>,
}

impl std::fmt::Debug for DecoratorContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoratorContext").field("root", &self.root).finish_non_exhaustive()
    }
}

/// The extension point: a module decorating the documents of a project (the git module:
/// the changed lines, the blame).
pub trait LineDecorator: Send + Sync + std::fmt::Debug {
    /// Starts decorating the project's documents: `None` if it has nothing to show there.
    /// What it returns lives as long as the project (its work ends with it).
    fn open(&self, project: &DecoratorContext<'_>) -> Option<Box<dyn ProjectDecorations>>;
}

/// A decorator's work for one project.
pub trait ProjectDecorations: Send + std::fmt::Debug {
    /// Before a frame, for each document shown in the editor: what's missing or out of date
    /// is asked for in the background (cheap when nothing changed; never waits).
    fn sync(&mut self, doc: &ShownDocument<'_>);
    /// After a wake: takes what the background work found. `true` if something changed (a
    /// redraw).
    fn poll(&mut self) -> bool;
    /// The project's version control data changed (inside `.git`): what's committed may
    /// differ. `true` if it did (a redraw).
    fn vcs_changed(&mut self) -> bool {
        false
    }
    /// Only these documents are still open: the others are forgotten.
    fn keep(&mut self, open: &[DocumentId]);
    /// Document `doc`'s decorations, as last computed.
    fn decorations(&self, doc: DocumentId) -> Option<DocumentDecorations>;
    /// The width the annotations of `doc` take in the gutter, in characters (0: none shown).
    fn annotation_chars(&self, doc: DocumentId) -> usize {
        let _ = doc;
        0
    }
    /// The tooltip of `doc`'s annotation on `line` (0-based), while they're shown.
    fn annotation_tip(&self, doc: DocumentId, line: usize) -> Option<String> {
        let _ = (doc, line);
        None
    }
    /// What a click on `doc`'s mark starting at `line` (0-based) runs when no module's
    /// [`crate::GutterAction`] took it (git: the file compared with its last commit).
    fn mark_clicked(&self, doc: DocumentId, line: usize) -> Option<command_model::CommandId> {
        let _ = (doc, line);
        None
    }
    /// Nothing is being computed for `doc` at `version` (snapshots wait for it).
    fn settled(&self, doc: DocumentId, version: u64) -> bool;
    /// A short label for the project, beside its name in the top bar (the branch).
    fn label(&self) -> Option<String> {
        None
    }
}
