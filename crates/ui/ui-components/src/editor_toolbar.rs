//! A bar a module puts above the editor of the files it knows, as a small [`Viewer`]: a
//! query console's (Run, Stop, the transaction, the database and schema it runs on), the
//! offer to download a language's server (one with no height once there's nothing to say:
//! its [`EditorToolbar::height`] is asked at each layout). The
//! file stays a text file in the editor — undo, find, multiple cursors, saving — and the bar
//! adds what its module does with it.
//!
//! A module contributes an [`EditorToolbarProvider`]; when a file's tab opens, the app asks
//! each one for a bar ([`EditorToolbarProvider::open`]: a look at the path, nothing
//! connects). The app lays the bar out above the text (the text below it, never under it),
//! hands it the clicks, the hover and the menus' rows, draws its color on the tab as a
//! view's ([`EditorToolbar::accent`]), takes the tab's title, icon and the breadcrumb's start
//! from it, asks it before the tab closes ([`EditorToolbar::may_close`]), tells it when its
//! file is renamed or moved ([`EditorToolbar::moved`]: it follows, or the providers are
//! asked again), and marks the
//! lines it says in the gutter ([`EditorToolbar::marks`]: a statement running, done or
//! failed) and in the text ([`EditorToolbar::highlight`]: the statement Run would run).
//!
//! [`Viewer`]: crate::Viewer

use std::fmt::Debug;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use icons::Icons;
use kernel::{Point, ProjectId, Rect, Rgba};
use render_api::Scene;
use text_layout::TextSystem;

use crate::palette::Palette;
use crate::tooltip::Tip;
use crate::viewer::{ViewMenuEntry, ViewerAction, ViewerCursor};

/// The bar's height when it doesn't say (logical pixels).
pub const HEIGHT: f32 = 40.0;

/// What a provider is asked with: the file, its language, the project it's in.
pub struct ToolbarContext<'a> {
    /// The file (absolute).
    pub path: &'a Path,
    /// Its language (its id, as detected), when it's a text file.
    pub language: Option<&'a str>,
    /// The open project's identity and root (none: a file opened alone).
    pub project: Option<ProjectId>,
    pub root: Option<&'a Path>,
    /// Wakes the app (from a background thread): it then calls [`EditorToolbar::poll`].
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for ToolbarContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolbarContext").field("path", &self.path).finish_non_exhaustive()
    }
}

/// A module's contribution: which files get its bar.
pub trait EditorToolbarProvider: Send + Sync + Debug {
    fn id(&self) -> &'static str;
    /// The bar of the file `context` names (`None`: not one of its). Asked when the file's
    /// tab opens: cheap, nothing started.
    fn open(&self, context: &ToolbarContext<'_>) -> Option<Box<dyn EditorToolbar>>;
}

/// What the bar draws with.
pub struct ToolbarPaint<'a> {
    pub bounds: Rect,
    pub scale: f32,
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'a Palette,
    pub icons: &'a Icons,
}

impl std::fmt::Debug for ToolbarPaint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolbarPaint").field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

/// What a click on the bar (or a key, a menu's row) asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolbarAction {
    /// As a view's (a command run, a file opened, text copied).
    View(ViewerAction),
    /// A menu shown under `anchor` (a button's dropdown); the chosen row's id comes back as
    /// an [`EditorToolbar::command`].
    Menu { anchor: Rect, entries: Vec<ViewMenuEntry> },
}

/// The editor's state the bar follows ([`EditorToolbar::sync_editor`]), while it asks to.
#[derive(Debug, Clone)]
pub struct EditorState {
    /// The text as in the editor (unsaved changes included).
    pub text: Arc<str>,
    /// Its version (changes with each edit).
    pub version: u64,
    /// The primary cursor (bytes).
    pub cursor: usize,
    /// The primary selection, when it isn't empty.
    pub selection: Option<Range<usize>>,
}

/// How a statement's lines are marked in the gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkState {
    /// Not run yet, ready to: a ▶ (a request of a `.http` file).
    Runnable,
    Running,
    Done,
    Failed,
}

/// Lines marked in the gutter: a statement (its bytes as they were when it ran, and a hash
/// of its text — the mark is only shown while the text there is still the same).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementMark {
    pub range: Range<usize>,
    /// [`text_hash`] of the statement's text.
    pub hash: u64,
    pub state: MarkState,
}

/// A statement's text's hash, as [`StatementMark::hash`] holds it.
pub fn text_hash(text: &str) -> u64 {
    // FNV-1a: stable, cheap, enough to tell an edited statement from the one that ran.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// A bar over a file's editor (see the module's doc). Everything but [`Self::paint`] has a
/// default: a bar drawing itself and doing nothing else.
pub trait EditorToolbar: Send {
    /// Its height (logical pixels).
    fn height(&self) -> f32 {
        HEIGHT
    }
    fn paint(&mut self, ctx: ToolbarPaint<'_>);
    /// A press at `p` (inside the bar).
    fn pointer_down(&mut self, p: Point) -> Option<ToolbarAction> {
        let _ = p;
        None
    }
    /// The mouse over the bar at `p` (`None`: it left). `true` if something changed.
    fn hover(&mut self, p: Option<Point>) -> bool {
        let _ = p;
        false
    }
    /// The cursor over `p` (a hand over a button).
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let _ = p;
        None
    }
    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let _ = p;
        None
    }
    /// After a wake: `true` if it has something new to draw (a statement ended).
    fn poll(&mut self) -> bool {
        false
    }
    /// Work going on (a spinner turns): frames come at a loader's pace.
    fn is_loading(&self) -> bool {
        false
    }
    /// A transition moving (a button widening for its spinner): frames come until it ends.
    fn is_animating(&self) -> bool {
        false
    }
    /// The tab's title (`None`: the file's name).
    fn title(&self) -> Option<String> {
        None
    }
    /// The tab's icon (a name of the app's set; `None`: the file's).
    fn icon(&self) -> Option<&'static str> {
        None
    }
    /// The color the tab carries (a data source's): a dot on the tab's icon and a strip
    /// along the bar's top, drawn by the app. Asked at each paint: kept, never worked out.
    fn accent(&self, palette: &Palette) -> Option<Rgba> {
        let _ = palette;
        None
    }
    /// A short mark the tab shows after its title, and its color (`TX` while a console's
    /// transaction is open). Asked at each paint: kept, never worked out.
    fn tab_badge(&self, palette: &Palette) -> Option<(String, Rgba)> {
        let _ = palette;
        None
    }
    /// The breadcrumb's start, in place of the file's name (`Database Consoles › source ›
    /// console`); empty: the file's.
    fn crumbs(&self) -> Vec<String> {
        Vec::new()
    }
    /// Asked before the tab closes: `false` keeps it open (the bar asks first — a
    /// transaction left open — and closes the tab itself once answered).
    fn may_close(&mut self) -> bool {
        true
    }
    /// Its file was renamed or moved to `path` (whoever did it: the tab's menu, the tree, a
    /// refactoring, the disk); the tab follows. `true`: the bar is still the file's and
    /// follows it too (its title, its breadcrumb); `false`: the app asks the providers
    /// again (the file may be no module's now, or another's).
    fn moved(&mut self, path: &Path) -> bool {
        let _ = path;
        false
    }
    /// A row of one of its menus chosen (its id), or a command meant for it.
    fn command(&mut self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// What it asks for on its own (after a poll or a command), taken by the app.
    fn take_action(&mut self) -> Option<ToolbarAction> {
        None
    }
    /// What it tells the user (toasts), taken by the app.
    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        Vec::new()
    }
    /// It follows the editor now (the statement under the cursor highlighted while the
    /// mouse rests on Run): the app hands it [`EditorState`] when it changes.
    fn follows_editor(&self) -> bool {
        false
    }
    fn sync_editor(&mut self, state: &EditorState) {
        let _ = state;
    }
    /// The gutter's marks, and a number changing whenever they do.
    fn marks(&self) -> (&[StatementMark], u64) {
        (&[], 0)
    }
    /// A gutter mark clicked: the one whose statement starts at byte `at` (a ▶: run it).
    /// `false`: nothing done (the click only moves the cursor there).
    fn mark_clicked(&mut self, at: usize) -> bool {
        let _ = at;
        false
    }
    /// The text drawn highlighted (bytes): the statement that would run.
    fn highlight(&self) -> Option<Range<usize>> {
        None
    }
    /// The statement the cursor is in (bytes; the selection's when there is one): what Run
    /// runs, marked by a bar beside its lines so it's clear which one that is.
    fn current(&self) -> Option<Range<usize>> {
        None
    }
    /// A place of the text pointed at (bytes) in a severity's color: what a finding is about
    /// (a profile's), drawn tinted and outlined, its lines lightly tinted.
    fn focus(&self) -> Option<(Range<usize>, crate::severity::Level)> {
        None
    }
    /// Clears what can be rebuilt when needed ("Clear and Rebuild Caches").
    fn purge(&mut self) {}
    /// Its file's grammar errors are shown even where its language doesn't report them
    /// (a permissive grammar's), faint: the user asked for them.
    fn grammar_hints(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/editor_toolbar.rs"]
mod tests;
