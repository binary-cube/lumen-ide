//! "Compare" module: two files side by side, with aligned differences (lines
//! deleted on the left, added on the right, modified on both, with the changed part of the
//! line highlighted) and a strip with all differences on the right.
//!
//! In "Compose" mode, the result appears in the middle: for each difference you pick left
//! (›), right (‹), both or neither, like resolving a merge conflict. ⌘S writes
//! the result to the chosen file ("Write to: left / right").
//!
//! Given a [`diff::MergeInput`] (a conflicted file), it resolves it in three parts: ours on
//! the left, theirs on the right (both read-only), the result in the middle, written to the
//! file. What one side changed alone is taken already; each conflict waits for a choice,
//! and the result is written once none is left.
//!
//! "Compare with Clipboard": the clipboard's text (read-only) against the active file, or
//! its selection (then read-only too: there's nothing to write it to). A tab's "Compare
//! with ›" lists the group's other tabs with files ([`OtherTabs`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use ui_components::ds::Pen;

use command_model::{CommandId, CommandSpec};
use diff::{
    Chunk, CompareInput, MergeChunks, MergeInput, Pick, compose, diff_lines, inline_change,
};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, Ctx};
use ui_components::{
    Palette, Splitter, SplitterStyle, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;
use workbench_model::{
    ActionContext, ActiveFile, CommandContribution, CommandHandler, ContextMenuItem,
    ListedMenuItems, ListedRow, MenuList, MenuPlace, MenuSection, ViewAction, WorkbenchHost,
};

mod diff_job;
mod paint;
mod select;
mod view;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("view.compare"),
    name: "Compare",
    description: "The differences between two files and composing a new one from them",
    depends_on: &[],
};

pub const COMPARE_CLIPBOARD: CommandId = CommandId("compare.clipboard");
/// A tree file compared with the active one (its id from when it was the tree's).
pub const COMPARE_WITH_ACTIVE: CommandId = CommandId("tree.compare_with_active");

/// What a diff on a thread brings: the side saved (with its lines) it was found for, if
/// any, and the differences.
type Diffed = (Option<(Side, Vec<String>)>, MergeChunks);

#[derive(Debug, Default)]
pub struct CompareModule;

impl Module for CompareModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["compare-diff"]));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: COMPARE_CLIPBOARD,
                label: "Compare with Clipboard",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::EDIT_CLIPBOARD,
                    50,
                )),
                shortcuts: &[],
            },
            icon: Some("compare"),
            handler: Arc::new(CompareClipboard),
        });
        ctx.contribute(ContextMenuItem::new(
            COMPARE_CLIPBOARD,
            &[MenuPlace::Editor],
            MenuSection::Clipboard,
            40,
        ));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: COMPARE_WITH_ACTIVE,
                label: "Compare with Active File",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("compare"),
            handler: Arc::new(CompareWithActive),
        });
        // A tree file's "Compare with ›": while another file is active.
        ctx.contribute(
            ContextMenuItem::new(
                COMPARE_WITH_ACTIVE,
                &[MenuPlace::TreeFile],
                MenuSection::Compare,
                10,
            )
            .label("compare.active_file"),
        );
        // A tab's "Compare with ›": the other tabs, then the clipboard.
        ctx.contribute(ListedMenuItems {
            id: OTHER_TABS,
            icon: Some("compare"),
            places: &[MenuPlace::Tab],
            section: MenuSection::Compare,
            list: Arc::new(OtherTabs),
        });
        ctx.contribute(
            ContextMenuItem::new(COMPARE_CLIPBOARD, &[MenuPlace::Tab], MenuSection::Compare, 20)
                .label("compare.clipboard"),
        );
        // A tree file's "Compare with ›": while the clipboard holds text.
        ctx.contribute(
            ContextMenuItem::new(
                COMPARE_CLIPBOARD,
                &[MenuPlace::TreeFile],
                MenuSection::Compare,
                20,
            )
            .label("compare.clipboard"),
        );
        for (id, label, shortcuts, action) in ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(ViewAction { view: VIEW, action }),
            });
        }
        Ok(())
    }
}

/// The comparison's viewer (where its actions' keys work).
const VIEW: &str = "compare";

/// The comparison's actions (remappable; the keys they always had): the command, its
/// label, its keys, what the view is sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 5] = [
    (CommandId("compare.next"), "Next Change", &["n", "j"], "next"),
    (CommandId("compare.previous"), "Previous Change", &["p", "k"], "previous"),
    (CommandId("compare.take_left"), "Take Left", &["["], "take_left"),
    (CommandId("compare.take_right"), "Take Right", &["]"], "take_right"),
    (CommandId("compare.merge"), "Compose Mode", &["m"], "merge"),
];

/// The clipboard against the active file (or its selection).
struct CompareClipboard;

impl CommandHandler for CompareClipboard {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        // On a tree's file only with something to compare (the editor's tells when empty).
        let tree = host.context().place == MenuPlace::TreeFile;
        host.active_file().is_some()
            && (!tree || host.clipboard_text().is_some_and(|t| !t.is_empty()))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(file) = host.active_file() else { return false };
        let Some(clipboard) = host.clipboard_text().filter(|t| !t.is_empty()) else {
            host.notify(workbench_model::Notification::info(t("compare.clipboard_empty")));
            return false;
        };
        let input = clipboard_input(&file, clipboard);
        let title = format!("{} ↔ {}", file_name(&file.path), t("compare.clipboard"));
        host.compare(input, &title)
    }
}

/// The file the tree's menu was opened on, against the active one (each as the editor has
/// it, unsaved changes included).
struct CompareWithActive;

impl CompareWithActive {
    fn files(host: &dyn WorkbenchHost) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let target = host.target_path().filter(|p| p.is_file())?;
        let active = host.active_file().map(|f| f.path).filter(|a| *a != target && a.is_file())?;
        Some((target, active))
    }
}

impl CommandHandler for CompareWithActive {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        Self::files(host).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some((left, right)) = Self::files(host) else { return false };
        compare_files(host, &left, &right)
    }
}

/// The rows of a tab's "Compare with ›" listing the other tabs (`compare.tabs:<tab>:<other>`,
/// by their documents' identities).
pub const OTHER_TABS: &str = "compare.tabs";

/// How many other tabs "Compare with ›" lists.
pub const OTHER_TABS_SHOWN: usize = 6;

/// A tab's "Compare with ›": the group's other tabs with files, each compared with it.
struct OtherTabs;

impl MenuList for OtherTabs {
    fn rows(&self, host: &dyn WorkbenchHost, context: &ActionContext) -> Vec<ListedRow> {
        // A tab without a file has nothing to compare.
        let Some(path) = context.path() else { return Vec::new() };
        let tabs = host.tabs();
        let Some(this) = tabs.iter().find(|t| t.path.as_deref() == Some(path)) else {
            return Vec::new();
        };
        tabs.iter()
            .filter(|t| t.id != this.id && t.path.is_some())
            .take(OTHER_TABS_SHOWN)
            .map(|t| ListedRow {
                label: t.title.clone(),
                argument: format!("{}:{}", this.id.get(), t.id.get()),
            })
            .collect()
    }

    fn run(&self, host: &mut dyn WorkbenchHost, argument: &str) -> bool {
        let tabs = host.tabs();
        let path = |raw: &str| {
            let raw = raw.parse::<u64>().ok()?;
            tabs.iter().find(|t| t.id.get() == raw)?.path.clone()
        };
        let Some((a, b)) = argument.split_once(':') else { return false };
        let (Some(left), Some(right)) = (path(a), path(b)) else { return false };
        compare_files(host, &left, &right)
    }
}

/// Opens the comparison of two files, each as the editor has it (unsaved changes included),
/// else as on disk.
pub fn compare_files(host: &mut dyn WorkbenchHost, left: &Path, right: &Path) -> bool {
    let (Some(a), Some(b)) = (host.file_text(left), host.file_text(right)) else {
        tracing::warn!("can't read the files to compare");
        return false;
    };
    let title = format!("{} ↔ {}", file_name(left), file_name(right));
    host.compare(CompareInput::files((left.to_path_buf(), a), (right.to_path_buf(), b)), &title)
}

/// The comparison: the clipboard on the left (read-only), the file on the right — or its
/// selection, read-only as well (it can't be written back as a whole file).
fn clipboard_input(file: &ActiveFile, clipboard: String) -> CompareInput {
    let name = file_name(&file.path);
    let selected = file.selection.clone().and_then(|r| file.text.get(r));
    let (right, label) = match selected {
        Some(text) => (text.to_owned(), Some(format!("{name} · {}", t("compare.selection")))),
        None => (file.text.to_string(), None),
    };
    CompareInput {
        left: (file.path.clone(), clipboard),
        right: (file.path.clone(), right),
        labels: [Some(t("compare.clipboard")), label],
        read_only: [true, selected.is_some()],
    }
}

/// Extension of the compare view's virtual path (anyone's comparison opens it).
const EXTENSION: &str = diff::COMPARE_VIEW;

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(Compare::default()))
    }
}

/// A displayed row: the left, result and right lines (or empty, for alignment)
/// and the difference it belongs to.
#[derive(Debug, Clone, Default)]
struct Row {
    left: Option<usize>,
    result: Option<String>,
    right: Option<usize>,
    change: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Mode(bool),
    Target(Side),
    AllLeft,
    AllRight,
    Prev,
    Next,
    Pick(usize, Pick),
    /// Resolving: the result written by hand, or back to the choices.
    ByHand,
    /// A difference's rows: it becomes the current one.
    Select(usize),
    Overview,
    /// Composing: the result saved into the file being written to.
    Save,
}

/// The keys of the columns' places (fractions of their width) in the project's sizes
/// ([`workbench_model::LayoutSizes`]): where the left column ends comparing, where the left
/// and the result columns end composing.
pub const DIFF_SPLIT: &str = "compare.diff";
pub const MERGE_LEFT: &str = "compare.merge-left";
pub const MERGE_RESULT: &str = "compare.merge-result";

#[derive(Debug, Default)]
struct Compare {
    left_path: PathBuf,
    right_path: PathBuf,
    /// Shown instead of the file names (e.g. `A.php · HEAD`).
    labels: [Option<String>; 2],
    /// Sides that can't be written (a version from git).
    read_only: [bool; 2],
    left: Vec<String>,
    right: Vec<String>,
    /// Line terminator of each file (preserved on write).
    crlf: (bool, bool),
    trailing_newline: (bool, bool),
    chunks: Vec<Chunk>,
    picks: Vec<Pick>,
    merge: bool,
    target: Option<Side>,
    rows: Vec<Row>,
    /// Start row of each difference.
    change_rows: Vec<usize>,
    current: usize,
    scroll: f32,
    /// Scrolled across, each column on its own (left, result, right): how far, how far it
    /// can go, its bar along its bottom.
    across: [Across; 3],
    /// Each column's longest line in characters (a tab as four; the result's: either
    /// side's), measured when the rows are made (never per frame).
    widest: [usize; 3],
    bounds: Rect,
    scale: f32,
    row_h: f32,
    list: Rect,
    overview: Rect,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    loaded: bool,
    /// The differences, found on a thread (the UI never waits for them), received here.
    diffing: Option<std::sync::mpsc::Receiver<Diffed>>,
    /// A side just saved, its differences being found again: nothing left to save meanwhile.
    saved: bool,
    /// How the thread wakes the app once they're found (set at the first paint; if they're
    /// found before, that paint takes them).
    waker: WakerSlot,
    /// Scroll to the current difference on the next paint (just opened).
    reveal_first: bool,
    /// The separators between the columns (draggable; the second only in "Compose").
    splitters: [Splitter; 2],
    /// Where the columns end, as fractions of their total width: `[left]` in diff mode,
    /// `[left, result]` in "Compose" (`None`: equal columns).
    split_diff: Option<f32>,
    split_merge: Option<[f32; 2]>,
    /// The project's dragged sizes: the columns' places are kept there ([`DIFF_SPLIT`]…).
    sizes: Option<workbench_model::LayoutSizes>,
    /// The gutter's width and the columns' total width, from the last paint (for dragging).
    gutter: f32,
    columns_w: f32,
    /// What the app does next (the result saved from the file's tab).
    pending: Option<ViewerAction>,
    /// Where the mouse last was (the wheel over the field written by hand).
    pointer: Point,
    /// The overview's thumb held: where in it (pixels from its top) it was taken.
    overview_grab: Option<f32>,
    /// Resolving a conflicted file (a [`MergeInput`]): always composing, into its file.
    resolving: Option<Resolving>,
    /// The text selected in a column (to copy it), and whether the mouse is selecting.
    selection: Option<select::TextSelection>,
    selecting: bool,
    /// Where each column's code starts (past its numbers) and a character's width, from
    /// the last paint (where a press lands in the text).
    code_x: [f32; 3],
    advance: f32,
    /// The clicks in a row of the press being handled (a word with two, a line with three).
    press_clicks: u32,
}

/// A conflicted file being resolved.
#[derive(Debug, Default)]
struct Resolving {
    /// The file written.
    result: PathBuf,
    /// The common ancestor's lines.
    base: Vec<String>,
    /// Per difference: the pick it started with (one side's change alone: that side), it's
    /// a conflict, a choice was made.
    initial: Vec<Pick>,
    conflicts: Vec<bool>,
    chosen: Vec<bool>,
    /// The result was written.
    written: bool,
    /// The result written by hand, in place of the choices (`None`: by the choices).
    by_hand: Option<ui_components::MultilineField>,
}

impl Resolving {
    /// The conflicts left without a choice.
    fn left(&self) -> usize {
        self.conflicts.iter().zip(&self.chosen).filter(|(c, chosen)| **c && !**chosen).count()
    }

    /// Difference `change` is a conflict still waiting for a choice.
    fn open(&self, change: usize) -> bool {
        self.conflicts.get(change).copied().unwrap_or(false)
            && !self.chosen.get(change).copied().unwrap_or(false)
    }
}

fn split_lines(text: &str) -> (Vec<String>, bool, bool) {
    let crlf = text.contains("\r\n");
    let trailing = text.ends_with('\n');
    (text.lines().map(str::to_owned).collect(), crlf, trailing)
}

/// How the resolution stands: the conflicts left, or that it's done (written).
fn resolving_status(resolving: &Resolving) -> String {
    match resolving.left() {
        0 if resolving.written => t("compare.resolved_written"),
        0 => t("compare.resolved"),
        left => tr!("compare.conflicts_left", count = left as i64),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl Compare {
    /// The extension of the files compared (the right one's, else the left's): their
    /// language colors the code.
    fn extension(&self) -> &str {
        [&self.right_path, &self.left_path]
            .into_iter()
            .find_map(|p| p.extension().and_then(|e| e.to_str()))
            .unwrap_or_default()
    }

    /// At least one side can be written (otherwise there's no "Compose"), or the result of a
    /// conflicted file.
    fn writable(&self) -> bool {
        self.resolving.is_some() || self.read_only.contains(&false)
    }

    /// A conflicted file taken to resolve: ours on the left, theirs on the right, composing.
    fn take_merge(&mut self, input: &MergeInput) {
        let (ours, crlf, trailing) = split_lines(&input.ours);
        let (theirs, theirs_crlf, theirs_trailing) = split_lines(&input.theirs);
        self.left_path.clone_from(&input.result);
        self.right_path.clone_from(&input.result);
        self.labels = input.labels.clone().map(Some);
        self.read_only = [true, true];
        self.left = ours;
        self.right = theirs;
        self.crlf = (crlf, theirs_crlf);
        self.trailing_newline = (trailing, theirs_trailing);
        self.merge = true;
        self.resolving = Some(Resolving {
            result: input.result.clone(),
            base: input.base.lines().map(str::to_owned).collect(),
            ..Resolving::default()
        });
        self.scroll = 0.0;
        self.loaded = false;
        self.diff_in_background();
    }

    /// The pick difference `change` goes back to (Both / None clicked again): what it started
    /// with when resolving, else the file being written to.
    fn initial_pick(&self, change: usize) -> Pick {
        match &self.resolving {
            Some(resolving) => resolving.initial.get(change).copied().unwrap_or_default(),
            None => self.default_pick(),
        }
    }

    /// Whether the result can be saved now: something chosen (resolving: every conflict, and
    /// not written yet).
    fn can_save(&self) -> bool {
        match &self.resolving {
            Some(resolving) => {
                !resolving.written && (resolving.by_hand.is_some() || resolving.left() == 0)
            }
            None => self.is_dirty(),
        }
    }

    /// Where the left (and, in "Compose", the result) column ends, as fractions.
    fn ends(&self) -> [f32; 2] {
        if self.merge {
            self.split_merge.unwrap_or([1.0 / 3.0, 2.0 / 3.0])
        } else {
            [self.split_diff.unwrap_or(0.5), 1.0]
        }
    }

    /// The project's sizes: the columns where they were last dragged in the project.
    fn take_sizes(&mut self, sizes: &workbench_model::LayoutSizes) {
        if let Some(at) = sizes.get(DIFF_SPLIT).filter(|a| *a > 0.0 && *a < 1.0) {
            self.split_diff = Some(at);
        }
        if let (Some(a), Some(b)) = (sizes.get(MERGE_LEFT), sizes.get(MERGE_RESULT))
            && 0.0 < a
            && a < b
            && b < 1.0
        {
            self.split_merge = Some([a, b]);
        }
        self.sizes = Some(sizes.clone());
    }

    /// The separators let go: their places kept in the project (its next comparisons, its
    /// next session).
    fn keep_splits(&self) {
        let Some(sizes) = &self.sizes else { return };
        let round = |v: f32| (v * 1000.0).round() / 1000.0;
        if let Some(at) = self.split_diff {
            sizes.set(DIFF_SPLIT, round(at));
        }
        if let Some([a, b]) = self.split_merge {
            sizes.set(MERGE_LEFT, round(a));
            sizes.set(MERGE_RESULT, round(b));
        }
    }

    /// Separator `i` dragged to `x`: the columns on both sides resize (each keeps a minimum).
    fn drag_separator(&mut self, i: usize, x: f32) {
        if self.columns_w <= 0.0 {
            return;
        }
        let min = (self.scale * 120.0 / self.columns_w).min(0.3);
        let at =
            (x - self.list.min_x() - i as f32 * self.gutter - self.gutter / 2.0) / self.columns_w;
        let mut ends = self.ends();
        let low = if i == 0 { 0.0 } else { ends[0] } + min;
        let high = if self.merge && i == 0 { ends[1] } else { 1.0 } - min;
        ends[i] = at.clamp(low, high.max(low));
        if self.merge {
            self.split_merge = Some(ends);
        } else {
            self.split_diff = Some(ends[0]);
        }
    }

    fn changes(&self) -> usize {
        self.chunks.iter().filter(|c| c.is_change()).count()
    }

    /// Where "Compose" writes: the chosen side, never a read-only one.
    fn target(&self) -> Side {
        let writable = |side: Side| !self.read_only[side as usize];
        match self.target.filter(|s| writable(*s)) {
            Some(side) => side,
            None if writable(Side::Left) => Side::Left,
            None => Side::Right,
        }
    }

    /// Default pick: the result is the file being written to (nothing changed).
    fn default_pick(&self) -> Pick {
        match self.target() {
            Side::Left => Pick::Left,
            Side::Right => Pick::Right,
        }
    }

    fn result(&self) -> Vec<String> {
        compose(&self.left, &self.right, &self.chunks, &self.picks)
    }

    fn rebuild_rows(&mut self) {
        // Each side's longest line; the result holds lines of either.
        let longest = |lines: &[String]| lines.iter().map(|l| shown_len(l)).max().unwrap_or(0);
        let (left, right) = (longest(&self.left), longest(&self.right));
        self.widest = [left, left.max(right), right];
        self.rows.clear();
        self.change_rows.clear();
        let mut change = 0;
        for chunk in &self.chunks {
            match chunk {
                Chunk::Equal { left, right } => {
                    for (l, r) in left.clone().zip(right.clone()) {
                        self.rows.push(Row {
                            left: Some(l),
                            result: Some(self.left[l].clone()),
                            right: Some(r),
                            change: None,
                        });
                    }
                }
                Chunk::Change { left, right } => {
                    let pick = self.picks.get(change).copied().unwrap_or_default();
                    let mut result: Vec<String> = Vec::new();
                    if matches!(pick, Pick::Left | Pick::Both) {
                        result.extend(self.left[left.clone()].iter().cloned());
                    }
                    if matches!(pick, Pick::Right | Pick::Both) {
                        result.extend(self.right[right.clone()].iter().cloned());
                    }
                    let rows = left
                        .len()
                        .max(right.len())
                        .max(if self.merge { result.len() } else { 0 })
                        .max(1);
                    self.change_rows.push(self.rows.len());
                    for i in 0..rows {
                        self.rows.push(Row {
                            left: (i < left.len()).then(|| left.start + i),
                            result: result.get(i).cloned(),
                            right: (i < right.len()).then(|| right.start + i),
                            change: Some(change),
                        });
                    }
                    change += 1;
                }
            }
        }
    }

    fn reveal_change(&mut self, i: usize) {
        let Some(&row) = self.change_rows.get(i) else { return };
        self.current = i;
        let view = self.list.size.height;
        self.scroll = (row as f32 * self.row_h - view / 3.0).max(0.0);
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let max =
            (self.rows.len() as f32 * self.row_h - self.list.size.height + self.row_h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
        for across in &mut self.across {
            across.clamp();
        }
    }

    /// The overview's thumb: where the rows in view are along it.
    fn overview_thumb(&self) -> Rect {
        let content = (self.rows.len().max(1) as f32 * self.row_h).max(1.0);
        let o = self.overview;
        let top = self.scroll / content * o.size.height;
        let h = (self.list.size.height / content * o.size.height).min(o.size.height);
        Rect::new(o.min_x(), o.min_y() + top, o.size.width, h)
    }

    /// The thumb's top at `y` (it held `grab` pixels below its top): the rows scrolled
    /// there.
    fn drag_overview(&mut self, y: f32, grab: f32) {
        let content = self.rows.len().max(1) as f32 * self.row_h;
        let top = y - grab - self.overview.min_y();
        self.scroll = top / self.overview.size.height.max(1.0) * content;
        self.clamp_scroll();
    }

    /// The column (0 left, 1 result, 2 right) whose place holds `p`.
    fn column_at(&self, p: Point) -> Option<usize> {
        self.across.iter().position(|a| a.area.contains(p))
    }

    fn set_pick(&mut self, change: usize, pick: Pick) {
        if let Some(p) = self.picks.get_mut(change) {
            *p = pick;
            self.current = change;
            if let Some(chosen) = self.resolving.as_mut().and_then(|r| r.chosen.get_mut(change)) {
                *chosen = true;
            }
            self.rebuild_rows();
        }
    }

    /// The field the result is written by hand in, if it is.
    fn by_hand(&mut self) -> Option<&mut ui_components::MultilineField> {
        self.resolving.as_mut()?.by_hand.as_mut()
    }

    /// The result written by hand from what's composed so far, or back to the choices.
    fn toggle_by_hand(&mut self) {
        let composed = self.result().join("\n");
        if let Some(resolving) = &mut self.resolving {
            resolving.by_hand = match resolving.by_hand {
                Some(_) => None,
                None => {
                    let mut field = ui_components::MultilineField::new(&composed);
                    field.set_text(&composed);
                    Some(field)
                }
            };
        }
    }

    /// Every difference to `pick` ("all from…"); resolving, only the conflicts (what one side
    /// changed alone stays taken).
    fn pick_all(&mut self, pick: Pick) {
        let count = self.changes();
        match &mut self.resolving {
            Some(resolving) => {
                for change in 0..count {
                    if resolving.conflicts.get(change).copied().unwrap_or(false) {
                        self.picks[change] = pick;
                        resolving.chosen[change] = true;
                    }
                }
            }
            None => self.picks = vec![pick; count],
        }
        self.rebuild_rows();
    }
}

/// Where a background thread finds how to wake the app (filled at the first paint).
/// Wakes the app.
type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct WakerSlot(Arc<std::sync::Mutex<Option<Wake>>>);

impl std::fmt::Debug for WakerSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WakerSlot")
    }
}

/// The columns' rectangles (the result's only while composing) and the gutter between them.
struct Columns {
    left: Rect,
    mid: Option<Rect>,
    right: Rect,
    gutter: f32,
}

/// What every row of the columns is drawn with: the fonts, the line numbers' width, the
/// rows in view.
struct Grid {
    font: Font,
    number_font: Font,
    number_w: f32,
    first: usize,
    visible: usize,
}

/// A column's scroll across: how far, how far it can go, the room its code has, where the
/// column is and its bar (from the last paint).
#[derive(Debug, Default)]
struct Across {
    offset: f32,
    max: f32,
    room: f32,
    area: Rect,
    bar: Option<ui_components::Scrollbar>,
}

impl Across {
    fn clamp(&mut self) {
        self.offset = self.offset.clamp(0.0, self.max);
    }

    /// Its bar's state now.
    fn metrics(&self) -> ui_components::ScrollMetrics {
        ui_components::ScrollMetrics {
            content: self.room + self.max,
            viewport: self.room,
            offset: self.offset,
        }
    }

    /// Scrolled to `offset` (kept within its reach). `true` if it moved.
    fn go(&mut self, offset: f32) -> bool {
        let before = self.offset;
        self.offset = offset;
        self.clamp();
        self.offset != before
    }
}

/// A line's length as shown, in characters (a tab as four spaces).
fn shown_len(line: &str) -> usize {
    line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// What of a column is drawn: its background (the tints, the line numbers, the current
/// difference's border: they never scroll across), or its code (clipped after the numbers,
/// scrolled across).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    Back,
    Code,
}

/// A cell to draw: its column and row, its line number and text, the difference's tint
/// and changed part, the gutter's width, the fonts and the baseline.
struct CellSpec<'a> {
    pass: Pass,
    /// How far the code is scrolled across.
    hscroll: f32,
    /// The extension of the file compared (its language colors the code).
    extension: &'a str,
    col: Rect,
    y: f32,
    row_h: f32,
    number: Option<usize>,
    source: Option<&'a str>,
    tint: Option<Rgba>,
    inline: Option<(std::ops::Range<usize>, Rgba)>,
    number_w: f32,
    font: &'a Font,
    number_font: &'a Font,
    base: f32,
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
