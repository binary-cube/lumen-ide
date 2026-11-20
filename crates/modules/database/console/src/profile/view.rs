//! A statement's profile, seen (plan 39h4): its tab in the Services window ("Profile ·
//! orders", in place of 39h's Plan tab) and the same view in an editor tab (`view-profile`,
//! "Open in Tab"), over the same `Arc<ProfileReport>`.
//!
//! - **The bar** (40 px): the views' switcher (the tree, the diagram — [`graph`] —, the
//!   flame — [`flame`] —, the tables — [`tables`] —), Compare ▾ (the console's other
//!   profiles, the same query's first: [`compare`]), the steps' filter, Expand All / Collapse All, Open in Tab,
//!   Export ▾, the panel's drawer when it isn't beside; on its right `ANALYZE` / `EXPLAIN`,
//!   "rolled back", the engine and its version; between them, the statement as sent. What
//!   doesn't fit goes into "…".
//! - **The summary** under it ([`summary`]): a pill per figure, the findings by severity on
//!   its right (a click filters the list).
//! - **The body** ([`layout`]): the view chosen (the tree: [`tree`]) and, past a splitter (its width kept for
//!   the project's session), the findings ([`findings`]) over the details ([`details`]);
//!   narrower, the panel is a drawer over the tree; narrower still, the findings go above it
//!   as a compact list.
//!
//! A finding clicked chooses its step (scrolled to it) and lights its place in the console
//! while the statement's text is the one profiled ([`Runner::set_focus`]); its SQL is copied
//! or inserted at the console's end, selected, never run. Keys: the tree's (↑ ↓ ← → Home End
//! PgUp PgDn ↵ `*`), F2 / ⇧F2 the next / previous problem, H the hottest step, ⌘C the row,
//! typing filters the steps (Esc clears). The three views share the step chosen: a card or
//! a bar clicked is the tree's row, the findings' card, the console's place; a table chosen
//! is its first step. Compared, the body is the comparison (both plans' steps aligned, their
//! deltas, the findings resolved and new) until it's closed.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use database::profile::derive::ProfileReport;
use database::profile::node::NodeId;
use database::profile::rules::Finding;
use kernel::{Memo, Point, Rect, Rgba};
use render_api::Quad;
use text_layout::Font;
use ui_components::control::{Control, Controls, Divider, NOWHERE, Press, Thumb};
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{
    AppRequest, Level, Orientation, PressMods, ServiceId, Splitter, SplitterStyle, TablePaint, Tip,
    TreeTable, ViewMemory, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerPaint,
};
use ui_kit::{draw_icon, draw_text};
use workbench_model::Notification;

use crate::profile::compare::{CompareEvent, CompareView, Side};
use crate::profile::details::{self, DetailHit, DetailsList};
use crate::profile::export::{self, ExportKind};
use crate::profile::findings::{FindingHit, FindingsList, ListPaint};
use crate::profile::flame::FlameView;
use crate::profile::graph::{GraphEvent, GraphView};
use crate::profile::layout::{self, LayoutSpec, ProfileLayout};
use crate::profile::tables::{TablesRow, TablesTable};
use crate::profile::tree::{self, Col, ProfileTable};
use crate::profile::{analysis, figures, summary};
use crate::results::PlanSet;
use crate::runner::{FocusMark, Runner, millis};

use self::paint::*;

mod actions;
mod paint;
mod tables;
mod viewer;

/// The views of a profile (the switcher's; the tables come with 39h6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileMode {
    Tree,
    Graph,
    Flame,
    Tables,
}

impl ProfileMode {
    pub const ALL: [ProfileMode; 4] =
        [ProfileMode::Tree, ProfileMode::Graph, ProfileMode::Flame, ProfileMode::Tables];

    fn id(self) -> &'static str {
        match self {
            ProfileMode::Tree => "tree",
            ProfileMode::Graph => "graph",
            ProfileMode::Flame => "flame",
            ProfileMode::Tables => "tables",
        }
    }

    pub fn label(self) -> String {
        i18n::t(&format!("profile.view.mode_{}", self.id()))
    }

    /// It's made (all of them, since 39h6).
    pub fn available(self) -> bool {
        true
    }

    /// Its icon.
    pub fn icon(self) -> &'static str {
        match self {
            ProfileMode::Tree => "plan",
            ProfileMode::Graph => "graph",
            ProfileMode::Flame => "flame",
            ProfileMode::Tables => "table",
        }
    }
}

/// What a profile's editor tab is given: the console's runner and the profile's set (its
/// report, statement, place).
#[derive(Debug, Clone)]
pub struct ProfileTabData {
    pub runner: Runner,
    pub set: PlanSet,
}

/// What opened it: a tab of the Services window (read again from the runner), or an editor
/// tab (its profile given).
#[derive(Debug, Clone)]
enum Source {
    Services(ServiceId),
    Tab,
    None,
}

/// The key of the panel's width in the project's sizes ([`workbench_model::LayoutSizes`]).
pub const PANEL_SIZE: &str = "database.profile-panel";

/// A part of the view (its bar, its summary, its splitter, its menu).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    Mode(usize),
    ModeMenu,
    Filter,
    FilterClear,
    ExpandAll,
    CollapseAll,
    OpenInTab,
    Export,
    More,
    Drawer,
    Statement,
    Depth,
    RolledBack,
    Engine,
    Pill(usize),
    Severity(Level),
    Compare,
    TablesNote,
    Compact(usize),
    MenuItem(usize),
    DrawerClose,
}

/// A dropdown the view draws itself (Export ▾, "…", the switcher as a menu): its rows' ids
/// come back as commands.
#[derive(Debug, Clone)]
struct Dropdown {
    anchor: Rect,
    items: Vec<(String, String, &'static str, bool)>,
    rect: Rect,
}

/// An export being written in the background.
type Pending = Arc<Mutex<Option<Result<PathBuf, String>>>>;

/// A profile's view (see the module's doc).
pub struct ProfileView {
    runner: Option<Runner>,
    source: Source,
    set: Option<PlanSet>,
    seen: Option<(Option<u64>, u64)>,
    table: TreeTable,
    made: Option<(database::profile::ProfileId, Vec<Col>, bool, u64)>,
    built: ProfileTable,
    columns_all: Vec<Col>,
    /// The findings shown (those of rules not ignored), by index into the report's.
    shown: Vec<usize>,
    shown_generation: u64,
    ignored: Vec<String>,
    severity: Option<Level>,
    selected_finding: Option<usize>,
    findings: FindingsList,
    details: DetailsList,
    details_for: Option<(Option<NodeId>, u64)>,
    sections: Vec<details::Section>,
    mode: ProfileMode,
    graph: GraphView,
    flame: FlameView,
    filter: String,
    typing: bool,
    panel_w: f32,
    /// The project's dragged sizes: the panel's width is kept there ([`PANEL_SIZE`]).
    sizes: Option<workbench_model::LayoutSizes>,
    splitter: Splitter,
    drawer_open: bool,
    layout: Memo<LayoutKey, ProfileLayout>,
    laid: ProfileLayout,
    scale: f32,
    bounds: Rect,
    hits: Vec<(Rect, Hit, Option<String>)>,
    hover: Option<Hit>,
    dropdown: Option<Dropdown>,
    /// The panel's two parts this frame.
    findings_area: Rect,
    details_area: Rect,
    /// The panel's two heads this frame (their lines meet the tree's header's).
    panel_heads: [Rect; 2],
    /// The source's name, its color, the console's name (read once).
    source_name: String,
    console_name: String,
    accent: Option<String>,
    action: Option<ViewerAction>,
    notifications: Vec<Notification>,
    export: Option<Pending>,
    /// Where the mouse was last (the wheel goes to what's under it).
    /// What a press on a side panel's list hit (taken by the view after it).
    finding_pressed: Option<FindingHit>,
    detail_pressed: Option<DetailHit>,
    /// The wheel over a side panel moved its list.
    panel_scrolled: bool,
    /// The tables' view: its table, what it was made of (the profile, the facts, the findings
    /// shown), what each row is.
    tables: TreeTable,
    tables_built: TablesTable,
    tables_made: Option<(database::profile::ProfileId, usize, u64)>,
    /// The tables' columns that fit the room.
    tables_fit: Vec<crate::profile::tables::TCol>,
    /// What's known of the profile's tables (the console's latest, else its tab's).
    facts: Option<Arc<database::profile::rules::Facts>>,
    /// Its tables' statistics were asked for (once).
    stats_asked: bool,
    /// Two profiles compared, while it is.
    compare: Option<CompareView>,
    /// The body's view's room this frame (the tables' and the comparison's take the panel's
    /// too, when it's beside).
    body: Rect,
    /// Its parts' state of use (see `with_controls`).
    controls: Controls,
    /// Where the held splitter's line goes (turned into the panel's width).
    split_at: Option<f32>,
}

impl std::fmt::Debug for ProfileView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileView").field("source", &self.source).finish_non_exhaustive()
    }
}

/// Logical sizes.
/// The panel's heads: as tall as the tree's header, so their lines run on across the
/// splitter.
const PANEL_HEAD: f32 = ui_components::tree_table::HEADER_H;

/// What the layout is worked out of: the bounds, the scale, the panel's width (bits), the
/// drawer, the compact rows.
type LayoutKey = (u32, u32, u32, u32, u32, u32, bool, usize);

impl ProfileView {
    fn blank() -> Self {
        let mut table = TreeTable::new(Vec::new());
        table.set_empty(i18n::t("console.plan_empty"));
        Self {
            runner: None,
            source: Source::None,
            set: None,
            seen: None,
            table,
            made: None,
            built: ProfileTable::default(),
            columns_all: Vec::new(),
            shown: Vec::new(),
            shown_generation: 0,
            ignored: Vec::new(),
            severity: None,
            selected_finding: None,
            findings: FindingsList::default(),
            details: DetailsList::default(),
            details_for: None,
            sections: Vec::new(),
            mode: ProfileMode::Tree,
            graph: GraphView::default(),
            flame: FlameView::default(),
            filter: String::new(),
            typing: false,
            panel_w: layout::PANEL_W,
            sizes: None,
            splitter: Splitter::new(SplitterStyle::default(), Orientation::Vertical),
            drawer_open: false,
            layout: Memo::default(),
            laid: ProfileLayout::default(),
            scale: 1.0,
            bounds: Rect::default(),
            hits: Vec::new(),
            hover: None,
            dropdown: None,
            findings_area: Rect::default(),
            details_area: Rect::default(),
            panel_heads: [Rect::default(); 2],
            source_name: String::new(),
            console_name: String::new(),
            accent: None,
            action: None,
            notifications: Vec::new(),
            export: None,
            finding_pressed: None,
            detail_pressed: None,
            panel_scrolled: false,
            tables: TreeTable::new(Vec::new()),
            tables_built: TablesTable::default(),
            tables_made: None,
            tables_fit: Vec::new(),
            facts: None,
            stats_asked: false,
            compare: None,
            body: Rect::default(),
            controls: Controls::default(),
            split_at: None,
        }
    }

    /// The profile of tab `id` of `runner`'s console (the Services window's).
    pub fn new(runner: Runner, id: ServiceId) -> Self {
        let mut view = Self::blank();
        view.source = Source::Services(id);
        view.attach(runner);
        view.refresh();
        view
    }

    /// An editor tab's view, given its profile later ([`Viewer::set_data`]).
    pub fn empty() -> Self {
        Self::blank()
    }

    /// An editor tab's view of `data`.
    pub fn of(data: ProfileTabData) -> Self {
        let mut view = Self::blank();
        view.source = Source::Tab;
        view.set = Some(data.set);
        view.attach(data.runner);
        view.refresh();
        view
    }

    /// The project's sizes: the panel as wide as it was last dragged in the project.
    fn take_sizes(&mut self, sizes: workbench_model::LayoutSizes) {
        if let Some(width) = sizes.get(PANEL_SIZE) {
            self.panel_w = width.max(layout::PANEL_MIN);
        }
        self.sizes = Some(sizes);
    }

    fn attach(&mut self, runner: Runner) {
        let consoles = runner.project().and_then(crate::consoles::consoles);
        let brief =
            consoles.as_ref().and_then(|c| c.access()).and_then(|a| a.source(runner.source()));
        self.source_name = brief.as_ref().map_or_else(|| runner.source_name(), |b| b.name.clone());
        self.accent = brief.and_then(|b| b.color).map(|c| c.name());
        self.console_name = runner.name();
        self.runner = Some(runner);
    }

    pub fn table(&self) -> &TreeTable {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut TreeTable {
        &mut self.table
    }

    /// The profile shown.
    pub fn report(&self) -> Option<&Arc<ProfileReport>> {
        self.set.as_ref()?.report.as_ref()
    }

    /// The findings shown (rules ignored left out), the worst first.
    pub fn shown_findings(&self) -> Vec<&Finding> {
        let Some(report) = self.report() else { return Vec::new() };
        shown_of(report, &self.shown)
    }

    /// The findings of the list (the severity's filter too): indices into the shown ones.
    fn listed(&self) -> Vec<usize> {
        let findings = self.shown_findings();
        (0..findings.len())
            .filter(|&i| {
                self.severity.is_none_or(|l| {
                    findings[i].is_problem() && figures::level(findings[i].severity) == l
                })
            })
            .collect()
    }

    /// The finding chosen (an index into the shown ones).
    pub fn selected_finding(&self) -> Option<usize> {
        self.selected_finding
    }

    /// The step chosen in the tree.
    pub fn selected_node(&self) -> Option<NodeId> {
        self.table.selected().and_then(|r| self.built.nodes.get(r).copied())
    }

    /// The columns shown.
    pub fn columns(&self) -> &[Col] {
        &self.built.cols
    }

    pub fn severity_filter(&self) -> Option<Level> {
        self.severity
    }

    /// The panel's heads (the findings', the details') last drawn.
    pub fn panel_heads(&self) -> [Rect; 2] {
        self.panel_heads
    }

    pub fn layout_of(&self) -> ProfileLayout {
        self.laid
    }

    /// The view shown (the tree, the diagram, the flame).
    pub fn mode(&self) -> ProfileMode {
        self.mode
    }

    /// `mode` shown, when it's made; `false` otherwise.
    pub fn set_mode(&mut self, mode: ProfileMode) -> bool {
        if !mode.available() {
            return false;
        }
        self.mode = mode;
        self.dropdown = None;
        // Another view chosen: the comparison left.
        self.compare = None;
        if mode == ProfileMode::Tables {
            self.ask_stats();
        }
        true
    }

    /// The tables' view's table.
    pub fn tables(&self) -> &TreeTable {
        &self.tables
    }

    /// What the tables' view was made of (its rows' kinds, the usage).
    pub fn tables_built(&self) -> &TablesTable {
        &self.tables_built
    }

    /// The comparison shown, if one is.
    pub fn comparing(&self) -> Option<&CompareView> {
        self.compare.as_ref()
    }

    pub fn comparing_mut(&mut self) -> Option<&mut CompareView> {
        self.compare.as_mut()
    }

    pub fn graph(&self) -> &GraphView {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut GraphView {
        &mut self.graph
    }

    pub fn flame(&self) -> &FlameView {
        &self.flame
    }

    pub fn flame_mut(&mut self) -> &mut FlameView {
        &mut self.flame
    }

    /// Step `node` chosen, from any of the views: the tree's row (its parents opened), its
    /// finding chosen with it (its place lit in the console), or none.
    pub fn select_node(&mut self, node: Option<NodeId>) {
        let row = node
            .and_then(|n| self.built.row_of.get(n.index()).copied())
            .filter(|&r| r != usize::MAX);
        self.table.select(row);
        self.follow_choice();
    }

    /// The step chosen in the tree: its problem chosen too (or none).
    fn follow_choice(&mut self) {
        let node = self.selected_node();
        self.sync_tables(node);
        let findings = self.shown_findings();
        // Its own problem first (the one about it before one it's only part of).
        let found = node.and_then(|n| {
            let about = |f: &&Finding| f.is_problem() && f.nodes.first() == Some(&n);
            findings
                .iter()
                .position(about)
                .or_else(|| findings.iter().position(|f| f.is_problem() && f.nodes.contains(&n)))
        });
        if let Some(f) = found {
            if self.selected_finding != Some(f) {
                let row = self.table.selected();
                self.choose_finding(f);
                // The step chosen stays chosen (the finding may start at another).
                self.table.select(row);
            }
        } else {
            self.selected_finding = None;
            if let Some(runner) = &self.runner {
                runner.set_focus(None);
            }
        }
        self.details_for = None;
    }

    /// What the diagram asked for: a step chosen, folded or opened.
    fn graph_event(&mut self, event: GraphEvent) {
        match event {
            GraphEvent::Selected(node) => self.select_node(Some(node)),
            GraphEvent::Toggle(node) => {
                if let Some(&row) = self.built.row_of.get(node.index()) {
                    self.table.toggle(row);
                }
            }
        }
    }

    /// The profile read again when its tab's generation (or the runner's: what's ignored)
    /// changed; its table made again when the profile or its columns did. `true` if
    /// something changed.
    fn refresh(&mut self) -> bool {
        let Some(runner) = self.runner.clone() else { return false };
        let tab = match self.source {
            Source::Services(id) => runner.result_generation(id),
            _ => Some(0),
        };
        let key = (tab, runner.generation());
        if self.seen == Some(key) {
            return false;
        }
        self.seen = Some(key);
        if let Source::Services(id) = self.source {
            self.set = runner.plan(id);
        }
        let ignored = runner.ignored();
        if ignored != self.ignored || self.made.is_none() {
            self.ignored = ignored;
            self.shown_generation += 1;
        }
        let Some(report) = self.report().cloned() else {
            self.compare = None;
            return true;
        };
        self.facts = runner
            .profile_facts(report.profile.id)
            .or_else(|| self.set.as_ref().and_then(|s| s.facts.clone()));
        if self.compare.as_ref().is_some_and(|c| {
            let own = if c.shown_after { &c.after.report } else { &c.before.report };
            own.profile.id != report.profile.id
        }) {
            self.compare = None;
        }
        self.shown = (0..report.findings.len())
            .filter(|&i| !self.ignored.iter().any(|r| r == report.findings[i].rule))
            .collect();
        self.columns_all = tree::columns_of(&report);
        self.rebuild(&report, None);
        true
    }

    /// The table made again for `fit` (the room's), when anything it shows changed.
    fn rebuild(&mut self, report: &Arc<ProfileReport>, fit: Option<tree::Fit>) {
        let fit = fit.unwrap_or_else(|| match self.made.as_ref() {
            Some(m) => tree::Fit { cols: m.1.clone(), compact: m.2 },
            None => tree::Fit { cols: self.columns_all.clone(), compact: false },
        });
        let cols = fit.cols.clone();
        let key = (report.profile.id, cols.clone(), fit.compact, self.shown_generation);
        if self.made.as_ref() == Some(&key) {
            return;
        }
        let first = self.made.as_ref().is_none_or(|m| m.0 != report.profile.id);
        let open: Vec<bool> = (0..self.table.rows().len()).map(|r| self.table.is_open(r)).collect();
        let findings = self.shown_findings();
        let built = tree::table(report, &cols, &findings);
        self.table.set_columns(built.columns.clone());
        if fit.compact {
            for (i, c) in cols.iter().enumerate().skip(1) {
                self.table.set_width(i, c.compact_width());
            }
        }
        let starts = built.open.clone();
        if first {
            self.table.set_rows(built.rows.clone(), |i| starts.get(i).copied().unwrap_or(true));
        } else {
            self.table.set_rows(built.rows.clone(), |i| open.get(i).copied().unwrap_or(true));
        }
        self.built = built;
        self.made = Some(key);
        self.details_for = None;
    }

    // ─── Acting ───

    /// Finding `i` (of those shown) chosen: its step chosen and brought into view, its
    /// place lit in the console.
    pub fn choose_finding(&mut self, i: usize) {
        let findings = self.shown_findings();
        let Some(f) = findings.get(i) else { return };
        let node = f.nodes.first().copied();
        let range = f.sql_range.clone();
        let level = figures::level(f.severity);
        self.selected_finding = Some(i);
        if let Some(node) = node
            && let Some(&row) = self.built.row_of.get(node.index())
        {
            self.table.select(Some(row));
        }
        self.sync_tables(node);
        if let Some(pos) = self.listed().iter().position(|&l| l == i) {
            self.findings.reveal(pos);
        }
        let focus = match (range, self.set.as_ref().and_then(|s| s.document_hash)) {
            (Some(range), Some(hash)) => self.set.as_ref().map(|s| FocusMark {
                statement: s.range.clone(),
                hash,
                range,
                level,
            }),
            _ => None,
        };
        if let Some(runner) = &self.runner {
            runner.set_focus(focus);
        }
    }

    /// The next (`forward`) or previous problem from the one chosen (round).
    pub fn next_problem(&mut self, forward: bool) -> bool {
        let findings = self.shown_findings();
        let problems: Vec<usize> =
            (0..findings.len()).filter(|&i| findings[i].is_problem()).collect();
        if problems.is_empty() {
            return false;
        }
        let at = self.selected_finding.and_then(|s| problems.iter().position(|&p| p == s));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => problems.len() - 1,
            (Some(a), true) => (a + 1) % problems.len(),
            (Some(a), false) => (a + problems.len() - 1) % problems.len(),
        };
        self.choose_finding(problems[next]);
        true
    }

    /// The costliest branch's end chosen.
    pub fn hottest(&mut self) -> bool {
        let Some(report) = self.report().cloned() else { return false };
        let Some(&last) = report.hot_path.last() else { return false };
        let Some(&row) = self.built.row_of.get(last.index()) else { return false };
        self.table.select(Some(row));
        true
    }

    /// `profile [source] · orders`.
    pub fn tab_title(&self) -> String {
        let table = self.set.as_ref().and_then(|s| s.table.clone());
        match table {
            Some(table) => i18n::tr!(
                "profile.view.tab_title_of",
                source = self.source_name.as_str(),
                table = table.as_str()
            ),
            None => i18n::tr!("profile.view.tab_title", source = self.source_name.as_str()),
        }
    }

    // ─── Drawing ───
}

#[cfg(test)]
#[path = "../../tests/unit/profile/view.rs"]
mod tests;
