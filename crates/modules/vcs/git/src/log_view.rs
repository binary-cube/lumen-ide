//! "Git Log": every branch's history as a list with its graph (lanes and the lines joining
//! commits), the references at each commit, its author and age; the branches on the left
//! (choosing one shows only its history); the selected commit's details on the right
//! (message, changed files — a click compares one with its parent). The three are resized
//! with splitters. A file's history shows, of each commit, that file (the others on demand).
//!
//! Loaded in pieces: commits are read a page at a time in the background (the next page
//! when the list nears its end), a commit's details when it's selected; closing the tab
//! lets everything go. Reading uses gitoxide (no `git` needed); Fetch / Pull / Push run the
//! Git menu's commands.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{
    Palette, Splitter, SplitterStyle, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;
use version_control::application::{LogRow, compare_commit_file, file_log_page, log_page};
use version_control::{
    CommitDetails, FileState, GraphBuilder, GraphRow, LogFilter, RefKind, RefLabel, Repository,
    SyncState,
};

use crate::branches::{BranchItem, BranchRow};
use workbench_model::{CommandHandler, WorkbenchHost};

mod branches;
mod commands;
mod details;
mod filter_bar;
mod filters;
mod menus;
mod paint;
mod view;

pub(crate) use commands::{missing_fixed_tabs, open_fixed_tabs, show_log_of};

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-git-log";
/// The tool window (at the bottom) the log and the histories open in.
pub const TOOL: &str = "git";
/// The name the whole log is found by in it (the same in every language).
pub const LOG_KEY: &str = "git.log";
/// Commits read per page.
const PAGE: usize = 300;
/// The next page is read when fewer rows than this are left below the view.
const AHEAD: usize = 60;
/// Lanes drawn at most (a history with more parallel branches is cut at the right).
const MAX_LANES: usize = 14;

/// The log's viewer (where its actions' keys work).
pub(crate) const VIEW: &str = "git.log";

#[derive(Debug)]
pub(crate) struct LogProvider;

impl ViewerProvider for LogProvider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(LogView::default()))
    }
}

/// What the view is opened with.
#[derive(Clone)]
pub struct LogData {
    pub repo: Arc<dyn Repository>,
    /// One file's history (relative, with `/`) instead of every branch's.
    pub path: Option<String>,
    /// The commit (id) to select once it's read (a blame annotation clicked).
    pub commit: Option<String>,
    /// The project's queries (the files' icons by language).
    pub queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    /// Only this reference's history (its full name: a branch, a tag), as if chosen in the
    /// branches pane.
    pub reference: Option<String>,
}

impl std::fmt::Debug for LogData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogData")
            .field("path", &self.path)
            .field("commit", &self.commit)
            .finish_non_exhaustive()
    }
}

/// The history of the active file (or the one the menu was opened on).
pub(crate) struct ShowFileHistory;

pub(crate) struct ShowLog;

/// The commit annotated on the active line (blame), selected in the log.
pub(crate) struct ShowAnnotatedCommit;

/// A message wrapped for a width: ((commit, width bits), lines with whether they're bold).
type WrappedMessage = ((String, u32), Vec<(String, bool)>);

/// A page read in the background: the graph continued, the rows; with the first one, the
/// branch's state and the references.
pub struct Page {
    graph: GraphBuilder,
    rows: Vec<LogRow>,
}

/// The panels' widths at first (logical pixels).
const BRANCHES_WIDTH: f32 = 220.0;
const DETAILS_WIDTH: f32 = 380.0;
/// The room between two panels, the separator's alone (logical px).
const SEAM: f32 = 8.0;
/// The room after a list's last row (logical pixels): it never ends flush on the edge.
const LIST_END: f32 = 12.0;

/// The commits chosen, read in the background: their details (newest first) and, for one,
/// the branches that hold it.
pub struct Chosen {
    details: Vec<CommitDetails>,
    branches: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Row(usize),
    /// The details' changed files (their tree has its own parts).
    Files,
    /// A toolbar button running a Git command (`git.fetch`…).
    Command(&'static str),
    Refresh,
    CloseDetails,
    /// A row of the branches panel.
    Branch(usize),
    /// A branch's commits to push / to pull (their tooltip).
    BranchSync(usize),
    /// The branches panel's filter field, and its ×.
    BranchSearch,
    ClearBranches,
    /// A file's history: every file of the commit, or only this one.
    AllFiles,
    /// The filter bar: the text field, its `.*` and `Cc`, its ×.
    TextField,
    Regex,
    MatchCase,
    ClearText,
    /// A filter's pill (its list), its ×.
    Pill(filters::FilterKind),
    ClearPill(filters::FilterKind),
    /// The open list: itself, its search (and its ×), a row, the interval's fields and Apply.
    Popup,
    PopupQuery,
    ClearPopupQuery,
    PopupRow(usize),
}

/// The keys of the branches' and the details' widths (logical pixels) in the project's
/// sizes ([`workbench_model::LayoutSizes`]).
pub const BRANCHES_SIZE: &str = "git.log-branches";
pub const DETAILS_SIZE: &str = "git.log-details";
/// The share of the details' height the changed files take (above the commit).
pub const FILES_SHARE_SIZE: &str = "git.log-details-files";

#[derive(Default)]
pub struct LogView {
    repo: Option<Arc<dyn Repository>>,
    /// Only this file's history.
    path: Option<String>,
    rows: Vec<LogRow>,
    /// The graph so far (`None` while a page is being read: it's with the reader).
    graph: Option<GraphBuilder>,
    loading: Option<Receiver<Page>>,
    /// Every commit was read.
    complete: bool,
    sync: Option<SyncState>,
    /// The commit chosen last (where ⇧ ranges start), and every one chosen (⇧ / ⌘), in the
    /// list's order.
    selected: Option<usize>,
    chosen: Vec<usize>,
    /// The chosen commit's details (one chosen), every chosen one's, the branches holding it.
    details: Option<Arc<CommitDetails>>,
    details_all: Vec<Arc<CommitDetails>>,
    containing: Vec<String>,
    details_loading: Option<Receiver<Chosen>>,
    /// The files the details show (with the commit each change is from), and their tree.
    files: Vec<(Arc<CommitDetails>, version_control::FileChange)>,
    tree: crate::files_tree::FilesTree,
    /// The tree has the keys (it was clicked last).
    files_focus: bool,
    queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    cancel: Arc<AtomicBool>,
    scroll: f32,
    row_h: f32,
    list: Rect,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    pending: Option<ViewerAction>,
    /// The selected commit's message wrapped to the panel: (commit, width) → lines (bold?).
    message: Option<WrappedMessage>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// A comparison being read (its action comes back through `poll`).
    opening: ui_components::deferred::Deferred,
    /// What the log shows (the filter bar's filters, the branch chosen in the panel).
    filter: LogFilter,
    /// The filter bar (its fields, its lists).
    bar: filters::FilterBar,
    /// The branches and tags, every local branch against its upstream, the remotes' names (a
    /// branch's menu pushes to them): taken from the repository's references every view
    /// shares ([`crate::refs_store`]), and the read taken last.
    refs: Option<Vec<RefLabel>>,
    branch_states: Vec<SyncState>,
    remotes: Vec<String>,
    shared: Option<Arc<crate::refs_store::Refs>>,
    shared_taken: u64,
    /// The branches panel's rows, the one clicked (a double click filters by it), and its
    /// folders closed.
    branch_rows: Vec<BranchRow>,
    branch_selected: Option<usize>,
    collapsed: HashSet<String>,
    branches_scroll: f32,
    branches: Rect,
    /// A file's history: every file of the selected commit shown (else only that file).
    all_files: bool,
    /// Between the branches and the list, between the list and the details.
    splitters: [Splitter; 2],
    /// Between the details' files and the commit (made when first painted), and the
    /// files' share of the height (0: the default).
    files_split: Option<Splitter>,
    files_share: f32,
    /// The details panel (from the last paint): what the files' share is of.
    details_area: Rect,
    /// The panels' widths (logical pixels), as dragged.
    branches_w: f32,
    details_w: f32,
    /// The project's dragged sizes: the panels' widths are kept there ([`BRANCHES_SIZE`],
    /// [`DETAILS_SIZE`]).
    sizes: Option<workbench_model::LayoutSizes>,
    bounds: Rect,
    scale: f32,
    /// Where the mouse is (the wheel scrolls what's under it).
    pointer: Point,
    /// Let go while hidden: read again when painted.
    stale: bool,
    /// Made again from a session where no repository is found: its tab closes.
    gone: bool,
    /// A field was pressed: the mouse dragged selects in it.
    field_drag: bool,
    /// The commit to select once read (pages are read until it's found).
    wanted: Option<String>,
    /// The selected row is brought into view at the next paint (its height known then).
    reveal_selected: bool,
    /// The details panel (from the last paint), what scrolls in it (the message and the
    /// files, under the id and the author) and its scrollbar.
    details_panel: Rect,
    detail_room: Rect,
    detail_scroll: ui_components::ScrollState,
    detail_bar: ui_components::EdgeScrollbar,
    /// The commits' and the branches' scrollbars (hidden at rest).
    list_bar: ui_components::EdgeScrollbar,
    branches_bar: ui_components::EdgeScrollbar,
}

impl std::fmt::Debug for LogView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogView").field("rows", &self.rows.len()).finish_non_exhaustive()
    }
}

impl Drop for LogView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl LogView {
    /// The commits read so far.
    #[cfg(test)]
    pub fn rows(&self) -> &[LogRow] {
        &self.rows
    }

    /// Starts over: the first page (after a pull, a commit, "Refresh").
    fn reload(&mut self) {
        // The references read again too (shared: every view of them follows).
        match &self.shared {
            Some(shared) => shared.refresh(),
            None => self.follow_refs(),
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.rows.clear();
        self.graph = Some(GraphBuilder::default());
        self.loading = None;
        self.complete = false;
        self.clear_selection_state();
        self.scroll = 0.0;
        self.load_more();
    }

    /// The branches panel's rows again (the references read, a folder opened / closed, its
    /// filter typed).
    fn rebuild_branches(&mut self) {
        let query = self.bar.branch_query.text().trim().to_owned();
        self.branch_rows = match &self.refs {
            Some(refs) if self.path.is_none() => {
                crate::branches::rows(refs, &self.collapsed, &query)
            }
            _ => Vec::new(),
        };
        self.branch_selected = self.branch_selected.filter(|&i| i < self.branch_rows.len());
    }

    /// A row of the branches panel chosen (a double click, ↵): the Branch filter is that
    /// one alone ("All": none), or a folder opened / closed.
    fn choose_branch(&mut self, i: usize) {
        let Some(row) = self.branch_rows.get(i) else { return };
        match row.item.clone() {
            BranchItem::All => self.filter_branch(None),
            BranchItem::Head => self.filter_branch(Some("HEAD".into())),
            BranchItem::Ref { name, .. } => self.filter_branch(Some(name)),
            BranchItem::Group { key, .. } => {
                if !self.collapsed.remove(&key) {
                    self.collapsed.insert(key);
                }
                self.rebuild_branches();
            }
        }
    }

    /// Nothing chosen any more (no details, no files).
    fn clear_selection_state(&mut self) {
        self.selected = None;
        self.chosen.clear();
        self.details = None;
        self.details_all.clear();
        self.containing.clear();
        self.details_loading = None;
        self.refresh_files();
    }

    /// The details' files again: every chosen commit's (a path changed by several: once,
    /// as the newest left it — added if an older one added it and it's still there); in a
    /// file's history, only that file (unless all are asked for).
    fn refresh_files(&mut self) {
        let mut files: Vec<(Arc<CommitDetails>, version_control::FileChange)> = Vec::new();
        for details in &self.details_all {
            for file in &details.files {
                match files.iter_mut().find(|(_, f)| f.path == file.path) {
                    Some((_, newer)) => {
                        if file.state == FileState::Added && newer.state != FileState::Deleted {
                            newer.state = FileState::Added;
                        }
                    }
                    None => files.push((Arc::clone(details), file.clone())),
                }
            }
        }
        if let Some(path) = &self.path
            && !self.all_files
        {
            files.retain(|(_, f)| f.path == *path || f.old_path.as_deref() == Some(path.as_str()));
        }
        let changed = files
            .iter()
            .map(|(_, f)| crate::files_tree::changed(&f.path, f.old_path.as_deref(), f.state))
            .collect();
        self.tree.set_files(changed);
        self.files = files;
    }

    /// The paths of the files the details show.
    #[cfg(test)]
    fn shown_paths(&self) -> Vec<&str> {
        self.files.iter().map(|(_, f)| f.path.as_str()).collect()
    }

    /// The project's sizes: the panels as wide as they were last dragged in the project.
    fn take_sizes(&mut self, sizes: &workbench_model::LayoutSizes) {
        if let Some(w) = sizes.get(BRANCHES_SIZE).filter(|w| *w > 0.0) {
            self.branches_w = w.max(140.0);
        }
        if let Some(w) = sizes.get(DETAILS_SIZE).filter(|w| *w > 0.0) {
            self.details_w = w.max(260.0);
        }
        self.tree.set_flat(sizes.get(crate::files_tree::LIST_MODE).is_some_and(|v| v > 0.5));
        if let Some(share) = sizes.get(FILES_SHARE_SIZE).filter(|s| (0.1..=0.9).contains(s)) {
            self.files_share = share;
        }
        self.sizes = Some(sizes.clone());
    }

    /// The separators let go: the panels' widths kept in the project.
    fn keep_widths(&self) {
        let Some(sizes) = &self.sizes else { return };
        if self.branches_w > 0.0 {
            sizes.set(BRANCHES_SIZE, self.branches_w.round());
        }
        if self.details_w > 0.0 {
            sizes.set(DETAILS_SIZE, self.details_w.round());
        }
        if self.files_share > 0.0 {
            sizes.set(FILES_SHARE_SIZE, (self.files_share * 1000.0).round() / 1000.0);
        }
    }

    /// Separator `i` dragged to `x` (physical): the panel beside it follows.
    fn drag_separator(&mut self, i: usize, x: f32) {
        let b = self.bounds;
        let scale = self.scale.max(0.1);
        // `x` is the seam's middle: the panel ends half a seam before it.
        let half = SEAM / 2.0;
        if i == 0 {
            let max = (b.size.width * 0.45 / scale).max(140.0);
            self.branches_w = ((x - b.min_x()) / scale - half).clamp(140.0, max);
        } else {
            let max = (b.size.width * 0.6 / scale).max(260.0);
            self.details_w = ((b.max_x() - x) / scale - half).clamp(260.0, max);
        }
    }

    /// Reads the next page in the background (if none is being read and some are left).
    fn load_more(&mut self) {
        if self.loading.is_some() || self.complete {
            return;
        }
        let (Some(repo), Some(mut graph)) = (self.repo.clone(), self.graph.take()) else {
            return;
        };
        let skip = self.rows.len();
        let cancel = Arc::clone(&self.cancel);
        let waker = self.waker.clone();
        let path = self.path.clone();
        let filter = self.filter.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-log", move || {
            let rows = match &path {
                Some(path) => file_log_page(repo.as_ref(), path, &mut graph, skip, PAGE),
                None => log_page(repo.as_ref(), &filter, &mut graph, skip, PAGE),
            };
            if !cancel.load(Ordering::Relaxed)
                && tx.send(Page { graph, rows }).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.loading = Some(rx);
    }

    /// The repository's shared references held (their reads waking this view's window).
    fn follow_refs(&mut self) {
        let Some(repo) = &self.repo else { return };
        match &self.shared {
            Some(shared) => {
                if let Some(wake) = &self.waker {
                    shared.follow(Arc::clone(wake));
                }
            }
            None => self.shared = Some(crate::refs_store::of(repo, self.waker.clone())),
        }
    }

    /// The shared references' newest read taken (the branches panel made again from it).
    /// `true` if there was one, or one is being read.
    pub(crate) fn take_refs(&mut self) -> bool {
        let Some(shared) = &self.shared else { return false };
        // Being read: busy until it's taken (what's shown follows).
        let busy = shared.reading();
        let (number, read) = shared.current();
        let Some(read) = read.filter(|_| number != self.shared_taken) else { return busy };
        self.shared_taken = number;
        self.refs = Some(read.refs.clone());
        self.branch_states.clone_from(&read.states);
        self.remotes.clone_from(&read.remotes);
        self.sync.clone_from(&read.sync);
        self.rebuild_branches();
        true
    }

    /// Takes a page read in the background.
    pub fn add_page(&mut self, Page { graph, rows }: Page) {
        self.complete = rows.len() < PAGE;
        self.rows.extend(rows);
        self.graph = Some(graph);
    }

    fn select(&mut self, i: usize) {
        if (self.selected == Some(i) && self.chosen == [i]) || i >= self.rows.len() {
            return;
        }
        self.selected = Some(i);
        self.chosen = vec![i];
        self.all_files = false;
        self.load_chosen();
    }

    /// Row `i` pressed with ⇧ (every row from the last one chosen) or ⌘ (it taken or
    /// left); without either: it alone.
    fn select_with(&mut self, i: usize, mods: ui_components::PressMods) {
        if i >= self.rows.len() {
            return;
        }
        if mods.shift
            && let Some(anchor) = self.selected
        {
            self.chosen = (anchor.min(i)..=anchor.max(i)).collect();
        } else if mods.primary {
            match self.chosen.iter().position(|&c| c == i) {
                Some(at) if self.chosen.len() > 1 => {
                    self.chosen.remove(at);
                }
                Some(_) => return,
                None => {
                    self.chosen.push(i);
                    self.chosen.sort_unstable();
                }
            }
            self.selected = Some(i);
        } else {
            self.select(i);
            return;
        }
        self.load_chosen();
    }

    /// The chosen commits' details read in the background (and, for one, the branches
    /// holding it).
    fn load_chosen(&mut self) {
        self.details = None;
        self.details_all.clear();
        self.containing.clear();
        self.details_loading = None;
        self.detail_scroll.reset();
        self.refresh_files();
        let Some(repo) = self.repo.clone() else { return };
        let ids: Vec<String> = self
            .chosen
            .iter()
            .filter_map(|&c| self.rows.get(c))
            .map(|r| r.entry.commit.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        let cancel = Arc::clone(&self.cancel);
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-commit", move || {
            let details: Vec<CommitDetails> =
                ids.iter().filter_map(|id| repo.commit_details(id)).collect();
            let branches = match ids.as_slice() {
                [one] => repo.containing_branches(one),
                _ => Vec::new(),
            };
            if !cancel.load(Ordering::Relaxed)
                && tx.send(Chosen { details, branches }).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.details_loading = Some(rx);
    }

    /// The chosen commits' details, read.
    fn take_chosen(&mut self, chosen: Chosen) {
        self.details_all = chosen.details.into_iter().map(Arc::new).collect();
        self.details = match self.details_all.as_slice() {
            [one] => Some(Arc::clone(one)),
            _ => None,
        };
        self.containing = chosen.branches;
        self.refresh_files();
    }

    /// The commit asked for, selected once read; not yet: the next page.
    fn find_wanted(&mut self) {
        let Some(id) = &self.wanted else { return };
        match self.rows.iter().position(|r| r.entry.commit.id == *id) {
            Some(i) => {
                self.wanted = None;
                self.select(i);
                self.reveal_selected = true;
            }
            None if self.complete => self.wanted = None,
            None => self.load_more(),
        }
    }

    /// Keeps the selected row in view.
    fn reveal(&mut self, i: usize) {
        if self.row_h <= 0.0 {
            return;
        }
        let top = i as f32 * self.row_h;
        let height = self.list.size.height;
        if top < self.scroll {
            self.scroll = top;
        } else if top + self.row_h > self.scroll + height {
            self.scroll = top + self.row_h - height;
        }
    }

    fn clamp_scroll(&mut self) {
        let content = self.list_content();
        self.scroll = self.scroll.clamp(0.0, (content - self.list.size.height).max(0.0));
    }

    /// The commits' height scrolled through: their rows, and some room after the last.
    fn list_content(&self) -> f32 {
        self.rows.len() as f32 * self.row_h + (LIST_END * self.scale.max(0.1)).round()
    }

    /// The branches' height scrolled through: their rows, and some room after the last.
    fn branches_content(&self) -> f32 {
        self.branch_rows.len() as f32 * self.branch_row_h()
            + (LIST_END * self.scale.max(0.1)).round()
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// File `i` of the details compared with its version before the commit that changed it.
    fn compare_file(&mut self, i: usize) -> Option<ViewerAction> {
        if !workbench_model::presence::compare() {
            return None;
        }
        let repo = self.repo.clone()?;
        let (details, file) = self.files.get(i)?.clone();
        self.opening.start("git-log", move || {
            let input = compare_commit_file(repo.as_ref(), &details, &file);
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            let title = format!("{name} · {}", details.commit.short());
            Some(ViewerAction::CompareBeside(Box::new(input), title))
        });
        None
    }
}

#[cfg(test)]
#[path = "../tests/unit/log_view.rs"]
mod tests;
