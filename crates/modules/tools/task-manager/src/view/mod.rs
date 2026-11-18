//! The Task Manager's tab: a page that recomposes itself for the room it has, as a
//! responsive web page — a toolbar (scope, grouping, filter, pause, interval; what doesn't
//! fit in its "…"), the whole app's metric cards (or a band of pills, or a line), the parts'
//! table grouped and virtualized (its columns by priority with their chooser, a part's
//! programs under it, or a list of two-line / one-line rows on a narrow page) and a part's
//! detail (on the right, below, or a page with "‹ Back"), resized by its separator. Where
//! each goes is [`crate::layout`]'s (pure, kept while nothing changes); what's listed is
//! the view model's ([`crate::model`], built once per look or change); the texts are
//! written once per look ([`texts`]); the graphs aggregated once per new value or width
//! (`chart::PlotCache`). A frame that draws the same allocates nothing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Instant;

use kernel::{Point, Rect};
use render_api::Scene;
use text_layout::TextSystem;
use ui_components::chart::PlotCache;
use ui_components::columns::Choice;
use ui_components::ds::{self, Ctx};
use ui_components::{EdgeScrollbar, Orientation, Splitter, SplitterStyle};
use ui_kit::TextInput;
use workbench_model::ResourceSource;

use crate::collector::{Frame, History, Hub, Line, Metric, Subscription, Tick};
use crate::layout::{COLUMNS, ColumnId, DetailPlace, LayoutCache, LayoutSpec, PageLayout};
use crate::model::{Column, GroupKey, Grouping, Item, ViewModel, ViewState};

mod actions;
mod commands;
mod detail;
mod keys;
mod menu;
mod summary;
mod table;
mod texts;
mod toolbar;
mod viewer;

pub(crate) use keys::{Focus, RowRef};
pub(crate) use summary::{color, reduce};
pub(crate) use texts::{Texts, floor};

/// The view of every open project, chosen last (kept while the app runs).
static ALL_PROJECTS: AtomicBool = AtomicBool::new(false);

/// The keys of the detail's shares (beside the list, below it) in the project's sizes
/// ([`workbench_model::LayoutSizes`]).
pub const DETAIL_RIGHT: &str = "task-manager.detail-right";
pub const DETAIL_BELOW: &str = "task-manager.detail-below";

/// What the user chose that's kept in the settings (the columns, the detail's share): read
/// at startup and after every change ([`set_preferences`]), written by the views through
/// the module's command ([`crate::SAVE`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preferences {
    pub choices: [Choice; COLUMNS.len()],
    /// The widths the columns were dragged to (logical pixels, 0: as their specs say).
    pub widths: [f32; COLUMNS.len()],
    /// The detail's share of the page on the right (0.15–0.6).
    pub ratio: f32,
    /// How often it looks and how far its graphs reach back (seconds).
    pub interval: i64,
    pub history: i64,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            choices: [Choice::Auto; COLUMNS.len()],
            widths: [0.0; COLUMNS.len()],
            ratio: 0.30,
            interval: 1,
            history: 60,
        }
    }
}

static PREFERENCES: Mutex<Option<Preferences>> = Mutex::new(None);

/// The tests changing the preferences (or the hubs' timing) take turns.
#[cfg(test)]
pub(crate) static PREFERENCES_TESTS: Mutex<()> = Mutex::new(());

/// The preferences as the settings have them (the views follow).
pub fn set_preferences(preferences: Preferences) {
    *PREFERENCES.lock().unwrap_or_else(PoisonError::into_inner) = Some(preferences);
}

/// The preferences now.
pub fn preferences() -> Preferences {
    PREFERENCES.lock().unwrap_or_else(PoisonError::into_inner).unwrap_or_default()
}

/// A metric card of the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Card {
    Cpu,
    Memory,
    Gpu,
    /// Network and disk.
    Io,
}

impl Card {
    /// The column it sorts the table by.
    fn column(self) -> Column {
        match self {
            Card::Cpu => Column::Cpu,
            Card::Memory => Column::Memory,
            Card::Gpu => Column::Gpu,
            Card::Io => Column::Net,
        }
    }
}

/// A popover the view opens over itself (a press outside or Esc closes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popover {
    /// The toolbar's "…": what didn't fit.
    More,
    /// How often it looks, how far back its graphs go.
    Interval,
    /// "Sort by…" (the lists).
    Sort,
    /// The columns' chooser.
    Columns,
}

/// A row of a popover: what a click on it does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MenuAction {
    /// A heading (not clickable): its label's index in [`texts::Labels::headings`].
    Heading(usize),
    Scope(bool),
    Grouping(Grouping),
    Pause,
    Interval(i64),
    History(i64),
    Sort(Column),
    Column(ColumnId),
    ResetColumns,
}

/// What's under the mouse (or was clicked): small, copied, nothing allocated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Hit {
    Scope(bool),
    Grouping(Grouping),
    Filter,
    /// The magnifier of a narrow page's filter (it opens over the bar).
    FilterIcon,
    FilterClear,
    Pause,
    Open(Popover),
    /// A card (a click sorts by its metric), its value (min / avg / max), its chart (a
    /// reading).
    Card(Card),
    CardValue(Card),
    CardChart(Card),
    Header(ColumnId),
    /// The edge on a column's right (dragged to resize it, double-clicked to fit it).
    Edge(ColumnId),
    Group(GroupKey),
    /// A row of the list (by its item's place).
    Row(usize),
    /// A part's chevron: its programs listed or not.
    Expand(usize),
    ClearFilter,
    Resume,
    CloseDetail,
    Back,
    /// A chart of the detail (by its metric).
    DetailChart(Metric),
    Menu(MenuAction),
    /// The popover's own area (a press there isn't outside).
    PopoverArea,
    /// One of the selected part's actions (its button in the detail).
    Action(workbench_model::PartAction),
}

/// The graphs' aggregations: the cards' (the whole app's), the detail's (the selected
/// part's), the trend column's (a row each, for the rows shown).
#[derive(Debug, Default)]
pub(crate) struct Plots {
    pub cards: [PlotCache; 5],
    pub detail: [PlotCache; 5],
    pub trend: Vec<PlotCache>,
}

impl kernel::Purge for Plots {
    fn purge(&mut self) {
        *self = Self::default();
    }
}

pub struct TaskManager {
    /// Its hold on the window's collector (the hub: one thread however many views).
    subscription: Option<Subscription>,
    /// Its window's resources, held weakly (the hub holds them while a view shows): given
    /// to its mirror when its tab is split.
    source: Option<Weak<dyn ResourceSource>>,
    /// Its tab is visible: the hub looks for it.
    shown: bool,
    /// The app's waker was given to the hub.
    waker_given: bool,
    /// The last look shown, and its number.
    frame: Arc<Frame>,
    generation: u64,
    /// Paused: what's shown stays; the looks that come wait here (the last one).
    paused: bool,
    pending: Option<Tick>,
    /// The values over time (the hub's for its scope, or made up).
    history: Option<Arc<Mutex<History>>>,
    /// Its clock stopped at this (a made-up Task Manager's: the same picture every run).
    clock: Option<Instant>,
    /// What it chose (sorting, filter, folded groups, grouping, opened parts), and its
    /// number (bumped by each change: the model is built again).
    state: ViewState,
    state_generation: u64,
    /// What it lists, for the last look and state.
    model: Arc<ViewModel>,
    /// Its texts, written once per look.
    texts: Texts,
    filter: TextInput,
    /// A narrow page's filter, opened over the bar.
    filter_open: bool,
    /// The selected part (by its key), its last row (kept when it's gone), its detail open.
    selected: Option<String>,
    last_selected: Option<Arc<Line>>,
    detail_open: bool,
    /// Bumped by each selection (the detail's graphs are another part's).
    selection_epoch: u64,
    /// Every open project shown (else this window's).
    all: bool,
    /// The columns chosen and the detail's shares: the project's as dragged in it
    /// ([`DETAIL_RIGHT`], [`DETAIL_BELOW`]), else the settings' on the right.
    choices: [Choice; COLUMNS.len()],
    /// The widths the columns were dragged to (logical pixels, 0: as their specs say).
    widths: [f32; COLUMNS.len()],
    /// A column's edge held: the column, where it was pressed and how wide it was then
    /// (physical pixels).
    resizing: Option<(ColumnId, f32, f32)>,
    /// A column's edge double-clicked: fitted to its content at the next paint (its texts
    /// are measured there).
    fit_pending: Option<ColumnId>,
    /// How many clicks in a row the last press was.
    clicks: u32,
    ratio_right: f32,
    ratio_below: f32,
    /// The project's dragged sizes (the detail's shares are kept there).
    sizes: Option<workbench_model::LayoutSizes>,
    /// The settings' preferences seen last (another view or the settings window changed
    /// them: followed).
    preferences: Preferences,
    popover: Option<(Popover, Rect)>,
    /// The open popover's rows (reused).
    menu: Vec<MenuAction>,
    scroll: f32,
    max_scroll: f32,
    bar: EdgeScrollbar,
    detail_scroll: f32,
    detail_max: f32,
    detail_bar: EdgeScrollbar,
    /// The list's scroll when its detail opened on a page ("‹ Back" returns to it).
    back_to: Option<f32>,
    splitter: Splitter,
    layouts: LayoutCache,
    plots: Plots,
    hits: Vec<(Rect, Hit)>,
    /// Texts drawn cut (their whole in a tooltip): where, and which row.
    cut: Vec<(Rect, usize)>,
    hover: Option<Hit>,
    mouse: Option<Point>,
    /// It has the keyboard (its filter's caret solid, its focus ring drawn).
    focused: bool,
    /// What has its keys (a control, a card, the table, the detail), and whether the ring
    /// shows it (after a key, not after a click).
    focus: Focus,
    ring: bool,
    /// The row the table's keys are on (else the selected part's), its number (bumped by
    /// each move) and its place found for a model.
    cursor: Option<RowRef>,
    cursor_epoch: u64,
    cursor_cache: Option<(CursorStamp, Option<usize>)>,
    /// The cursor's row as drawn last (its ring).
    cursor_rect: Option<Rect>,
    /// What Tab goes through, in the order drawn (reused).
    stops: Vec<(Rect, Focus)>,
    /// The open popover's row the keys are on.
    menu_at: Option<usize>,
    /// An action asked first (in the module's modal): the question's identity, the part's
    /// key, the action.
    asking: Option<(crate::confirm::Question, String, workbench_model::PartAction)>,
    /// What it tells the user (how an action went), taken by the app as toasts.
    notifications: Vec<workbench_model::Notification>,
    /// What it asks the app to do (save the preferences).
    action: Option<ui_components::ViewerAction>,
    /// The theme as it draws it (its muted text readable on a card), made once a theme.
    themed: Option<(ThemeKey, Arc<ui_components::Palette>)>,
}

/// What the cursor's place was found for: the model's stamp, the cursor's and the
/// selection's numbers.
type CursorStamp = ((u64, u64), u64, u64);

/// The colors a theme's copy is made from.
type ThemeKey = [u32; 32];

fn theme_key(p: &ui_components::Palette) -> ThemeKey {
    let mut key = [0; 32];
    let colors = [
        p.editor_background,
        p.text,
        p.text_muted,
        p.accent,
        p.border,
        p.card_border,
        p.warning,
        p.danger,
    ];
    for (i, c) in colors.iter().enumerate() {
        key[i * 4..i * 4 + 4].copy_from_slice(&[
            c.r.to_bits(),
            c.g.to_bits(),
            c.b.to_bits(),
            c.a.to_bits(),
        ]);
    }
    key
}

impl Default for TaskManager {
    fn default() -> Self {
        let all = ALL_PROJECTS.load(Ordering::Relaxed);
        let state = ViewState { grouping: grouping(all), ..ViewState::default() };
        let preferences = preferences();
        Self {
            subscription: None,
            source: None,
            shown: false,
            waker_given: false,
            frame: Arc::new(Frame::default()),
            generation: 0,
            paused: false,
            pending: None,
            history: None,
            clock: None,
            state,
            state_generation: 1,
            model: Arc::new(ViewModel::default()),
            texts: Texts::default(),
            filter: TextInput::default(),
            filter_open: false,
            selected: None,
            last_selected: None,
            detail_open: false,
            selection_epoch: 0,
            all,
            choices: preferences.choices,
            widths: preferences.widths,
            resizing: None,
            fit_pending: None,
            clicks: 1,
            ratio_right: preferences.ratio,
            ratio_below: 0.45,
            sizes: None,
            preferences,
            popover: None,
            menu: Vec::new(),
            scroll: 0.0,
            max_scroll: 0.0,
            bar: EdgeScrollbar::default(),
            detail_scroll: 0.0,
            detail_max: 0.0,
            detail_bar: EdgeScrollbar::default(),
            back_to: None,
            splitter: Splitter::new(SplitterStyle::default(), Orientation::Vertical),
            layouts: LayoutCache::default(),
            plots: Plots::default(),
            hits: Vec::new(),
            cut: Vec::new(),
            hover: None,
            mouse: None,
            focused: false,
            focus: Focus::Table,
            ring: false,
            cursor: None,
            cursor_epoch: 0,
            cursor_cache: None,
            cursor_rect: None,
            stops: Vec::new(),
            menu_at: None,
            asking: None,
            notifications: Vec::new(),
            action: None,
            themed: None,
        }
    }
}

impl std::fmt::Debug for TaskManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskManager")
            .field("lines", &self.frame.lines.len())
            .finish_non_exhaustive()
    }
}

/// How a view groups first: by project in the view of every project, else by kind.
fn grouping(all: bool) -> Grouping {
    if all { Grouping::Project } else { Grouping::Kind }
}

/// The sorting column of a table's column (the trend, the project and the details sort
/// nothing).
pub(crate) fn sort_column(column: ColumnId) -> Option<Column> {
    Some(match column {
        ColumnId::Name => Column::Name,
        ColumnId::Cpu => Column::Cpu,
        ColumnId::Memory => Column::Memory,
        ColumnId::Network => Column::Net,
        ColumnId::Disk => Column::Disk,
        ColumnId::Gpu => Column::Gpu,
        ColumnId::Threads => Column::Threads,
        ColumnId::Project | ColumnId::Trend | ColumnId::Detail => return None,
    })
}

/// The table's column of a sorting column.
pub(crate) fn column_of(column: Column) -> ColumnId {
    match column {
        Column::Name => ColumnId::Name,
        Column::Cpu => ColumnId::Cpu,
        Column::Memory => ColumnId::Memory,
        Column::Net => ColumnId::Network,
        Column::Disk => ColumnId::Disk,
        Column::Gpu => ColumnId::Gpu,
        Column::Threads => ColumnId::Threads,
    }
}

impl TaskManager {
    /// Where the numbers come from (the app's resources).
    pub fn set_source(&mut self, source: Arc<dyn ResourceSource>) {
        let labels = crate::collector::FrameLabels::translated();
        let subscription = Hub::subscribe(&source, labels, self.all);
        self.source = Some(Arc::downgrade(&source));
        subscription.set_shown(self.shown);
        self.history = Some(subscription.history());
        self.subscription = Some(subscription);
        self.waker_given = false;
    }

    /// Its tab shown or hidden: the hub looks while one of its views is shown.
    pub fn show(&mut self, shown: bool) {
        self.shown = shown;
        if let Some(subscription) = &self.subscription {
            subscription.set_shown(shown);
        }
    }

    /// What it chose changed: listed again (once, when next asked).
    fn changed(&mut self) {
        self.state_generation += 1;
    }

    /// What it lists now (built again only when the look or its state changed).
    pub fn model(&mut self) -> Arc<ViewModel> {
        let stamp = (self.generation, self.state_generation);
        if self.model.stamp != stamp {
            let frame = Arc::clone(&self.frame);
            let now = self.clock.unwrap_or_else(Instant::now);
            self.model = Arc::new(ViewModel::build(frame, &self.state, stamp, now));
        }
        Arc::clone(&self.model)
    }

    /// The parts shown: those matching the filter, sorted.
    pub fn shown_names(&mut self) -> Vec<String> {
        self.model().shown_lines().map(|l| l.name.clone()).collect()
    }

    /// Every open project shown (else this window's).
    pub fn set_all(&mut self, all: bool) {
        self.all = all;
        ALL_PROJECTS.store(all, Ordering::Relaxed);
        self.state.grouping = grouping(all);
        self.changed();
        if let Some(subscription) = &self.subscription {
            subscription.set_all(all);
            self.history = Some(subscription.history());
        }
        self.scroll = 0.0;
    }

    pub fn shows_all(&self) -> bool {
        self.all
    }

    /// Grouped by `grouping` (by project only in the view of every project).
    pub fn set_grouping(&mut self, grouping: Grouping) {
        if self.state.grouping != grouping {
            self.state.grouping = grouping;
            self.scroll = 0.0;
            self.changed();
        }
    }

    pub fn grouping(&self) -> Grouping {
        self.state.grouping
    }

    /// The parts' keys in the order shown (folded groups' left out).
    pub fn visible_keys(&mut self) -> Vec<String> {
        self.model().keys.clone()
    }

    /// Sorted by `column`: again on the same one, the other way.
    pub fn sort_by(&mut self, column: Column) {
        self.state.sort = if self.state.sort.0 == column {
            (column, !self.state.sort.1)
        } else {
            // Numbers: the largest first; names: A to Z.
            (column, column != Column::Name)
        };
        self.changed();
    }

    pub fn sorted_by(&self) -> (Column, bool) {
        self.state.sort
    }

    /// The part `key` selected and its detail opened (again: unselected, closed).
    pub fn select(&mut self, key: &str) {
        if self.selected.as_deref() == Some(key) {
            self.close_detail();
            self.selected = None;
            self.selection_epoch += 1;
            return;
        }
        self.selected = Some(key.to_owned());
        self.last_selected = self.frame.line(key).cloned().map(Arc::new);
        self.selection_epoch += 1;
        self.set_cursor(Some(RowRef::Part(key.to_owned())));
        self.open_detail();
    }

    /// The selected part's detail shown (on a page: the list's scroll kept for "‹ Back").
    pub fn open_detail(&mut self) {
        if self.selected.is_none() {
            return;
        }
        if !self.detail_open && self.place() == DetailPlace::Page {
            self.back_to = Some(self.scroll);
        }
        self.detail_open = true;
        self.detail_scroll = 0.0;
    }

    /// The detail closed (a page: back to the list, its row and scroll as they were).
    pub fn close_detail(&mut self) {
        self.detail_open = false;
        if let Some(scroll) = self.back_to.take() {
            self.scroll = scroll;
        }
    }

    pub fn detail_open(&self) -> bool {
        self.detail_open && self.selected.is_some()
    }

    /// Where its detail opens on the page as it was last laid out.
    pub fn place(&self) -> DetailPlace {
        self.layouts.last().map_or(DetailPlace::Right, |l| l.place)
    }

    /// The page as it was last laid out.
    pub fn page(&self) -> Option<&PageLayout> {
        self.layouts.last()
    }

    /// Group `key` folded / opened.
    pub fn toggle_group(&mut self, key: GroupKey) {
        if !self.state.folded.remove(&key) {
            self.state.folded.insert(key);
        }
        self.changed();
    }

    /// Part `key`'s programs listed under it, or not.
    pub fn toggle_expanded(&mut self, key: &str) {
        if !self.state.expanded.remove(key) {
            self.state.expanded.insert(key.to_owned());
        }
        self.changed();
    }

    /// The filter's text changed.
    fn filtered(&mut self) {
        if self.state.filter != self.filter.text() {
            self.state.filter = self.filter.text().to_owned();
            self.scroll = 0.0;
            self.changed();
        }
    }

    /// The filter emptied.
    pub fn clear_filter(&mut self) {
        self.filter.set_text("");
        self.filtered();
    }

    /// Paused (what's shown stays, the looks wait) or going on (the last look shown).
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if !paused && let Some(tick) = self.pending.take() {
            self.apply(tick);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// A new look from the hub (kept aside while paused).
    pub fn take(&mut self, tick: Tick) {
        if self.paused {
            self.pending = Some(tick);
        } else {
            self.apply(tick);
        }
    }

    fn apply(&mut self, tick: Tick) {
        // The selected part's last row kept: gone, its detail says so with these.
        if let Some(selected) = &self.selected
            && let Some(line) = tick.now.line(selected)
        {
            self.last_selected = Some(Arc::new(line.clone()));
        }
        self.frame = tick.now;
        self.generation = tick.generation;
    }

    /// A frame taken as the next look (the tests').
    pub fn take_frame(&mut self, frame: Frame) {
        let generation = self.generation.max(self.pending.as_ref().map_or(0, |t| t.generation)) + 1;
        self.take(Tick { generation, now: Arc::new(frame) });
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The selected part as it stands (its last row when gone).
    pub fn selection(&self) -> Option<crate::model::Selected> {
        let key = self.selected.as_deref()?;
        crate::model::selection(&self.frame, key, self.last_selected.as_deref())
    }

    /// The columns' choices and the detail's share, as the user left them.
    pub fn choices(&self) -> [Choice; COLUMNS.len()] {
        self.choices
    }

    /// Column `column` chosen in the chooser: shown → hidden; hidden or left for want of
    /// room → kept (it pushes others out); kept → hidden. Saved.
    pub fn choose_column(&mut self, column: ColumnId) {
        let i = column.index();
        let dropped = self.layouts.last().is_some_and(|l| l.columns.dropped.contains(&column));
        self.choices[i] = match self.choices[i] {
            Choice::Auto if dropped => Choice::Pinned,
            Choice::Auto | Choice::Pinned => Choice::Hidden,
            Choice::Hidden => Choice::Pinned,
        };
        self.save();
    }

    /// Every column as its priority says again, at its spec's width.
    pub fn reset_columns(&mut self) {
        self.choices = [Choice::Auto; COLUMNS.len()];
        self.widths = [0.0; COLUMNS.len()];
        self.save();
    }

    /// The widths the columns were dragged to (logical pixels, 0: as their specs say).
    pub fn column_widths(&self) -> [f32; COLUMNS.len()] {
        self.widths
    }

    /// Column `column`'s edge pressed at `x` (physical pixels): a drag resizes it.
    pub(super) fn begin_resize(&mut self, column: ColumnId, x: f32) {
        let width = self.layouts.last().and_then(|l| l.columns.get(column)).map(|(_, w)| w);
        self.resizing = width.map(|w| (column, x, w));
    }

    /// The held edge dragged to `x` (physical pixels): the column's width follows, between
    /// its least and what the others leave it. `true` if it changed.
    pub(super) fn resize_to(&mut self, x: f32) -> bool {
        let Some((column, from, width)) = self.resizing else { return false };
        let Some(page) = self.layouts.last() else { return false };
        let Some(w) = page.columns.dragged(column, width, x - from) else { return false };
        let scale = self.layouts.scale().max(0.1);
        let logical = (w / scale).round();
        let before = self.widths[column.index()];
        self.widths[column.index()] = logical;
        before != logical
    }

    /// The held edge let go: the widths saved. `true` if one was held.
    pub(super) fn end_resize(&mut self) -> bool {
        if self.resizing.take().is_none() {
            return false;
        }
        self.save();
        true
    }

    /// Column `column` fitted to its content (`content`: its widest content, physical
    /// pixels at `scale`; `None`: back to its spec's width). Saved.
    pub(super) fn fit_column(&mut self, column: ColumnId, content: Option<f32>, scale: f32) {
        let scale = scale.max(0.1);
        let width = content.map_or(0.0, |w| (w / scale).ceil().max(column.min_width()));
        self.widths[column.index()] = width.min(*crate::settings::WIDTH_RANGE.end());
        self.save();
    }

    /// The preferences handed to the app to save (through the module's command).
    fn save(&mut self) {
        let mut preferences = preferences();
        preferences.choices = self.choices;
        preferences.widths = self.widths;
        preferences.ratio = self.ratio_right;
        set_preferences(preferences);
        self.preferences = preferences;
        self.action = Some(ui_components::ViewerAction::RunCommand(crate::SAVE.0.to_owned()));
    }

    /// How often it looks / how far back: the hubs follow at once, saved.
    fn set_timing(&mut self, interval: Option<i64>, history: Option<i64>) {
        let mut preferences = preferences();
        if let Some(interval) = interval {
            preferences.interval = interval;
        }
        if let Some(history) = history {
            preferences.history = history;
        }
        let secs = |s: i64| std::time::Duration::from_secs(s.max(1) as u64);
        crate::collector::configure(secs(preferences.interval), secs(preferences.history));
        set_preferences(preferences);
        self.preferences = preferences;
        self.action = Some(ui_components::ViewerAction::RunCommand(crate::SAVE.0.to_owned()));
    }

    /// The settings' preferences followed when they changed elsewhere.
    fn follow_preferences(&mut self) {
        let now = preferences();
        if now != self.preferences {
            self.choices = now.choices;
            // A drag under way keeps its own widths.
            if self.resizing.is_none() {
                self.widths = now.widths;
            }
            self.ratio_right = now.ratio;
            self.preferences = now;
        }
    }

    /// What a click on `hit` does.
    fn click_hit(&mut self, hit: Hit) {
        if !matches!(hit, Hit::Menu(_) | Hit::PopoverArea) {
            self.popover = None;
        }
        match hit {
            Hit::Scope(all) => self.set_all(all),
            Hit::Grouping(grouping) => self.set_grouping(grouping),
            Hit::Filter => {}
            Hit::FilterIcon => self.filter_open = true,
            Hit::FilterClear => {
                self.clear_filter();
                self.filter_open = false;
            }
            Hit::Pause => self.set_paused(!self.paused),
            Hit::Open(popover) => {
                self.open_popover(popover);
                // Opened by the mouse: no row is the keys' yet.
                self.menu_at = None;
            }
            Hit::Card(card) | Hit::CardValue(card) | Hit::CardChart(card) => {
                let column = card.column();
                if self.state.sort.0 != column {
                    self.sort_by(column);
                }
            }
            Hit::Header(column) => {
                if let Some(column) = sort_column(column) {
                    self.sort_by(column);
                }
            }
            Hit::Group(key) => self.toggle_group(key),
            Hit::Row(i) => self.click_row(i),
            Hit::Expand(i) => {
                let model = self.model();
                if let Some(Item::Part { index, .. }) = model.items.get(i) {
                    let key = model.frame.lines[*index].key.clone();
                    self.toggle_expanded(&key);
                }
            }
            Hit::ClearFilter => self.clear_filter(),
            Hit::Resume => self.set_paused(false),
            Hit::CloseDetail | Hit::Back => self.close_detail(),
            Hit::DetailChart(_) | Hit::PopoverArea | Hit::Edge(_) => {}
            Hit::Menu(action) => self.run_menu(action),
            Hit::Action(action) => {
                if let Some(key) = self.selected.clone() {
                    self.request(&key, action);
                }
            }
        }
    }

    /// A row clicked: a part selected (its detail opened), a program's part, an
    /// operation's part.
    fn click_row(&mut self, i: usize) {
        let model = self.model();
        let row = keys::row_ref(&model, i);
        let key = match model.items.get(i) {
            Some(Item::Part { index, .. }) => model.frame.lines[*index].key.clone(),
            Some(Item::Process { part, .. }) => model.frame.lines[*part].key.clone(),
            Some(Item::Operation(operation)) => operation.owner.clone(),
            Some(Item::Group { key, .. }) => return self.toggle_group(*key),
            None => return,
        };
        self.select(&key);
        if self.selected.is_some() {
            self.set_cursor(row);
        }
    }

    /// A popover's row chosen.
    fn run_menu(&mut self, action: MenuAction) {
        match action {
            MenuAction::Heading(_) => return,
            MenuAction::Scope(all) => self.set_all(all),
            MenuAction::Grouping(grouping) => self.set_grouping(grouping),
            MenuAction::Pause => self.set_paused(!self.paused),
            MenuAction::Interval(secs) => self.set_timing(Some(secs), None),
            MenuAction::History(secs) => self.set_timing(None, Some(secs)),
            MenuAction::Sort(column) => self.sort_by(column),
            // The chooser stays open: several columns are chosen in a row.
            MenuAction::Column(column) => return self.choose_column(column),
            MenuAction::ResetColumns => return self.reset_columns(),
        }
        self.popover = None;
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The layout's spec for `bounds` at `scale` now.
    fn spec(&self, bounds: Rect, scale: f32) -> LayoutSpec {
        let sorted = column_of(self.state.sort.0);
        LayoutSpec {
            bounds,
            scale,
            has_gpu: self.frame.gpu.is_some() || self.frame.lines.is_empty(),
            detail_open: self.detail_open(),
            scope_all: self.all,
            grouping: self.state.grouping.on(&self.frame),
            choices: self.choices,
            widths: self.widths,
            sorted_by: Some(sorted),
            text_widths: self.texts.widths,
            tools: self.texts.tools,
            ratio_right: self.ratio_right,
            ratio_below: self.ratio_below,
        }
    }

    /// The project's sizes: the detail's shares as last dragged in the project.
    pub(super) fn take_sizes(&mut self, sizes: &workbench_model::LayoutSizes) {
        if let Some(ratio) = sizes.get(DETAIL_RIGHT) {
            self.ratio_right = ratio.clamp(0.15, 0.6);
        }
        if let Some(ratio) = sizes.get(DETAIL_BELOW) {
            self.ratio_below = ratio.clamp(0.2, 0.8);
        }
        self.sizes = Some(sizes.clone());
    }

    /// The separator let go: the share kept in the project (and, on the right, in the
    /// settings: the share of a project where it was never dragged).
    pub(super) fn keep_detail_share(&mut self) {
        let (key, ratio) = match self.splitter.orientation() {
            Orientation::Vertical => (DETAIL_RIGHT, self.ratio_right),
            Orientation::Horizontal => (DETAIL_BELOW, self.ratio_below),
        };
        if let Some(sizes) = &self.sizes {
            sizes.set(key, (ratio * 1000.0).round() / 1000.0);
        }
        if self.splitter.orientation() == Orientation::Vertical {
            self.save();
        }
    }

    /// The detail's separator dragged to `center` (physical pixels on its axis): the
    /// share follows (the ratio, not the pixels, is kept).
    fn drag_splitter(&mut self, center: f32) {
        let Some(page) = self.layouts.last() else { return };
        let (Some(table), Some(detail)) = (page.table, page.detail) else { return };
        match self.splitter.orientation() {
            Orientation::Vertical => {
                let total = detail.max_x() - table.min_x();
                if total > 0.0 {
                    self.ratio_right = ((detail.max_x() - center) / total).clamp(0.15, 0.6);
                }
            }
            Orientation::Horizontal => {
                let total = detail.max_y() - table.min_y();
                if total > 0.0 {
                    self.ratio_below = ((detail.max_y() - center) / total).clamp(0.2, 0.8);
                }
            }
        }
    }

    /// Everything drawn in `scene` (the view's paint without the app's context: the
    /// tests' and the snapshots' way in).
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        palette: &ui_components::Palette,
        bounds: Rect,
        scale: f32,
    ) {
        self.follow_preferences();
        let palette = self.themed(palette);
        let palette = &*palette;
        let ds = Ctx::new(palette, scale);
        let model = self.model();
        self.refresh_texts(text, &ds, &model);
        if let Some(column) = self.fit_pending.take() {
            let content = self.content_width(text, &ds, &model, column);
            self.fit_column(column, content, scale);
        }
        let spec = self.spec(bounds, scale);
        let page = self.layouts.get(&spec);
        self.hits.clear();
        self.cut.clear();
        self.stops.clear();
        self.cursor_rect = None;
        let layer = scene.push_layer(Some(bounds));
        layer.quad(render_api::Quad::filled(bounds, palette.editor_background));
        self.paint_toolbar(scene, text, icons, &ds, &page);
        self.paint_summary(scene, text, icons, &ds, &page);
        if page.table.is_some() {
            self.paint_table(scene, text, icons, &ds, &page, &model);
        }
        if let Some(detail) = page.detail.filter(|_| self.detail_open()) {
            self.paint_detail(scene, text, icons, &ds, &page, detail);
        }
        // The separator over both (its handle reaches over each).
        match page.splitter.filter(|_| self.detail_open()) {
            Some((strip, orientation)) => {
                if self.splitter.orientation() != orientation {
                    self.splitter =
                        Splitter::new(SplitterStyle::from_palette(palette), orientation);
                } else {
                    self.splitter.set_style(SplitterStyle::from_palette(palette));
                }
                self.splitter.set_bounds(strip, scale);
                self.splitter.paint(scene);
            }
            None => self.splitter.set_bounds(Rect::default(), scale),
        }
        // The keys' place, after a key (not after a click), while it has them.
        if self.shows_ring()
            && let Some(rect) = self.ring_rect()
        {
            let layer = scene.push_layer(Some(bounds));
            ds::focus_ring(layer, &ds, rect, ds.px(ui_components::ds::radius::SM));
        }
        self.paint_popover(scene, text, icons, &ds, &page);
    }

    /// `palette` as the view draws it: its muted text moved toward the text just enough to
    /// read at 4.5:1 on a card (made once a theme; nothing copied otherwise).
    fn themed(&mut self, palette: &ui_components::Palette) -> Arc<ui_components::Palette> {
        let key = theme_key(palette);
        if let Some((cached, themed)) = &self.themed
            && *cached == key
        {
            return Arc::clone(themed);
        }
        let mut themed = palette.clone();
        let card = palette.editor_background.mix(palette.text, 0.03);
        themed.text_muted =
            ui_components::severity::readable(palette.text_muted, palette.text, card, 4.5);
        let themed = Arc::new(themed);
        self.themed = Some((key, Arc::clone(&themed)));
        themed
    }

    /// The focus ring is drawn: it has the keys and a key (not a click) moved them.
    fn shows_ring(&self) -> bool {
        self.focused && self.ring
    }

    /// The splitter (the tests check its handle is drawn on top).
    pub fn splitter(&self) -> &Splitter {
        &self.splitter
    }

    /// The clickable areas drawn last (the tests').
    pub fn hit_areas(&self) -> impl Iterator<Item = (Rect, String)> + '_ {
        self.hits.iter().map(|(r, h)| (*r, format!("{h:?}")))
    }

    /// The open popover.
    pub fn popover(&self) -> Option<Popover> {
        self.popover.map(|(p, _)| p)
    }

    /// The history's values for `metric` of the whole app (or `part`), in two slices, with
    /// its generation and capacity; `None` without a history.
    fn with_history<R>(&self, f: impl FnOnce(&History) -> R) -> Option<R> {
        let history = self.history.as_ref()?;
        let history = history.lock().unwrap_or_else(PoisonError::into_inner);
        Some(f(&history))
    }

    /// The made-up Task Manager `scenario` shown (the snapshots' and the tests'): its parts,
    /// history and state, its clock stopped, nothing measured.
    #[cfg(any(test, feature = "snapshot"))]
    pub fn use_demo(&mut self, scenario: &str) {
        let now = Instant::now();
        let all = scenario == "all";
        self.subscription = None;
        self.clock = Some(now);
        self.all = all;
        self.state.grouping = grouping(all);
        let frame = if scenario == "servers" {
            crate::demo::frame_with_servers(now)
        } else {
            crate::demo::frame(all, now)
        };
        self.history = Some(crate::demo::history(&frame, 60, now));
        self.take_frame(frame);
        self.changed();
        let key =
            |name: &str| self.frame.lines.iter().find(|l| l.name == name).map(|l| l.key.clone());
        match scenario {
            "wide" | "split" | "page" | "all" => {
                if let Some(key) = key(&i18n::t("tasks.terminal")) {
                    self.state.expanded.insert(key.clone());
                    self.select(&key);
                }
                // In a split: the row selected, the table kept (the page has its own case).
                if scenario == "split" || scenario == "all" {
                    self.close_detail();
                }
            }
            // The project's language servers: vtsls selected, its programs and its figures
            // shown; the app's parts folded, so they're in view.
            "servers" => {
                self.state.folded.insert(GroupKey::Operations);
                use workbench_model::resources::{APP, INDEXING, PROGRAMS, SERVICES, TERMINALS};
                for group in [APP, INDEXING, TERMINALS, PROGRAMS, SERVICES] {
                    self.state.folded.insert(GroupKey::Kind(group));
                }
                if let Some(key) = key("vtsls") {
                    self.state.expanded.insert(key.clone());
                    self.select(&key);
                }
            }
            "columns" => self.popover = Some((Popover::Columns, Rect::default())),
            // Stopping a formatter, asked first: the dialog over the tab.
            "stop" => {
                if let Some(key) = key(&i18n::t("tasks.quality")) {
                    self.select(&key);
                    let stop = self.frame.line(&key).and_then(|l| l.actions.first().copied());
                    if let Some(stop) = stop {
                        self.request(&key, stop);
                    }
                }
            }
            // A row's right-click menu (the snapshot right-clicks the terminal's row): the
            // operations and the app's parts folded, so it's in view.
            "menu" => {
                self.state.folded.insert(GroupKey::Operations);
                self.state.folded.insert(GroupKey::Kind(workbench_model::resources::APP));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view.rs"]
mod tests;
