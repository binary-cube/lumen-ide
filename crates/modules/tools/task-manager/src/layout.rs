//! The Task Manager's page laid out for the room its view has — a whole tab, a split dragged
//! to 250 px, a short window — as a responsive web page: pure, from a [`LayoutSpec`] (the
//! view's bounds and scale, what it shows, its texts' widths) to a [`PageLayout`] (every
//! region in physical pixels, the toolbar's items, the table's columns). Its user keeps the
//! last one ([`LayoutCache`]): nothing is worked out again while the bounds and the state
//! are the same, and while a separator is dragged only this (O(columns + regions)) runs.
//!
//! The width makes a class ([`WidthClass`], logical pixels: ≥ 1080 wide, 760–1079 medium,
//! 480–759 narrow, 320–479 compact, < 320 minimal), changed only 8 px past a breakpoint
//! from the current one ([`width_class`]: no flicker while a separator is dragged along
//! one). Each class has its summary (four cards in a row, two by two, a band of pills, a
//! line of text), its table (columns by priority with their "⋯" chooser, or a list of
//! two-line or one-line rows; the columns keep the widths the user dragged them to, the
//! last one taking what's left) and its detail's place (on the right, below, or a page in
//! place of the table). The height has its say too: under 420 px the summary is the band,
//! under 260 it's gone (the status line stays); the detail goes below only where there's
//! height for it, else to a page.

use kernel::Rect;
use ui_components::Orientation;
use ui_components::columns::{self, Choice, ColumnFit, ColumnSpec};
use ui_components::overflow::{self, OverflowItem, OverflowLayout, OverflowSpec, Side};

/// How far the columns' chooser ("⋯") stays from the table's right edge (logical pixels).
const CHOOSER_GAP: f32 = 10.0;

use crate::model::Grouping;

/// The breakpoints between the classes (logical pixels): compact, narrow, medium, wide.
pub const BREAKPOINTS: [f32; 4] = [320.0, 480.0, 760.0, 1080.0];
/// How far past a breakpoint the width must go to change the class (logical pixels).
pub const HYSTERESIS: f32 = 8.0;

/// How wide the page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WidthClass {
    /// < 320: a line of text, a list of one-line rows.
    Minimal,
    /// 320–479: a band of pills, a list of two-line rows, the detail on a page.
    Compact,
    /// 480–759: cards two by two, compact rows, the columns by priority.
    Narrow,
    /// 760–1079: four cards, the detail below.
    Medium,
    /// ≥ 1080: four cards, every column that fits with the trend, the detail on the right.
    Wide,
}

impl WidthClass {
    pub const ALL: [WidthClass; 5] = [
        WidthClass::Minimal,
        WidthClass::Compact,
        WidthClass::Narrow,
        WidthClass::Medium,
        WidthClass::Wide,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// Its narrowest width (logical pixels).
    pub fn lower(self) -> f32 {
        match self.index() {
            0 => 0.0,
            i => BREAKPOINTS[i - 1],
        }
    }
}

/// The class of `width` (logical pixels), without hysteresis.
pub fn class_of(width: f32) -> WidthClass {
    let i = BREAKPOINTS.iter().filter(|b| width >= **b).count();
    WidthClass::ALL[i]
}

/// The class of `width` coming from `previous`: it changes only once the width is
/// [`HYSTERESIS`] past the breakpoint (either way).
pub fn width_class(width: f32, previous: Option<WidthClass>) -> WidthClass {
    let raw = class_of(width);
    let Some(previous) = previous else { return raw };
    let mut class = raw;
    // Up: each step needs the width well inside it.
    while class > previous && width < class.lower() + HYSTERESIS {
        class = WidthClass::ALL[class.index() - 1];
    }
    // Down: each step needs the width well under the one above it.
    while class < previous && width >= WidthClass::ALL[class.index() + 1].lower() - HYSTERESIS {
        class = WidthClass::ALL[class.index() + 1];
    }
    class
}

/// A column of the parts' table (its id in the settings: [`ColumnId::id`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnId {
    Name,
    Cpu,
    Memory,
    Network,
    Disk,
    /// Its project's name (every project's view, not grouped by project).
    Project,
    Gpu,
    /// The CPU over time, a small sparkline (wide pages only).
    Trend,
    Threads,
    Detail,
}

/// The columns, in the table's order.
pub const COLUMNS: [ColumnId; 10] = [
    ColumnId::Name,
    ColumnId::Cpu,
    ColumnId::Memory,
    ColumnId::Network,
    ColumnId::Disk,
    ColumnId::Project,
    ColumnId::Gpu,
    ColumnId::Trend,
    ColumnId::Threads,
    ColumnId::Detail,
];

impl ColumnId {
    pub fn index(self) -> usize {
        self as usize
    }

    /// Its id in `[tasks] columns` (the name has none: it's always there).
    pub fn id(self) -> &'static str {
        match self {
            ColumnId::Name => "name",
            ColumnId::Cpu => "cpu",
            ColumnId::Memory => "memory",
            ColumnId::Network => "network",
            ColumnId::Disk => "disk",
            ColumnId::Project => "project",
            ColumnId::Gpu => "gpu",
            ColumnId::Trend => "trend",
            ColumnId::Threads => "threads",
            ColumnId::Detail => "detail",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        COLUMNS.into_iter().find(|c| c.id() == id)
    }

    /// Its priority (0 never leaves; the higher, the sooner it leaves) and its width
    /// (logical pixels: a flexible one's least and most).
    pub fn spec(self) -> (u8, f32, f32) {
        match self {
            ColumnId::Name => (0, 150.0, f32::INFINITY),
            ColumnId::Cpu => (1, 64.0, 64.0),
            ColumnId::Memory => (2, 76.0, 76.0),
            ColumnId::Network => (3, 80.0, 80.0),
            ColumnId::Disk => (4, 80.0, 80.0),
            ColumnId::Project => (4, 120.0, 120.0),
            ColumnId::Gpu => (5, 72.0, 72.0),
            ColumnId::Trend => (6, 72.0, 72.0),
            ColumnId::Threads => (7, 48.0, 48.0),
            ColumnId::Detail => (8, 160.0, 360.0),
        }
    }

    /// The least width its edge may be dragged to (logical pixels; a number's column never
    /// under its widest text either).
    pub fn min_width(self) -> f32 {
        match self {
            ColumnId::Name => 80.0,
            ColumnId::Project | ColumnId::Detail => 60.0,
            ColumnId::Trend => 40.0,
            _ => 32.0,
        }
    }

    /// Its numbers sit on the right.
    pub fn numeric(self) -> bool {
        !matches!(self, ColumnId::Name | ColumnId::Project | ColumnId::Trend | ColumnId::Detail)
    }
}

/// A control of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// This project / every project.
    Scope,
    /// By kind / project / a list.
    Grouping,
    /// The filter field (or, on a narrow page, its magnifier opening it over the bar).
    Filter,
    /// Pause / resume what's shown.
    Pause,
    /// How often it looks and how far back its graphs go (a small menu).
    Interval,
    /// "Sort by…" (the lists of the narrow classes, which have no headers).
    Sort,
}

/// The toolbar's texts' widths (physical pixels: as the view measured them).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ToolWidths {
    pub scope: f32,
    pub grouping: f32,
    pub interval: f32,
    pub sort: f32,
}

/// What the layout is worked out from.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutSpec {
    /// The view's area (physical pixels) and its scale.
    pub bounds: Rect,
    pub scale: f32,
    /// The GPU's memory is known (its card and its column).
    pub has_gpu: bool,
    /// A part's detail is open.
    pub detail_open: bool,
    /// Every project's view.
    pub scope_all: bool,
    pub grouping: Grouping,
    /// What the user chose of each column (by [`COLUMNS`]).
    pub choices: [Choice; COLUMNS.len()],
    /// The widths the user dragged the columns to (logical pixels, 0: as their spec says),
    /// by [`COLUMNS`].
    pub widths: [f32; COLUMNS.len()],
    /// The column the table is sorted by.
    pub sorted_by: Option<ColumnId>,
    /// Each column's widest text (physical pixels, 0: not measured), by [`COLUMNS`].
    pub text_widths: [f32; COLUMNS.len()],
    pub tools: ToolWidths,
    /// The detail's share of the page's width on the right, and of its height below
    /// (0..1).
    pub ratio_right: f32,
    pub ratio_below: f32,
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            bounds: Rect::default(),
            scale: 1.0,
            has_gpu: true,
            detail_open: false,
            scope_all: false,
            grouping: Grouping::Kind,
            choices: [Choice::Auto; COLUMNS.len()],
            widths: [0.0; COLUMNS.len()],
            sorted_by: Some(ColumnId::Cpu),
            text_widths: [0.0; COLUMNS.len()],
            tools: ToolWidths::default(),
            ratio_right: 0.30,
            ratio_below: 0.45,
        }
    }
}

/// How the summary is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryForm {
    /// Metric cards (in one row or two by two).
    Cards,
    /// A band of pills (value and a micro-sparkline).
    Pills,
    /// A line of text (`CPU 3.4% · 412 MB`).
    Line,
    /// None (a page too short): the status line says it.
    None,
}

/// The summary's area and its cards (or pills).
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub form: SummaryForm,
    pub area: Rect,
    pub cards: Vec<Rect>,
}

/// How the parts are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    /// A table: headers and columns.
    Table,
    /// Rows of two lines: the name and the sorted metric, the others under.
    TwoLines,
    /// Rows of one line: the name and the sorted metric.
    OneLine,
}

/// Where a part's detail opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailPlace {
    Right,
    Below,
    /// A page in place of the summary and the table, with "‹ Back".
    Page,
}

/// How the detail lays out its charts and figures on its width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailLayout {
    /// Charts in a row (2: two by two).
    pub chart_columns: usize,
    /// Figures in a row.
    pub stat_columns: usize,
    /// A chart's height (physical pixels).
    pub chart_height: f32,
}

/// The detail's layout on `width` (physical pixels) at `scale`: two by two past 420 px, the
/// figures in two columns past 360.
pub fn detail_layout(width: f32, scale: f32) -> DetailLayout {
    let logical = width / scale.max(0.1);
    DetailLayout {
        chart_columns: if logical > 420.0 { 2 } else { 1 },
        stat_columns: if logical > 360.0 { 2 } else { 1 },
        chart_height: (96.0 * scale).round(),
    }
}

/// Every region of the page (physical pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    pub class: WidthClass,
    pub bounds: Rect,
    pub toolbar: Rect,
    /// The toolbar's items (their x and width, inside `toolbar`) and its "…".
    pub tools: OverflowLayout<Tool>,
    /// The filter is a magnifier (it opens over the bar), not a field.
    pub filter_icon: bool,
    /// The line under the bar: how many threads, how often, how far back.
    pub status: Option<Rect>,
    pub summary: Summary,
    /// The parts' area (header and rows); `None` while the detail is a page.
    pub table: Option<Rect>,
    /// The columns' header (a table, not a list).
    pub header: Option<Rect>,
    /// The rows' area.
    pub rows: Rect,
    pub columns: ColumnFit<ColumnId>,
    /// The "⋯" at the header's end (the columns' chooser).
    pub chooser: Option<Rect>,
    pub row_height: f32,
    pub list_mode: ListMode,
    /// Where the detail opens (when it does).
    pub place: DetailPlace,
    /// The detail's area, when open.
    pub detail: Option<Rect>,
    /// The strip between the table and the detail (dragged to resize), and how it lies.
    pub splitter: Option<(Rect, Orientation)>,
    pub detail_layout: Option<DetailLayout>,
}

/// The table's least width beside the detail, and the detail's (logical pixels).
pub const TABLE_MIN: f32 = 320.0;
pub const DETAIL_MIN: f32 = 280.0;
/// The detail on the right: its width's range (logical pixels).
pub const DETAIL_RIGHT: (f32, f32) = (300.0, 520.0);
/// The strip between the table and the detail (logical pixels): its line and handle in the
/// middle, room on each side (the table's scrollbar never against it).
pub const SPLITTER: f32 = 20.0;
/// A row's least height at any scale (logical pixels: something to click).
pub const MIN_ROW: f32 = 24.0;

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x.round(), y.round(), w.max(0.0).round(), h.max(0.0).round())
}

/// The page laid out for `spec` (`previous`: the class it had, for the hysteresis).
pub fn layout(spec: &LayoutSpec, previous: Option<WidthClass>) -> PageLayout {
    let scale = spec.scale.max(0.1);
    let px = |v: f32| (v * scale).round();
    let b = spec.bounds;
    let logical_w = b.size.width / scale;
    let logical_h = b.size.height / scale;
    let class = width_class(logical_w, previous);
    let narrowish = class <= WidthClass::Narrow;
    // The lists' pages keep their edges closer (the tables' the same, so a column doesn't
    // leave as the class changes).
    let pad = if class <= WidthClass::Compact { px(8.0) } else { px(12.0) };
    let x0 = b.min_x() + pad;
    let inner_w = (b.size.width - 2.0 * pad).max(0.0);
    let bottom = b.max_y() - pad.min(px(8.0));

    // The toolbar.
    let toolbar_h = px(40.0);
    let toolbar = r(x0, b.min_y() + px(4.0), inner_w, toolbar_h.min(b.size.height));
    let list_mode = match class {
        WidthClass::Minimal => ListMode::OneLine,
        WidthClass::Compact => ListMode::TwoLines,
        _ => ListMode::Table,
    };
    let filter_icon = logical_w < 360.0;
    let (tools, _) = toolbar_items(spec, toolbar, filter_icon, list_mode != ListMode::Table);
    let mut y = toolbar.max_y();

    // The status line (the minimal page's summary line takes its place).
    let status = (class != WidthClass::Minimal && logical_h >= 160.0).then(|| {
        let line = r(x0, y, inner_w, px(20.0));
        y = line.max_y();
        line
    });
    y += px(6.0);

    let place = detail_place(class, logical_w - 2.0 * pad / scale, logical_h);
    let page = spec.detail_open && place == DetailPlace::Page;

    // The summary.
    let form = if page {
        SummaryForm::None
    } else if class == WidthClass::Minimal {
        SummaryForm::Line
    } else if logical_h < 260.0 {
        SummaryForm::None
    } else if logical_h < 420.0 || class == WidthClass::Compact {
        SummaryForm::Pills
    } else {
        SummaryForm::Cards
    };
    let count: usize = if spec.has_gpu { 4 } else { 3 };
    let gap = px(10.0);
    let (per_row, card_h) = match (form, class) {
        (SummaryForm::Cards, WidthClass::Wide) => (count, px(96.0)),
        (SummaryForm::Cards, WidthClass::Medium) => (count, px(80.0)),
        (SummaryForm::Cards, _) => (if count == 3 { 3 } else { 2 }, px(68.0)),
        (SummaryForm::Pills, _) if logical_w < 400.0 && count == 4 => (2, px(36.0)),
        (SummaryForm::Pills, _) => (count, px(36.0)),
        (SummaryForm::Line, _) => (1, px(20.0)),
        (SummaryForm::None, _) => (1, 0.0),
    };
    let mut cards = Vec::new();
    let summary_area = match form {
        SummaryForm::None => r(x0, y, inner_w, 0.0),
        SummaryForm::Line => {
            let area = r(x0, y, inner_w, card_h);
            y = area.max_y() + px(6.0);
            area
        }
        SummaryForm::Cards | SummaryForm::Pills => {
            let rows = count.div_ceil(per_row);
            let card_w = ((inner_w - gap * (per_row - 1) as f32) / per_row as f32).floor();
            for i in 0..count {
                let (row, col) = (i / per_row, i % per_row);
                let cx = x0 + col as f32 * (card_w + gap);
                // The last of a row takes what the rounding left.
                let w = if col + 1 == per_row { x0 + inner_w - cx } else { card_w };
                cards.push(r(cx, y + row as f32 * (card_h + gap), w, card_h));
            }
            let h = rows as f32 * card_h + (rows - 1) as f32 * gap;
            let area = r(x0, y, inner_w, h);
            y = area.max_y() + px(12.0);
            area
        }
    };
    // Nothing past the bottom: a summary that doesn't fit isn't drawn.
    let summary = if summary_area.max_y() > bottom {
        y = summary_area.min_y();
        Summary { form: SummaryForm::None, area: r(x0, y, inner_w, 0.0), cards: Vec::new() }
    } else {
        Summary { form, area: summary_area, cards }
    };

    let content = r(x0, y, inner_w, bottom - y);
    let row_height = match list_mode {
        ListMode::Table if narrowish => px(26.0),
        ListMode::Table => px(32.0),
        ListMode::TwoLines => px(44.0),
        ListMode::OneLine => px(30.0),
    }
    .max(px(MIN_ROW));

    let mut out = PageLayout {
        class,
        bounds: b,
        toolbar,
        tools,
        filter_icon,
        status,
        summary,
        table: None,
        header: None,
        rows: Rect::default(),
        columns: ColumnFit::default(),
        chooser: None,
        row_height,
        list_mode,
        place,
        detail: None,
        splitter: None,
        detail_layout: None,
    };
    if page {
        out.detail = Some(content);
        out.detail_layout = Some(detail_layout(content.size.width, scale));
        return out;
    }
    // The table and, beside or under it, the detail.
    let strip = px(SPLITTER);
    let (table, detail) = match (spec.detail_open, place) {
        (true, DetailPlace::Right) => {
            let room = content.size.width - strip;
            let (least, most) = (px(DETAIL_RIGHT.0), px(DETAIL_RIGHT.1));
            let wanted = (content.size.width * spec.ratio_right.clamp(0.15, 0.6)).round();
            let w = wanted.clamp(least, most).min(room - px(TABLE_MIN)).max(px(DETAIL_MIN));
            let table = r(content.min_x(), content.min_y(), room - w, content.size.height);
            let detail = r(table.max_x() + strip, content.min_y(), w, content.size.height);
            out.splitter = Some((
                r(table.max_x(), content.min_y(), strip, content.size.height),
                Orientation::Vertical,
            ));
            (table, Some(detail))
        }
        (true, DetailPlace::Below) => {
            let room = content.size.height - strip;
            let wanted = (content.size.height * spec.ratio_below.clamp(0.2, 0.8)).round();
            let h = wanted.max(px(220.0)).min(room - px(120.0)).max(0.0);
            let table = r(content.min_x(), content.min_y(), content.size.width, room - h);
            let detail = r(content.min_x(), table.max_y() + strip, content.size.width, h);
            out.splitter = Some((
                r(content.min_x(), table.max_y(), content.size.width, strip),
                Orientation::Horizontal,
            ));
            (table, Some(detail))
        }
        _ => (content, None),
    };
    out.detail = detail;
    out.detail_layout = detail.map(|d| detail_layout(d.size.width, scale));
    out.table = Some(table);
    if list_mode == ListMode::Table {
        let header_h = px(ui_components::table::HEADER_H);
        let header = r(table.min_x(), table.min_y(), table.size.width, header_h);
        // The columns' chooser: a square button, centered in the header, kept off the
        // table's rounded edge.
        let chooser_w = px(28.0);
        let chooser_gap = px(CHOOSER_GAP);
        out.chooser = Some(r(
            header.max_x() - chooser_gap - chooser_w,
            header.min_y() + (header_h - chooser_w) / 2.0,
            chooser_w,
            chooser_w,
        ));
        out.columns = fit_columns(spec, class, table, chooser_w + chooser_gap + px(4.0));
        out.header = Some(header);
        out.rows =
            r(table.min_x(), header.max_y(), table.size.width, table.max_y() - header.max_y());
    } else {
        out.rows = table;
    }
    out
}

/// Where a part's detail opens for `class` on a page `width` × `height` (logical pixels).
pub fn detail_place(class: WidthClass, width: f32, height: f32) -> DetailPlace {
    match class {
        WidthClass::Wide if width >= TABLE_MIN + DETAIL_MIN + SPLITTER => DetailPlace::Right,
        WidthClass::Wide | WidthClass::Medium if height >= 560.0 => DetailPlace::Below,
        WidthClass::Narrow if height >= 640.0 => DetailPlace::Below,
        _ => DetailPlace::Page,
    }
}

/// The columns present for `spec` and `class`: the project's only in every project's view
/// not grouped by project, the GPU's when it's known, the trend only on a wide page.
pub fn present(spec: &LayoutSpec, class: WidthClass) -> impl Iterator<Item = ColumnId> + '_ {
    COLUMNS.into_iter().filter(move |c| match c {
        ColumnId::Project => spec.scope_all && spec.grouping != Grouping::Project,
        ColumnId::Gpu => spec.has_gpu,
        ColumnId::Trend => class == WidthClass::Wide,
        _ => true,
    })
}

/// The table's columns in `table` (the chooser's room left at its end).
fn fit_columns(
    spec: &LayoutSpec,
    class: WidthClass,
    table: Rect,
    chooser: f32,
) -> ColumnFit<ColumnId> {
    let px = |v: f32| (v * spec.scale).round();
    let specs: Vec<ColumnSpec<ColumnId>> = present(spec, class)
        .map(|c| {
            let (priority, least, most) = c.spec();
            let most = if most.is_finite() { px(most) } else { f32::INFINITY };
            let reserve = spec.text_widths[c.index()];
            let reserve = if reserve > 0.0 { reserve + px(12.0) } else { 0.0 };
            let set = spec.widths[c.index()];
            ColumnSpec::new(c, priority, px(least))
                .flexible(most)
                .reserving(reserve)
                .chosen(spec.choices[c.index()])
                .min(px(c.min_width()))
                .sized((set > 0.0).then(|| px(set)))
        })
        .collect();
    let inset = px(8.0);
    let width = (table.size.width - 2.0 * inset - chooser).max(0.0);
    columns::fit(&specs, table.min_x() + inset, width, px(12.0), spec.sorted_by)
}

/// The toolbar's items in `bar` (the filter a magnifier when `filter_icon`; "Sort by…" for
/// a list), and whether they fit.
fn toolbar_items(
    spec: &LayoutSpec,
    bar: Rect,
    filter_icon: bool,
    list: bool,
) -> (OverflowLayout<Tool>, bool) {
    let px = |v: f32| (v * spec.scale).round();
    let icon = px(28.0);
    let filter = if filter_icon {
        OverflowItem::new(Tool::Filter, icon, Side::Left, None, 9)
    } else {
        OverflowItem::new(Tool::Filter, px(280.0), Side::Left, None, 9).flexible(px(140.0))
    };
    let mut items = vec![
        OverflowItem::new(Tool::Scope, spec.tools.scope, Side::Left, Some(3), 0),
        OverflowItem::new(Tool::Grouping, spec.tools.grouping, Side::Left, Some(1), 1),
        filter,
    ];
    if list {
        items.push(OverflowItem::new(Tool::Sort, spec.tools.sort, Side::Right, None, 8));
    }
    items.push(OverflowItem::new(Tool::Pause, icon, Side::Right, Some(2), 2));
    items.push(OverflowItem::new(Tool::Interval, spec.tools.interval, Side::Right, Some(0), 3));
    let layout = overflow::fit(&OverflowSpec {
        min_x: bar.min_x(),
        max_x: bar.max_x(),
        pad: 0.0,
        gap: px(8.0),
        more_width: icon,
        items: &items,
    });
    let fits = layout.fits;
    (layout, fits)
}

/// The last layout, kept while its spec is the same (shared: a paint holds it while it
/// draws, nothing copied); the class it found is the next one's previous (the hysteresis).
#[derive(Debug, Default)]
pub struct LayoutCache {
    memo: kernel::Memo<LayoutSpec, std::sync::Arc<PageLayout>>,
    /// How many times it worked one out (the tests').
    pub computed: u64,
}

impl LayoutCache {
    /// The layout of `spec` (worked out only when it changed).
    pub fn get(&mut self, spec: &LayoutSpec) -> std::sync::Arc<PageLayout> {
        let previous = self.memo.value().map(|l| l.class);
        let computed = &mut self.computed;
        // The spec holds no heap data: copying it allocates nothing.
        let page = self.memo.get_or_update(spec.clone(), || {
            *computed += 1;
            std::sync::Arc::new(layout(spec, previous))
        });
        std::sync::Arc::clone(page)
    }

    /// The last one worked out.
    pub fn last(&self) -> Option<&PageLayout> {
        self.memo.value().map(|l| &**l)
    }

    /// The scale the last one was worked out at (1 before any).
    pub fn scale(&self) -> f32 {
        self.memo.key().map_or(1.0, |spec| spec.scale)
    }
}

impl kernel::Purge for LayoutCache {
    fn purge(&mut self) {
        kernel::Purge::purge(&mut self.memo);
    }
}

#[cfg(test)]
#[path = "../tests/unit/layout.rs"]
mod tests;
