//! The grid's state and what the mouse and the keys do to it (its drawing: `paint.rs`).

use std::ops::Range;
use std::time::Instant;

use icons::Icons;
use kernel::{Point, Rect};
use render_api::Scene;
use text_layout::TextSystem;
use ui_kit::TextInput;

use super::MENU_PREFIX;
use super::aggregate::Aggregates;
use super::copy::{Extractor, copy_text};
use super::filter::{
    self, GridFilter, cycle_sort, order_text, parse_order, parse_where, where_text,
};
use super::layout::{ColumnLayout, SavedLayout};
use super::paging::Paging;
use super::selection::{CellPos, Selection};
use super::source::{CellRef, GridQuery, GridSource, RowState, SortKey, SourceStatus};
use super::toolbar::{Tool, ToolbarLayout, ToolbarSpec};
use crate::palette::Palette;
use crate::scrollbar::{EdgeScrollbar, ScrollMetrics, Scrollbar, ScrollbarStyle};
use crate::splitter::Orientation;
use crate::tool_panel::KeyReach;
use crate::viewer::{PressMods, ViewMenuEntry, ViewerCursor, ViewerKey};

mod extractor_list;
mod find_copy;
mod keys;
mod menus;
mod pointer;

/// Heights and paddings (logical pixels).
pub(super) const TOOLBAR_H: f32 = 46.0;
pub(super) const FILTER_H: f32 = 44.0;
pub(super) const FIND_H: f32 = 42.0;
pub(super) const HEADER_H: f32 = crate::table::HEADER_H;
pub(super) const ROW_H: f32 = 28.0;
pub(super) const BOTTOM_H: f32 = 32.0;
pub(super) const CELL_PAD: f32 = 10.0;
/// How near a header's edge the mouse resizes it.
pub(super) const GRIP: f32 = 4.0;
/// A cell's text is cut to this many characters before it's shaped.
pub(super) const CELL_CHARS: usize = 240;
/// At most this many selected cells are added up (a column of millions isn't).
const AGGREGATE_LIMIT: usize = 100_000;
/// At most this many matches of ⌘F are kept.
const FIND_LIMIT: usize = 10_000;

/// What a part of the grid's chrome is (its rows and headers are found by place).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    First,
    Back,
    Forward,
    Last,
    PageSize,
    PageChoice(Option<u32>),
    Refresh,
    Cancel,
    Count,
    Transpose,
    FindToggle,
    Where,
    Order,
    ClearWhere,
    ClearOrder,
    Find,
    ClearFind,
    FindPrev,
    FindNext,
    FindClose,
    RecordPrev,
    RecordNext,
    Corner,
    AddRow,
    DeleteRows,
    Revert,
    Submit,
    /// The "…" holding the tools that didn't fit, and one of its rows.
    More,
    MoreChoice(Tool),
    /// The data extractor's pill ("CSV ▾"), and a row of its popover.
    Extractor,
    ExtractorRow(usize),
    /// The large editor's buttons, and its text.
    BigApply,
    BigCancel,
    BigArea,
}

/// The toolbar's button of `tool` (`None`: a text, the loader).
pub(super) fn tool_hit(tool: Tool) -> Option<Hit> {
    Some(match tool {
        Tool::First => Hit::First,
        Tool::Back => Hit::Back,
        Tool::Forward => Hit::Forward,
        Tool::Last => Hit::Last,
        Tool::PageSize => Hit::PageSize,
        Tool::Cancel => Hit::Cancel,
        Tool::Add => Hit::AddRow,
        Tool::Delete => Hit::DeleteRows,
        Tool::Revert => Hit::Revert,
        Tool::Submit => Hit::Submit,
        Tool::Refresh => Hit::Refresh,
        Tool::Find => Hit::FindToggle,
        Tool::Transpose => Hit::Transpose,
        Tool::More => Hit::More,
        Tool::Extractor => Hit::Extractor,
        Tool::Range | Tool::Of | Tool::Loader => return None,
    })
}

/// The toolbar's tool behind `hit` (`None`: not a toolbar's button).
pub(super) fn hit_tool(hit: Hit) -> Option<Tool> {
    Some(match hit {
        Hit::First => Tool::First,
        Hit::Back => Tool::Back,
        Hit::Forward => Tool::Forward,
        Hit::Last => Tool::Last,
        Hit::PageSize => Tool::PageSize,
        Hit::Cancel => Tool::Cancel,
        Hit::AddRow => Tool::Add,
        Hit::DeleteRows => Tool::Delete,
        Hit::Revert => Tool::Revert,
        Hit::Submit => Tool::Submit,
        Hit::Refresh => Tool::Refresh,
        Hit::FindToggle => Tool::Find,
        Hit::Transpose => Tool::Transpose,
        Hit::More => Tool::More,
        Hit::Extractor => Tool::Extractor,
        _ => return None,
    })
}

/// The command a view runs a toolbar's action by (its keys shown in the tooltips and the
/// "…" menu): given by the view, which knows its commands.
pub type ToolCommands = fn(Tool) -> Option<&'static str>;

/// What a press is dragging.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// A column's right edge (its source index), from where and how wide it was.
    Resize {
        col: usize,
        x: f32,
        width: f32,
    },
    /// A header pressed (its place among those shown): a click sorts, a drag moves it.
    Header {
        pos: usize,
        from: Point,
        shift: bool,
        moved: bool,
    },
    /// Cells being selected.
    Select,
    /// Row numbers being selected (rows).
    Rows,
    VBar,
    HBar,
    /// Text being selected in a field.
    Field(Field),
}

/// What has a tooltip without being clicked: a header's marks, a button that can't be
/// used now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    /// A toolbar's button disabled (the pages' arrows at an end).
    Disabled(Tool),
    /// The column is (part of) the primary key.
    Key,
    /// The rows are ordered by it: which way, its place among the order's columns, how
    /// many there are.
    Sort { descending: bool, place: usize, of: usize },
}

/// Which field has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Where,
    Order,
    Find,
}

/// Where something was right-clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuTarget {
    Cell(CellPos),
    Header(usize),
}

/// What the grid asks of its view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridAction {
    /// This text on the clipboard.
    Copy(String),
    /// The pending changes submitted: shown first, then run.
    Submit,
    /// Every row (the order and filter applied, not just the page) written in `format` (an
    /// extractor's id, or the workbook's: [`super::export::ExportFormat::find`]): to a file
    /// the view asks for, or on the clipboard (a small result).
    Export { format: String, clipboard: bool },
    /// A command run by its id (the extractors' module's: the choice saved, "Configure CSV
    /// Formats…").
    Command(String),
}

/// A row of the data extractors' popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PopRow {
    /// A group's heading.
    Heading(String),
    /// An extractor (one with a problem can't be chosen: why, beside it).
    Choice { id: String, name: String, problem: Option<String> },
    /// "Skip Computed Columns" (`computed`) / "Skip Generated Columns".
    Toggle { computed: bool, label: String },
    /// A command of the extractors' module.
    Action { command: &'static str, label: String },
    /// A line of explanation.
    Note(String),
}

/// What the grid draws with.
pub struct GridPaint<'a> {
    pub bounds: Rect,
    pub scale: f32,
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'a Palette,
    pub icons: &'a Icons,
}

impl std::fmt::Debug for GridPaint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridPaint").field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

/// Where the last paint put things (physical pixels).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Geometry {
    pub bounds: Rect,
    pub scale: f32,
    /// The headers' row.
    pub header: Rect,
    /// The rows' area (under the headers, the row numbers included).
    pub body: Rect,
    /// The row numbers' width.
    pub gutter: f32,
    pub row_h: f32,
    /// The rows shown on the page.
    pub rows: u64,
}

/// ⌘F in the rows.
#[derive(Debug, Default)]
pub(super) struct Find {
    /// Its field (the app's search field: × empties it; its caret while it has the keys).
    pub search: crate::search_field::SearchField,
    pub open: bool,
    pub matches: Vec<CellPos>,
    pub current: usize,
    /// The matches must be looked for again (the text or the rows changed).
    pub stale: bool,
}

/// The grid (see the module's doc), over a source `S`.
pub struct DataGrid<S: GridSource> {
    pub(super) source: S,
    pub(super) paging: Paging,
    pub(super) query: GridQuery,
    pub(super) where_input: TextInput,
    pub(super) order_input: TextInput,
    pub(super) where_error: Option<String>,
    pub(super) order_error: Option<String>,
    pub(super) field: Option<Field>,
    pub(super) layout: ColumnLayout,
    /// A layout to put back once the columns are known.
    pending_layout: Option<SavedLayout>,
    pub(super) selection: Selection,
    /// Rows scrolled (physical pixels), and columns.
    pub(super) scroll_y: f32,
    pub(super) scroll_x: f32,
    pub(super) find: Find,
    /// The field having the keys and why its text isn't understood (drawn last).
    pub(super) field_tip: Option<(String, Rect)>,
    /// One row shown as column – value.
    pub(super) transpose: bool,
    /// The page sizes' popover is open.
    pub(super) sizes_open: bool,
    /// The toolbar's layout for its last width and state.
    pub(super) toolbar: kernel::Memo<ToolbarSpec, ToolbarLayout>,
    /// The tools in the "…" menu now, and whether it's open.
    pub(super) collapsed: Vec<Tool>,
    pub(super) more_open: bool,
    /// What has a tooltip besides the hits (the headers' marks, disabled buttons).
    pub(super) marks: Vec<(Rect, Mark)>,
    /// The commands behind the toolbar's actions (their keys shown).
    pub(super) commands: Option<ToolCommands>,
    pub(super) geometry: Geometry,
    pub(super) hits: Vec<(Rect, Hit)>,
    pub(super) hover: Option<Hit>,
    /// The mouse over a header (its place), for its hover.
    pub(super) hover_header: Option<usize>,
    drag: Option<Drag>,
    /// Where a header being moved would go (its place), drawn as a line.
    pub(super) drop_at: Option<usize>,
    /// What was asked of the source last (asked again only when it changes).
    requested: Option<(GridQuery, Range<u64>)>,
    /// The first row the source said isn't there (the page ends before it).
    pub(super) end: Option<u64>,
    /// The new rows' ids (shown after the page's).
    pub(super) new_rows: Vec<u64>,
    /// Columns not measured yet are, once rows came.
    pub(super) measure: bool,
    /// Which columns were keys when last looked at: one that becomes a key once its
    /// structure is read (after its width was fitted) is fitted again, its key's mark too.
    keyed: Vec<bool>,
    /// A column (its source index) to fit to its content at the next paint.
    pub(super) fit_pending: Option<usize>,
    /// A press in the large editor's text, placed at the next paint (it needs the text).
    pub(super) big_click: Option<Point>,
    action: Option<GridAction>,
    menu_target: Option<MenuTarget>,
    /// The press being handled: its clicks in a row and whether ⇧ was down (a field's
    /// word / all selected, its selection stretched).
    press_clicks: u32,
    press_shift: bool,
    pub(super) vbar: EdgeScrollbar,
    pub(super) hbar: Option<Scrollbar>,
    pub(super) hbar_metrics: ScrollMetrics,
    /// The cell edited in place, and the large editor.
    pub(super) editor: Option<super::edit::CellEditor>,
    pub(super) big: Option<super::edit::BigEditor>,
    /// A cell whose whole value was asked of the source (its place, its column): its large
    /// editor opens once it comes, if the cursor is still on it.
    pub(super) whole_wanted: Option<(u64, usize)>,
    /// Why the last edit wasn't made (shown in the bottom bar until the next change).
    pub(super) notice: Option<String>,
    /// The selection's figures, worked out once per change.
    aggregates: Option<Aggregates>,
    /// Since when the source has been loading (a cancel offered after a moment).
    pub(super) loading_since: Option<Instant>,
    /// Bumped whenever what [`Self::saved_layout`] gives may have changed (a view keeps
    /// it then, not every frame).
    layout_version: u64,
    /// The page size was chosen in the grid (kept with its layout), not given.
    page_chosen: bool,
    /// The project it's shown in (its data extractor is the project's choice).
    project: Option<kernel::ProjectId>,
    /// The extractors' popover: open, its rows, how far they're scrolled, where it was
    /// drawn.
    pub(super) extract_open: bool,
    pub(super) extract_rows: Vec<PopRow>,
    pub(super) extract_scroll: f32,
    pub(super) extract_area: Option<Rect>,
    /// The chosen extractor's name, for the catalog's generation it was read at.
    extract_label: Option<(u64, String)>,
}

impl<S: GridSource> std::fmt::Debug for DataGrid<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataGrid").field("paging", &self.paging).finish_non_exhaustive()
    }
}

impl<S: GridSource> DataGrid<S> {
    /// A grid over `source`, `page` rows a page (`None`: all of them).
    pub fn new(source: S, page: Option<u32>) -> Self {
        Self {
            source,
            paging: Paging::new(page),
            query: GridQuery::default(),
            where_input: TextInput::default(),
            order_input: TextInput::default(),
            where_error: None,
            order_error: None,
            field: None,
            layout: ColumnLayout::default(),
            pending_layout: None,
            selection: Selection::default(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            find: Find::default(),
            field_tip: None,
            transpose: false,
            sizes_open: false,
            toolbar: kernel::Memo::default(),
            collapsed: Vec::new(),
            more_open: false,
            marks: Vec::new(),
            commands: None,
            geometry: Geometry::default(),
            hits: Vec::new(),
            hover: None,
            hover_header: None,
            drag: None,
            drop_at: None,
            requested: None,
            end: None,
            new_rows: Vec::new(),
            measure: true,
            keyed: Vec::new(),
            fit_pending: None,
            big_click: None,
            action: None,
            menu_target: None,
            press_clicks: 1,
            press_shift: false,
            vbar: EdgeScrollbar::default(),
            hbar: None,
            hbar_metrics: ScrollMetrics::default(),
            editor: None,
            big: None,
            whole_wanted: None,
            notice: None,
            aggregates: None,
            loading_since: None,
            layout_version: 0,
            page_chosen: false,
            project: None,
            extract_open: false,
            extract_rows: Vec::new(),
            extract_scroll: 0.0,
            extract_area: None,
            extract_label: None,
        }
    }

    // ─── Data extractors ───

    /// The project it's shown in: ⌘C, "Copy As" and the export use its data extractor.
    pub fn set_project(&mut self, project: Option<kernel::ProjectId>) {
        if self.project != project {
            self.project = project;
            self.extract_label = None;
        }
    }

    pub fn project(&self) -> Option<kernel::ProjectId> {
        self.project
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    pub fn paging(&self) -> Paging {
        self.paging
    }

    pub fn query(&self) -> &GridQuery {
        &self.query
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn transposed(&self) -> bool {
        self.transpose
    }

    /// The commands the view runs the toolbar's actions by: their keys are shown in the
    /// tooltips and the "…" menu.
    pub fn set_commands(&mut self, commands: ToolCommands) {
        self.commands = Some(commands);
    }

    /// The tools that didn't fit the toolbar's last width (in its "…" menu).
    pub fn collapsed_tools(&self) -> &[Tool] {
        &self.collapsed
    }

    /// Where tool `tool`'s button was last drawn (none: not shown).
    pub fn tool_area(&self, tool: Tool) -> Option<Rect> {
        let hit = tool_hit(tool)?;
        let found = self.hits.iter().find(|(_, h)| *h == hit).map(|(r, _)| *r);
        found.or_else(|| {
            self.marks.iter().find(|(_, m)| *m == Mark::Disabled(tool)).map(|(r, _)| *r)
        })
    }

    /// Whether the "…" menu is open.
    pub fn more_open(&self) -> bool {
        self.more_open
    }

    /// Opens the "…" menu (`false`: everything fits, there's none).
    pub fn open_more(&mut self) -> bool {
        self.more_open = !self.collapsed.is_empty();
        self.sizes_open = false;
        self.extract_open = false;
        self.more_open
    }

    /// Tool `tool` run as from its button (the "…" menu's rows). `true` if it could be.
    pub fn run_tool(&mut self, tool: Tool) -> bool {
        let Some(hit) = tool_hit(tool) else { return false };
        if !self.tool_enabled(tool) {
            return false;
        }
        self.press_hit(hit, Point::default())
    }

    /// Whether `tool` can be used now (the pages' arrows at the ends can't).
    pub(super) fn tool_enabled(&self, tool: Tool) -> bool {
        let known = self.source.total().known();
        let rows = self.page_rows();
        let full = self.end.is_none_or(|e| e >= self.paging.start + rows) && rows > 0;
        match tool {
            Tool::First | Tool::Back => self.paging.can_back(),
            Tool::Forward => self.paging.can_forward(known, full),
            Tool::Last => self.paging.can_forward(known, full) && known.is_some(),
            Tool::Add | Tool::Delete => self.can_edit(),
            Tool::Revert | Tool::Submit => self.source.pending() > 0,
            Tool::PageSize => true,
            Tool::Cancel => self.is_loading(),
            _ => true,
        }
    }

    /// Tool `tool`'s keys as the user has them now (none: no command, or no keys).
    pub(super) fn tool_keys(&self, tool: Tool) -> Option<String> {
        let id = self.commands.and_then(|c| c(tool))?;
        crate::shortcuts::label(id)
    }

    /// Another page size (`None`: every row), the first row shown kept in view.
    pub fn set_page_size(&mut self, size: Option<u32>) {
        if self.paging.size == size {
            return;
        }
        self.paging.resize(size);
        self.scroll_y = 0.0;
        self.end = None;
        self.changed_rows();
    }

    /// The columns' layout, the order, the filter and the page size chosen: what's kept
    /// for the table.
    pub fn saved_layout(&self) -> SavedLayout {
        let mut saved = self.layout.save();
        saved.sort = self.query.sort.clone();
        saved.filter = self.query.filter.as_ref().map(where_text).unwrap_or_default();
        saved.page = self.page_chosen.then_some(self.paging.size);
        saved
    }

    /// Changes whenever [`Self::saved_layout`] may have (a resize, a move, a column
    /// hidden, another order, filter or page size).
    pub fn layout_version(&self) -> u64 {
        self.layout_version
    }

    pub(super) fn layout_touched(&mut self) {
        self.layout_version += 1;
    }

    /// A kept layout, put back once the columns are known (now if they are): what names
    /// columns the table no longer has is ignored (a filter not understood any more is
    /// dropped).
    pub fn restore_layout(&mut self, saved: SavedLayout) {
        if let Some(page) = saved.page {
            self.page_chosen = true;
            self.set_page_size(page);
        }
        if self.layout.is_empty() {
            self.pending_layout = Some(saved);
        } else {
            self.apply_saved(&saved);
        }
    }

    /// A kept layout put back over the known columns.
    fn apply_saved(&mut self, saved: &SavedLayout) {
        self.layout.restore(saved);
        let columns = self.source.columns().to_vec();
        let known = |name: &str| columns.iter().any(|c| c.name == name);
        let sort: Vec<SortKey> = saved.sort.iter().filter(|k| known(&k.column)).cloned().collect();
        let filter = parse_where(&saved.filter, &columns).ok().flatten();
        let caps = self.source.capabilities();
        let sort = if caps.sort { sort } else { Vec::new() };
        let filter = if caps.filter { filter } else { None };
        if sort != self.query.sort || filter != self.query.filter {
            self.order_input.set_text(&order_text(&sort));
            self.where_input.set_text(&filter.as_ref().map(where_text).unwrap_or_default());
            self.query.sort = sort;
            self.query.filter = filter;
            self.query_changed();
        }
    }

    /// The page size chosen in the grid (its popover): kept with the table's layout.
    pub fn choose_page_size(&mut self, size: Option<u32>) {
        self.page_chosen = true;
        self.set_page_size(size);
        self.layout_touched();
    }

    /// An order from outside (a header's menu).
    pub fn set_sort(&mut self, keys: Vec<SortKey>) {
        self.query.sort = keys;
        self.order_input.set_text(&order_text(&self.query.sort));
        self.order_error = None;
        self.query_changed();
    }

    /// A filter from outside.
    pub fn set_filter(&mut self, filter: Option<GridFilter>) {
        self.where_input.set_text(&filter.as_ref().map(where_text).unwrap_or_default());
        self.query.filter = filter;
        self.where_error = None;
        self.query_changed();
    }

    /// The source's columns shown, in their order (what an export writes).
    pub fn shown_columns(&self) -> Vec<usize> {
        self.layout.visible()
    }

    /// What the grid asks of its view (picked up after a key, a command).
    pub fn take_action(&mut self) -> Option<GridAction> {
        self.action.take()
    }

    /// The source's news: `true` if something changed (drawn again).
    pub fn poll(&mut self) -> bool {
        let changed = self.source.poll();
        if changed {
            self.sync_columns();
            self.changed_data();
            self.follow_whole();
        }
        let loading = self.source.status() == SourceStatus::Loading;
        match (loading, self.loading_since) {
            (true, None) => self.loading_since = Some(Instant::now()),
            (false, Some(_)) => self.loading_since = None,
            _ => {}
        }
        changed
    }

    /// Reading something (a loader turns).
    pub fn is_loading(&self) -> bool {
        self.source.status() == SourceStatus::Loading
    }

    /// The columns changed (another structure): a new layout, unless they're the same.
    fn sync_columns(&mut self) {
        let names: Vec<String> = self.source.columns().iter().map(|c| c.name.clone()).collect();
        let keyed: Vec<bool> = self.source.columns().iter().map(|c| c.key).collect();
        let before = std::mem::replace(&mut self.keyed, keyed);
        if self.layout.same_columns(&names) {
            // The same columns, a key found since (the structure read after the rows): its
            // width fitted again, with room for the key's mark.
            if before.len() == self.keyed.len() {
                for col in (0..self.keyed.len()).filter(|&c| self.keyed[c] && !before[c]) {
                    self.layout.unmeasure(col);
                    self.measure = true;
                }
            }
            return;
        }
        let kept = (!self.layout.is_empty()).then(|| self.layout.save());
        self.layout = ColumnLayout::new(&names);
        if let Some(saved) = self.pending_layout.take() {
            self.apply_saved(&saved);
        } else if let Some(kept) = kept {
            self.layout.restore(&kept);
        }
        self.measure = true;
        self.selection.clear();
    }

    /// Rows came or went: what's derived from them is worked out again.
    fn changed_data(&mut self) {
        self.find.stale = true;
        self.aggregates = None;
        self.measure |= (0..self.layout.len()).any(|c| !self.layout.is_measured(c));
    }

    /// Another page, size or order: the view starts at its top.
    fn changed_rows(&mut self) {
        self.selection.clear();
        self.changed_data();
    }

    fn query_changed(&mut self) {
        self.layout_touched();
        self.paging.first();
        self.scroll_y = 0.0;
        self.end = None;
        self.changed_rows();
    }

    /// The page's rows, as far as known: total counted, the rows left of it; else a whole
    /// page, up to the first the source said isn't there.
    pub(super) fn page_rows(&self) -> u64 {
        let total = self.source.total();
        let known = total.known();
        let loaded = match total.approximate().or(total.at_least()) {
            Some(n) => n,
            None => self.end.unwrap_or_else(|| self.paging.start + 200),
        };
        let mut rows = self.paging.rows(known, loaded);
        if let Some(end) = self.end {
            rows = rows.min(end.saturating_sub(self.paging.start));
        }
        rows
    }

    /// The rows shown: the page's, then the new ones (not submitted yet).
    pub(super) fn shown_rows(&self) -> u64 {
        self.page_rows() + self.new_rows.len() as u64
    }

    /// The source's row at place `i` of those shown (past the page's: a new row's id).
    pub(super) fn row_at(&self, i: u64) -> u64 {
        let rows = self.page_rows();
        if i < rows {
            self.paging.start + i
        } else {
            self.new_rows.get((i - rows) as usize).copied().unwrap_or(super::source::NEW_ROWS)
        }
    }

    /// The source's row at place `place` of those shown (a selection's), for a view.
    pub fn row_of(&self, place: u64) -> u64 {
        self.row_at(place)
    }

    /// The source's column shown at place `pos` (a selection's), for a view.
    pub fn source_column(&self, pos: usize) -> Option<usize> {
        self.layout.visible().get(pos).copied()
    }

    /// The new rows as the source has them now (at each paint and after an edit).
    pub(super) fn sync_new_rows(&mut self) {
        self.new_rows = self.source.new_rows();
    }

    /// The rows in view (the page's places), and asks the source for them (a screen more
    /// on each side) when that changed.
    pub(super) fn request_visible(&mut self, body_h: f32) {
        let rows = self.page_rows();
        let row_h = self.geometry.row_h.max(1.0);
        let first = (self.scroll_y / row_h).floor().max(0.0) as u64;
        let count = (body_h / row_h).ceil() as u64 + 1;
        let from = first.saturating_sub(count);
        // All rows, total not known: a little past what's known, to find where they end.
        let to = if self.paging.size.is_none() && self.source.total().known().is_none() {
            first + count * 2
        } else {
            (first + count * 2).min(rows)
        };
        let range = self.paging.start + from..self.paging.start + to.max(from);
        let asked = Some((self.query.clone(), range.clone()));
        if self.requested != asked {
            self.source.request(&self.query, range);
            self.requested = asked;
        }
    }

    /// Notes rows the source says aren't there (where the page ends).
    pub(super) fn note_missing(&mut self, row: u64) {
        if self.end.is_none_or(|e| row < e) && self.source.row_state(row) == RowState::Missing {
            self.end = Some(row);
        }
    }

    // ─── Keys and text ───

    /// The selection's figures worked out again (its values changed).
    pub(super) fn aggregates_stale(&mut self) {
        self.aggregates = None;
    }

    pub(super) fn set_action(&mut self, action: GridAction) {
        self.action = Some(action);
    }

    /// The cursor's cell scrolled into view.
    pub(super) fn reveal(&mut self) {
        self.reveal_cursor();
    }

    /// The cursor's cell scrolled into view.
    fn reveal_cursor(&mut self) {
        let Some(cursor) = self.selection.cursor() else { return };
        let g = self.geometry;
        if g.row_h <= 0.0 {
            return;
        }
        let i = cursor.row as f32;
        let top = i * g.row_h;
        let view = g.body.size.height - g.row_h;
        if top < self.scroll_y {
            self.scroll_y = top;
        } else if top + g.row_h > self.scroll_y + view.max(g.row_h) {
            self.scroll_y = top + g.row_h - view.max(g.row_h);
        }
        let edges = self.layout.edges();
        if let (Some(&left), Some(&right)) = (edges.get(cursor.col), edges.get(cursor.col + 1)) {
            let (left, right) = (left * g.scale, right * g.scale);
            let room = g.body.size.width - g.gutter;
            if left < self.scroll_x {
                self.scroll_x = left;
            } else if right > self.scroll_x + room {
                self.scroll_x = (right - room).max(0.0);
            }
        }
    }

    /// ⌘A: the text being edited (a cell, a field) all selected, else every cell of the
    /// page. `false`: no rows.
    pub fn select_all(&mut self) -> bool {
        // The text being edited first: never the cells under it.
        if let Some(big) = &mut self.big {
            big.area.select_all();
            return true;
        }
        if let Some(editor) = &mut self.editor {
            editor.input.select_all();
            return true;
        }
        if let Some(field) = self.field {
            self.input_mut(field).select_all();
            return true;
        }
        let Some((first, last)) = self.rows_span() else { return false };
        self.selection.all(first, last, self.layout.visible().len());
        self.aggregates = None;
        true
    }

    // ─── Find ───

    // ─── Transposed ───

    fn toggle_transpose(&mut self) -> bool {
        self.transpose = !self.transpose;
        if self.transpose && self.selection.cursor().is_none() && self.page_rows() > 0 {
            self.selection.set(CellPos::new(0, 0));
        }
        self.scroll_y = 0.0;
        true
    }

    /// The place of the row shown transposed (the cursor's, else the first).
    pub(super) fn record_row(&self) -> Option<u64> {
        let rows = self.shown_rows();
        (rows > 0).then(|| self.selection.cursor().map_or(0, |c| c.row).min(rows - 1))
    }

    // ─── Copy, figures ───

    // ─── The mouse ───

    // ─── Menus and commands ───
}

/// The rows of a menu of extractors, a group after the other (separated): `command:<id>`
/// each; `ask` labels them as asking for a file (`CSV…`); `workbook` adds Excel's last.
fn extractor_entries(
    project: Option<kernel::ProjectId>,
    command: &str,
    ask: bool,
    workbook: bool,
) -> Vec<ViewMenuEntry> {
    let mut entries = Vec::new();
    let mut last = None;
    for format in super::export::ExportFormat::offered(project, !workbook) {
        let group = match &format {
            super::export::ExportFormat::Text(e) => Some(e.group()),
            super::export::ExportFormat::Xlsx => None,
        };
        if !entries.is_empty() && last != group {
            entries.push(ViewMenuEntry::Separator);
        }
        last = group;
        let label = if ask { format!("{}…", format.name()) } else { format.name() };
        entries.push(ViewMenuEntry::item(format!("{MENU_PREFIX}{command}:{}", format.id()), label));
    }
    entries
}

/// "Export All Rows ›" (to a file, a format each: every data extractor, and an Excel
/// workbook) and "Copy All Rows as ›" (the clipboard, for a small result: the extractors).
pub fn export_menu(project: Option<kernel::ProjectId>) -> Vec<ViewMenuEntry> {
    vec![
        ViewMenuEntry::Submenu {
            label: i18n::t("grid.export"),
            icon: Some("download"),
            entries: extractor_entries(project, "export", true, true),
        },
        ViewMenuEntry::Submenu {
            label: i18n::t("grid.export_copy"),
            icon: Some("copy"),
            entries: extractor_entries(project, "export-copy", false, false),
        },
    ]
}

/// A value as a filter's menu row names it (`None`: one that can't be filtered on: bytes).
fn filter_value_label(value: &super::source::CellValue) -> Option<String> {
    use super::source::CellValue;
    let text = match value {
        CellValue::Null => return None,
        CellValue::Bytes(_) => return None,
        CellValue::Text(t) | CellValue::Json(t) | CellValue::Other(t) => format!("'{t}'"),
        other => other.as_ref().display(),
    };
    let short: String = text.chars().take(40).collect();
    Some(if short.len() < text.len() { format!("{short}…") } else { short })
}

/// A scrollbar for the columns (along the rows' bottom).
pub(super) fn horizontal_bar(palette: &Palette) -> Scrollbar {
    Scrollbar::new(ScrollbarStyle::from_palette(palette), Orientation::Horizontal)
}

/// A data grid as a form's control: a press on a cell, a header, a column's edge or a tool
/// (⇧ / ⌘ extend the choice), held while a selection, a column or its scrollbars are dragged;
/// with the focus its keys, its cell's editing and its commands. What a press decided is
/// taken from it by its view ([`DataGrid::take_action`]).
impl<S: GridSource> crate::control::Control for DataGrid<S> {
    fn area(&self) -> Option<Rect> {
        Some(self.geometry.bounds).filter(|b| b.size.width > 0.0)
    }

    fn press_with(
        &mut self,
        p: Point,
        clicks: u32,
        mods: crate::PressMods,
    ) -> crate::control::Press {
        DataGrid::press(self, p, clicks, mods);
        crate::control::Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        DataGrid::drag(self, p)
    }

    fn release(&mut self) -> bool {
        DataGrid::release(self)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        DataGrid::hover(self, p.unwrap_or(crate::control::NOWHERE))
    }

    fn cursor(&self, p: Point) -> Option<crate::ViewerCursor> {
        self.cursor_at(p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        DataGrid::scroll(self, dx, dy)
    }

    fn focusable(&self) -> bool {
        true
    }

    /// Tab moves between its cells.
    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: crate::ViewerKey, reach: crate::KeyReach) -> bool {
        self.edit_key(key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        self.text_input(typed)
    }

    /// A character over the selected cell with no field open: its editor starts with it.
    fn char_key(&mut self, key: char) -> bool {
        self.type_char(key)
    }

    fn selected_text(&self) -> Option<String> {
        DataGrid::selected_text(self)
    }

    /// Its commands, and ⌘A: all its cells chosen.
    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => self.select_all(),
            _ => DataGrid::command(self, id),
        }
    }

    fn tooltip(&self, p: Point) -> Option<crate::tooltip::Tip> {
        self.tooltip_at(p)
    }

    fn menu(&mut self, p: Point) -> Option<Vec<crate::ViewMenuEntry>> {
        self.context_menu(p)
    }
}
