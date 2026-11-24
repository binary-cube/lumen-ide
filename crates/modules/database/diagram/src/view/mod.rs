//! The relations diagram as a view: what it shows (every table, or one and its neighbors,
//! narrowed by a filter), where it reads and lays out (in the background, never on the UI
//! thread), and what it reacts to (clicks, double clicks, menus, keys, scrolling, zoom).

mod edges;
mod filter;
mod navigation;
mod paint;
mod reading;
mod viewer;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use database::diagram::{DiagramRequest, Opening, SchemaGraph};
use database::relations::{Candidate, RelationKinds, RelationNotes, VirtualKey, infer};
use database::{Cancel, DbError, TableRef};
use i18n::{t, tr};
use kernel::{Point, Rect, Subscription};
use ui_components::control::{Control, Controls};
use ui_components::diagram::{History, Insets, Release, Viewport};
use ui_components::search_field::SearchField;
use ui_components::{
    AppRequest, PressMods, Tip, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerPaint,
};

use self::filter::FilterField;

use crate::THREAD;
use crate::layout::{DiagramLayout, Shown, lay_out};

/// The toolbar's height (logical pixels).
pub const TOOLBAR_H: f32 = 52.0;
/// How far it can zoom: out to see a large schema whole, in to read.
const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 2.5;
/// Fitted between these (a diagram too large to stay readable is shown around its table).
const FIT_MIN: f32 = 0.3;
const FIT_MAX: f32 = 1.0;
/// The neighbors' depths offered.
pub const DEPTHS: [usize; 3] = [1, 2, 3];
/// What the bar of controls under the diagram covers of its bottom (logical pixels):
/// fitting and centering leave it free.
pub const BAR_INSET: f32 = 64.0;
/// How near an edge the mouse has to be to be over it (logical pixels at 100%; scaled with
/// the zoom, never under [`EDGE_HIT_MIN`]).
const EDGE_HIT: f32 = 4.0;
const EDGE_HIT_MIN: f32 = 3.0;

/// Wakes the app from a background thread.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a point of the view is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Back,
    /// A table's card (its header).
    Card(usize),
    /// A column's row: table, column.
    Row(usize, usize),
    /// The filter field.
    Field,
    /// A depth of the neighbors shown.
    Depth(usize),
    /// Every table again (out of a table's neighbors).
    ShowAll,
    /// Which relations are shown.
    Kinds(RelationKinds),
    /// The list of the relations found from names, shown or hidden.
    Found,
    /// In that list: one found accepted, set aside (by its place among those found); one
    /// accepted or set aside undone (by its place in its list); a row (its table centered).
    Accept(usize),
    Ignore(usize),
    Unaccept(usize),
    Unignore(usize),
    ListRow(usize),
    /// The list itself (scrolled with the wheel, the diagram under it untouched).
    List,
    /// A line (a link of the graph drawn, by its place), or its label.
    Edge(usize),
    /// A bar over the diagram (the toolbar, the controls): nothing under it is reached.
    Bar,
    /// The popover of a line: itself, accept, set aside, undo the decision, go to a table.
    Popover,
    PopAccept,
    PopIgnore,
    PopUndo,
    GoTo(usize),
}

impl Hit {
    /// The table it's about.
    fn table(self) -> Option<usize> {
        match self {
            Self::Card(t) | Self::Row(t, _) => Some(t),
            _ => None,
        }
    }

    /// Over the diagram itself (a press there may pan it).
    fn in_diagram(self) -> bool {
        matches!(self, Self::Card(_) | Self::Row(..) | Self::Edge(_))
    }

    /// Part of a line's popover.
    fn in_popover(self) -> bool {
        matches!(
            self,
            Self::Popover | Self::PopAccept | Self::PopIgnore | Self::PopUndo | Self::GoTo(_)
        )
    }
}

/// A link known by what it joins (its place changes as decisions rebuild the graph): the
/// table it starts from, its columns, the table it points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeKey {
    pub from: usize,
    pub columns: Vec<String>,
    pub to: usize,
}

impl EdgeKey {
    fn of(graph: &SchemaGraph, link: usize) -> Option<Self> {
        let l = graph.links.get(link)?;
        Some(Self { from: l.from, columns: l.columns.clone(), to: l.to })
    }

    /// Its place in `graph`.
    fn find(&self, graph: &SchemaGraph) -> Option<usize> {
        graph.links.iter().position(|l| {
            l.from == self.from
                && l.to == self.to
                && l.columns.len() == self.columns.len()
                && l.columns.iter().zip(&self.columns).all(|(a, b)| a.eq_ignore_ascii_case(b))
        })
    }
}

/// What was being looked at (← goes back to it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    focus: Option<usize>,
    depth: usize,
    selected: Option<usize>,
}

/// What a read gives: the graph of the engine's keys, the relations its names say, what
/// the user decided.
struct Read {
    graph: SchemaGraph,
    found: Vec<Candidate>,
    notes: RelationNotes,
}

/// A read of the structure going on: its result, how far it got, how to stop it.
struct Reading {
    rx: Receiver<Result<Read, DbError>>,
    cancel: Cancel,
    progress: Arc<(AtomicUsize, AtomicUsize)>,
}

/// The view (see the module's doc).
pub struct RelationsView {
    request: Option<DiagramRequest>,
    /// The engine's keys only; the relations found from its names; what was decided.
    base: Option<Arc<SchemaGraph>>,
    found: Arc<Vec<Candidate>>,
    notes: RelationNotes,
    /// Those of [`Self::found`] not decided on yet: found again with the decisions (not at
    /// each frame the bar and the list draw).
    undecided: Vec<usize>,
    /// The base with the keys accepted and the relations found (but those set aside).
    graph: Option<Arc<SchemaGraph>>,
    /// The graph the layout shown was made from (its links' places).
    drawn: Arc<SchemaGraph>,
    /// The list of the relations found is open, its scroll (logical pixels).
    list_open: bool,
    list_scroll: f32,
    /// Where the mouse is.
    pointer: Point,
    reading: Option<Reading>,
    /// Why the structure couldn't be read.
    error: Option<String>,
    /// The table whose neighbors are shown (none: every table), how far.
    focus: Option<usize>,
    depth: usize,
    /// The filter (the app's search field: ×, ⌘A; its focus the controls').
    filter: SearchField,
    /// The filter was drawn by the last paint (only what's drawn can be pressed).
    filter_shown: bool,
    /// Its parts' state of use (see `with_controls`): the filter has the keys while it has
    /// the focus.
    controls: Controls,
    /// The table clicked last (outlined, its lines and their labels shown).
    selected: Option<usize>,
    shown: Shown,
    layout: Arc<DiagramLayout>,
    laying: Option<(Shown, Arc<SchemaGraph>, Receiver<DiagramLayout>)>,
    /// Fit once laid out (else: center the selected table).
    refit: bool,
    history: History<Place>,
    view: Viewport,
    hits: Vec<(Rect, Hit)>,
    /// The controls' tooltips, each with the part it's shown over (shown only while that
    /// part is what's under the mouse: nothing drawn over it).
    tips: Vec<(Hit, Tip)>,
    hover: Option<Hit>,
    /// The card a first click centered (a double click opens it).
    clicked: Option<usize>,
    /// The line clicked last (drawn lit, its whole label shown), its popover (where it
    /// was clicked, in diagram space; none: its middle).
    edge: Option<EdgeKey>,
    popover: Option<(EdgeKey, Option<Point>)>,
    /// The labels placed for the lines lit (found again only when what they depend on
    /// changes: the layout, the lines lit, the zoom).
    labels: paint::Labels,
    /// Counts the layouts taken (what the labels were placed for).
    layout_generation: u64,
    /// Which kinds of lines the layout shown has (the engine's, accepted, found): the
    /// legend.
    kinds_drawn: [bool; 3],
    /// The layout shown spans several schemas (each card names its own).
    several_schemas: bool,
    /// The link (among those shown) each column starts: table → column name → link.
    row_links: std::collections::HashMap<usize, std::collections::HashMap<String, usize>>,
    /// The bar of controls drawn last (the popover keeps its distance from it).
    bar: Rect,
    pending: Option<ViewerAction>,
    waker: Arc<Mutex<Option<Waker>>>,
    /// A commit changed the source's structure: read it again.
    changed: Arc<AtomicBool>,
    _watch: Option<Subscription>,
}

impl Default for RelationsView {
    fn default() -> Self {
        Self {
            request: None,
            base: None,
            found: Arc::default(),
            notes: RelationNotes::default(),
            undecided: Vec::new(),
            graph: None,
            drawn: Arc::default(),
            list_open: false,
            list_scroll: 0.0,
            pointer: Point::new(0.0, 0.0),
            reading: None,
            error: None,
            focus: None,
            depth: 1,
            filter: SearchField::new(),
            filter_shown: false,
            controls: Controls::default(),
            selected: None,
            shown: Shown::default(),
            layout: Arc::default(),
            laying: None,
            refit: true,
            history: History::default(),
            view: Viewport::new(TOOLBAR_H),
            hits: Vec::new(),
            tips: Vec::new(),
            hover: None,
            clicked: None,
            edge: None,
            popover: None,
            labels: paint::Labels::default(),
            layout_generation: 0,
            kinds_drawn: [false; 3],
            several_schemas: false,
            row_links: std::collections::HashMap::new(),
            bar: Rect::default(),
            pending: None,
            waker: Arc::default(),
            changed: Arc::default(),
            _watch: None,
        }
    }
}

impl std::fmt::Debug for RelationsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelationsView").field("request", &self.request).finish_non_exhaustive()
    }
}

impl Drop for RelationsView {
    fn drop(&mut self) {
        if let Some(reading) = &self.reading {
            reading.cancel.cancel();
        }
    }
}

impl RelationsView {
    /// The graph shown (tests, snapshots).
    pub fn graph(&self) -> Option<&SchemaGraph> {
        self.graph.as_deref()
    }

    /// The layout shown.
    pub fn layout(&self) -> &DiagramLayout {
        &self.layout
    }

    /// What's shown: the tables, the links, the table the diagram is about.
    pub fn shown(&self) -> &Shown {
        &self.shown
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn focus(&self) -> Option<(usize, usize)> {
        self.focus.map(|f| (f, self.depth))
    }

    /// Still reading or laying out.
    pub fn busy(&self) -> bool {
        self.reading.is_some() || self.laying.is_some()
    }

    /// The decisions: which relations are shown, the keys accepted and set aside.
    pub fn notes(&self) -> &RelationNotes {
        &self.notes
    }

    /// The relations found from names, not decided on yet (by their place among those
    /// found).
    pub fn undecided(&self) -> &[usize] {
        &self.undecided
    }

    /// `change` made to the decisions, kept with the source, shown.
    fn decide(&mut self, change: impl FnOnce(&mut RelationNotes)) {
        change(&mut self.notes);
        if let Some(request) = &self.request {
            request.access.set_notes(self.notes.clone());
        }
        self.rebuild();
    }

    /// Shows `kinds` of relations.
    pub fn show_kinds(&mut self, kinds: RelationKinds) {
        if kinds != self.notes.show {
            self.refit = true;
            self.decide(|n| n.show = kinds);
        }
    }

    /// The relation found at `i` accepted (a virtual key) or set aside.
    pub fn accept(&mut self, i: usize, accept: bool) -> bool {
        let Some(key) = self.found.get(i).map(|c| c.key.clone()) else { return false };
        self.decide(|n| if accept { n.accept(key) } else { n.ignore(key) });
        true
    }

    /// The key accepted (`accepted`) or set aside at `i` of its list, undone.
    pub fn undo_decision(&mut self, i: usize, accepted: bool) -> bool {
        let list = if accepted { &self.notes.virtual_keys } else { &self.notes.ignored };
        let Some(key) = list.get(i).cloned() else { return false };
        self.decide(|n| n.forget(&key));
        true
    }

    /// Centers the selected table's card (if shown).
    fn center_selected(&mut self) {
        let Some(card) = self.selected.and_then(|t| self.layout.card_of(t)) else { return };
        let rect = self.layout.layout.cards[card].rect;
        if self.view.scale > 0.0 {
            // In what the bar and the list leave of the area.
            self.update_insets();
            self.view.center_on(rect);
        }
    }

    /// What the bar of controls and the list (when open) cover of the area: fitting and
    /// centering leave it free.
    fn update_insets(&mut self) {
        let scale = self.view.scale;
        let right = if self.list_open {
            list_width(self.view.area().size.width, scale) + (24.0 * scale).round()
        } else {
            0.0
        };
        let bottom = if self.graph.is_some() { (BAR_INSET * scale).round() } else { 0.0 };
        self.view.insets = Insets { top: 0.0, right, bottom, left: 0.0 };
    }

    /// The list of the relations found, shown or hidden.
    fn toggle_list(&mut self) {
        self.list_open = !self.list_open;
        self.list_scroll = 0.0;
        self.update_insets();
    }

    /// Ends typing in the filter; Enter centers the first table it matches.
    fn stop_typing(&mut self, center: bool) {
        self.with_controls(|c, parts| c.focus(parts, None));
        if center && let Some(&t) = self.shown.tables.first() {
            self.select(t);
        }
    }

    /// The filter has the keys.
    fn typing(&self) -> bool {
        self.controls.focused().is_some()
    }

    /// Its parts (the filter), with their state of use. The diagram (panned, zoomed, its
    /// cards and lines), its bars, its list and its popovers are its own drawing; what the
    /// filter changed is followed after.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let (mut edited, mut left) = (false, None);
        let mut filter = FilterField {
            field: &mut self.filter,
            shown: self.filter_shown,
            edited: &mut edited,
            left: &mut left,
        };
        let mut parts: [&mut dyn Control; 1] = [&mut filter];
        let answer = f(&mut self.controls, &mut parts);
        if edited {
            self.filtered();
        }
        if let Some(center) = left {
            self.stop_typing(center);
        }
        answer
    }
}

/// The filter's place among the view's parts.
const FILTER: usize = 0;

/// The width of the list of the relations found (physical pixels) in an area `width` wide.
pub(crate) fn list_width(width: f32, scale: f32) -> f32 {
    (440.0 * scale).round().min(width * 0.55)
}

/// The message for a structure that couldn't be read.
fn failed(error: &str) -> String {
    tr!("relations.failed", why = error)
}

#[cfg(test)]
#[path = "../../tests/unit/view.rs"]
mod tests;
