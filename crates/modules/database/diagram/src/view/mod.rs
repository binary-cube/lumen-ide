//! The relations diagram as a view: what it shows (every table, or one and its neighbors,
//! narrowed by a filter), where it reads and lays out (in the background, never on the UI
//! thread), and what it reacts to (clicks, double clicks, menus, keys, scrolling, zoom).

mod paint;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use database::diagram::{DiagramRequest, Opening, SchemaGraph};
use database::relations::{Candidate, RelationKinds, RelationNotes, VirtualKey, infer};
use database::{Cancel, DbError, TableRef};
use i18n::{t, tr};
use kernel::{Point, Rect, Subscription};
use ui_components::diagram::{History, Insets, Release, Viewport};
use ui_components::{
    AppRequest, PressMods, Tip, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerPaint,
};
use ui_kit::TextInput;

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
    /// The filter field, its ×.
    Field,
    Clear,
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
    filter: TextInput,
    /// The filter has the keys.
    typing: bool,
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
            filter: TextInput::new(""),
            typing: false,
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

    fn wake(&self) -> Waker {
        let waker = Arc::clone(&self.waker);
        Arc::new(move || {
            if let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                wake();
            }
        })
    }

    /// Reads the source's structure again, in the background (what's shown stays until
    /// it comes).
    fn read(&mut self) {
        let Some(request) = self.request.clone() else { return };
        if let Some(old) = self.reading.take() {
            old.cancel.cancel();
        }
        let cancel = Cancel::default();
        let progress = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0)));
        let (tx, rx) = std::sync::mpsc::channel();
        let (stop, counts, wake) = (cancel.clone(), Arc::clone(&progress), self.wake());
        kernel::work::run(THREAD, move || {
            let told = Arc::clone(&wake);
            // The counts kept at each table; the app woken at most every 50 ms (thousands
            // of tables read: not a wake each), and at the last.
            let started = std::time::Instant::now();
            // When it was last woken (0: never; the first table wakes it).
            let last = AtomicU64::new(0);
            let tell = move |done: usize, total: usize| {
                counts.0.store(done, Ordering::Relaxed);
                counts.1.store(total, Ordering::Relaxed);
                let now = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let before = last.load(Ordering::Relaxed);
                let due = before == 0 || now.saturating_sub(before) >= 50;
                if due || done >= total {
                    last.store(now.max(1), Ordering::Relaxed);
                    told();
                }
            };
            let result = request.access.read(request.schema.as_deref(), &stop, &tell).map(|t| {
                let graph = SchemaGraph::new(t);
                let found = infer(&graph);
                Read { graph, found, notes: request.access.notes() }
            });
            if !stop.is_cancelled() && tx.send(result).is_ok() {
                wake();
            }
        });
        self.reading = Some(Reading { rx, cancel, progress });
    }

    /// Shows `graph` (given directly in tests): the relations its names say found here,
    /// the decisions the source keeps.
    pub fn set_graph(&mut self, graph: SchemaGraph) {
        let found = infer(&graph);
        let notes = self.request.as_ref().map(|r| r.access.notes()).unwrap_or_default();
        self.show_read(Read { graph, found, notes });
    }

    /// Shows what was read: the table asked first becomes the one whose neighbors are
    /// shown; the tables kept by their names across a read again.
    fn show_read(&mut self, read: Read) {
        let Read { graph, found, notes } = read;
        let name = |g: &Option<Arc<SchemaGraph>>, i: Option<usize>| -> Option<TableRef> {
            g.as_ref().and_then(|g| i.and_then(|i| g.tables.get(i)).map(|t| t.table().clone()))
        };
        let first = self.graph.is_none();
        let (focus, selected) = (name(&self.graph, self.focus), name(&self.graph, self.selected));
        let wanted = self.request.as_ref().and_then(|r| r.focus.clone());
        self.focus = None;
        self.selected = None;
        if first {
            if let Some(table) = wanted.and_then(|t| graph.find(&t)) {
                self.focus = Some(table);
                self.selected = Some(table);
            }
        } else {
            self.focus = focus.and_then(|t| graph.find(&t));
            self.selected = selected.and_then(|t| graph.find(&t));
            // The places before point to the tables by place: gone with the old graph.
            self.history.clear();
        }
        // The lines are known by their tables' places: gone with the old graph.
        self.edge = None;
        self.popover = None;
        self.error = None;
        self.base = Some(Arc::new(graph));
        self.found = Arc::new(found);
        self.notes = notes;
        self.refit = first;
        self.rebuild();
    }

    /// The graph made again from the base, the relations found and the decisions; laid out.
    fn rebuild(&mut self) {
        self.undecided = (0..self.found.len())
            .filter(|&i| {
                let key = &self.found[i].key;
                !self.notes.is_accepted(key) && !self.notes.is_ignored(key)
            })
            .collect();
        let Some(base) = &self.base else { return };
        self.graph = Some(Arc::new(base.with_notes(&self.found, &self.notes)));
        self.relayout();
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

    /// What's to be shown now: every table or a table's neighbors, narrowed by the filter.
    pub fn wanted(&self) -> Shown {
        let Some(graph) = &self.graph else { return Shown::default() };
        let kinds = self.notes.show;
        let mut tables = match self.focus {
            Some(f) => graph.neighborhood(f, self.depth, |l| l.origin.shown(kinds)),
            None => (0..graph.tables.len()).collect(),
        };
        let filter = self.filter.text().trim();
        if !filter.is_empty() {
            let matching = graph.matching(filter);
            tables.retain(|t| matching.binary_search(t).is_ok() || Some(*t) == self.focus);
        }
        let links = graph
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.origin.shown(kinds)
                    && tables.binary_search(&l.from).is_ok()
                    && tables.binary_search(&l.to).is_ok()
            })
            .map(|(i, _)| i)
            .collect();
        Shown { tables, links, focus: self.focus }
    }

    /// Lays out what's to be shown, in the background, unless it's what's shown already.
    fn relayout(&mut self) {
        let wanted = self.wanted();
        let Some(graph) = self.graph.clone() else { return };
        if wanted == self.shown
            && self.laying.is_none()
            && Arc::ptr_eq(&graph, &self.drawn)
            && !self.layout.layout.cards.is_empty()
        {
            return;
        }
        if self.laying.as_ref().is_some_and(|(s, g, _)| *s == wanted && Arc::ptr_eq(g, &graph)) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (shown, wake, of) = (wanted.clone(), self.wake(), Arc::clone(&graph));
        kernel::work::run(THREAD, move || {
            if tx.send(lay_out(&of, &shown)).is_ok() {
                wake();
            }
        });
        self.laying = Some((wanted, graph, rx));
    }

    /// Lays out what's to be shown here and now (tests, snapshots: the same function the
    /// background runs).
    pub fn lay_out_now(&mut self) {
        let Some(graph) = self.graph.clone() else { return };
        let wanted = self.wanted();
        self.laying = None;
        let layout = lay_out(&graph, &wanted);
        self.take_layout(wanted, graph, layout);
    }

    fn take_layout(&mut self, shown: Shown, graph: Arc<SchemaGraph>, layout: DiagramLayout) {
        self.kinds_drawn = [false; 3];
        // What each frame would otherwise look for in the whole model: the schemas, each
        // row's link (the first shown that starts at it).
        let mut schemas = graph.tables.iter().filter_map(|t| t.table().schema.as_deref());
        let first = schemas.next();
        self.several_schemas = schemas.any(|s| Some(s) != first);
        self.row_links.clear();
        for &i in &shown.links {
            if let Some(l) = graph.links.get(i)
                && let Some(column) = l.columns.first()
            {
                self.row_links.entry(l.from).or_default().entry(column.clone()).or_insert(i);
            }
        }
        for l in shown.links.iter().filter_map(|&l| graph.links.get(l)) {
            let kind = match l.origin {
                database::diagram::Origin::Real => 0,
                database::diagram::Origin::Virtual => 1,
                database::diagram::Origin::Inferred(_) => 2,
            };
            self.kinds_drawn[kind] = true;
        }
        self.shown = shown;
        self.drawn = graph;
        self.layout = Arc::new(layout);
        self.layout_generation += 1;
        if std::mem::take(&mut self.refit) {
            self.view.fit_pending = true;
        } else {
            self.center_selected();
        }
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

    /// The line selected (its place in the graph shown).
    pub fn selected_edge(&self) -> Option<usize> {
        self.edge.as_ref().and_then(|e| e.find(self.graph.as_deref()?))
    }

    /// The line whose popover is open (its place in the graph shown).
    pub fn popover_edge(&self) -> Option<usize> {
        self.popover.as_ref().and_then(|(e, _)| e.find(self.graph.as_deref()?))
    }

    /// The line at screen point `p` (its place in the graph drawn): the nearest within a
    /// few pixels, scaled with the zoom; none over the bars or out of view.
    pub fn edge_at(&self, p: Point) -> Option<usize> {
        if !self.view.visible().contains(p) || self.view.scale <= 0.0 {
            return None;
        }
        let k = self.view.k();
        let reach = (EDGE_HIT * k).max(EDGE_HIT_MIN * self.view.scale) / k;
        let d = self.view.to_diagram(p);
        self.layout
            .layout
            .edges
            .iter()
            .filter(|e| ui_components::diagram::inflate(e.bounds(), reach).contains(d))
            .map(|e| (e.distance(d), e.kind))
            .filter(|(dist, _)| *dist <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, link)| link)
    }

    /// Line `link` (of the graph drawn) selected, its popover open at `at` (diagram space;
    /// none: its middle).
    pub fn open_edge(&mut self, link: usize, at: Option<Point>) -> bool {
        let Some(key) = EdgeKey::of(&self.drawn, link) else { return false };
        self.edge = Some(key.clone());
        self.popover = Some((key, at));
        true
    }

    /// The popover closed (the line stays selected).
    pub fn close_popover(&mut self) -> bool {
        self.popover.take().is_some()
    }

    /// The key the popover's line stands for (as the decisions keep it).
    fn popover_key(&self) -> Option<VirtualKey> {
        let graph = self.graph.as_deref()?;
        let link = graph.links.get(self.popover_edge()?)?;
        Some(VirtualKey {
            table: graph.tables.get(link.from)?.table().clone(),
            columns: link.columns.clone(),
            target: graph.tables.get(link.to)?.table().clone(),
            references: link.references.clone(),
        })
    }

    /// The popover's line accepted (`Some(true)`), set aside (`Some(false)`), or its
    /// decision undone (`None`).
    fn decide_edge(&mut self, accept: Option<bool>) -> bool {
        let Some(key) = self.popover_key() else { return false };
        let done = match accept {
            Some(accept) => match self.found.iter().position(|c| c.key.same(&key)) {
                Some(i) => self.accept(i, accept),
                None => false,
            },
            None => match self.notes.virtual_keys.iter().position(|k| k.same(&key)) {
                Some(i) => self.undo_decision(i, true),
                None => false,
            },
        };
        // Set aside: the line is gone with its popover.
        if accept == Some(false) && done {
            self.popover = None;
            self.edge = None;
        }
        done
    }

    /// The selected table's lines (shown), the next after the line selected (`back`: the
    /// one before) selected.
    fn cycle_edges(&mut self, back: bool) -> bool {
        let Some(t) = self.selected else { return false };
        let graph = Arc::clone(&self.drawn);
        let lines: Vec<usize> = self
            .shown
            .links
            .iter()
            .copied()
            .filter(|&l| graph.links.get(l).is_some_and(|l| l.from == t || l.to == t))
            .collect();
        if lines.is_empty() {
            return false;
        }
        let at = self
            .edge
            .as_ref()
            .and_then(|e| e.find(&graph))
            .and_then(|l| lines.iter().position(|&x| x == l));
        let n = lines.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        self.edge = EdgeKey::of(&graph, lines[next]);
        self.popover = None;
        true
    }

    fn place(&self) -> Place {
        Place { focus: self.focus, depth: self.depth, selected: self.selected }
    }

    /// Goes to `place` (the current one remembered for ←).
    fn go(&mut self, place: Place) {
        if place == self.place() {
            self.center_selected();
            return;
        }
        let before = self.place();
        self.history.push(before.clone());
        let regroup = place.focus != before.focus || place.depth != before.depth;
        self.apply(place, regroup);
    }

    fn apply(&mut self, place: Place, regroup: bool) {
        self.focus = place.focus;
        self.depth = place.depth;
        self.selected = place.selected;
        if regroup {
            self.refit = true;
            self.relayout();
        } else {
            self.center_selected();
        }
    }

    pub fn back(&mut self) -> bool {
        let Some(place) = self.history.pop() else { return false };
        let regroup = place.focus != self.focus || place.depth != self.depth;
        self.apply(place, regroup);
        true
    }

    /// Table `t` selected and centered.
    pub fn select(&mut self, t: usize) {
        self.go(Place { selected: Some(t), ..self.place() });
    }

    /// Table `t`'s neighbors shown (`depth` links away).
    pub fn show_neighbors(&mut self, t: usize, depth: usize) {
        self.go(Place { focus: Some(t), depth, selected: Some(t) });
    }

    /// Every table shown again.
    pub fn show_all(&mut self) {
        self.go(Place { focus: None, depth: self.depth, selected: self.selected });
    }

    /// The filter's text changed: what matches is shown.
    fn filtered(&mut self) {
        self.refit = true;
        self.relayout();
    }

    /// What opens table `t`'s data.
    fn open(&self, t: usize) -> Option<ViewerAction> {
        let request = self.request.as_ref()?;
        let table = self.graph.as_ref()?.tables.get(t).filter(|t| !t.external)?.table().clone();
        Some(match request.access.open(&table)? {
            Opening::File(path, asked) => {
                ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
                    host.open_file_with(&path, &asked);
                }))
            }
            Opening::View(v) => ViewerAction::OpenView(ui_components::OpenView::shared(
                v.title,
                v.extension,
                v.data,
            )),
        })
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        match self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h) {
            Some(hit) => Some(hit),
            None => self.edge_at(p).map(Hit::Edge),
        }
    }

    /// What a click on `hit` (at screen point `at`) does.
    fn activate(&mut self, hit: Hit, at: Point) {
        match hit {
            Hit::Back => {
                self.back();
            }
            Hit::Card(t) | Hit::Row(t, _) => {
                self.clicked = Some(t);
                self.select(t);
            }
            Hit::Clear => {
                self.filter.set_text("");
                self.filtered();
            }
            Hit::Depth(d) => {
                if let Some(f) = self.focus
                    && d != self.depth
                {
                    self.show_neighbors(f, d);
                }
            }
            Hit::ShowAll => self.show_all(),
            Hit::Kinds(kinds) => self.show_kinds(kinds),
            Hit::Found => self.toggle_list(),
            Hit::Accept(i) => {
                self.accept(i, true);
            }
            Hit::Ignore(i) => {
                self.accept(i, false);
            }
            Hit::Unaccept(i) => {
                self.undo_decision(i, true);
            }
            Hit::Unignore(i) => {
                self.undo_decision(i, false);
            }
            Hit::ListRow(t) => self.select(t),
            Hit::Edge(link) => {
                let at = self.view.to_diagram(at);
                self.open_edge(link, Some(at));
            }
            Hit::PopAccept => {
                self.decide_edge(Some(true));
            }
            Hit::PopIgnore => {
                self.decide_edge(Some(false));
            }
            Hit::PopUndo => {
                self.decide_edge(None);
            }
            Hit::GoTo(t) => {
                self.popover = None;
                self.select(t);
            }
            Hit::Field | Hit::List | Hit::Bar | Hit::Popover => {}
        }
    }

    /// The menu of table `t`'s card.
    fn card_menu(&self, t: usize) -> Vec<ViewMenuEntry> {
        let item = |id: String, key: &str| ViewMenuEntry::item(id, i18n::t(key));
        let external = self.graph.as_ref().and_then(|g| g.tables.get(t)).is_none_or(|t| t.external);
        let mut entries = Vec::new();
        if !external {
            entries.push(item(format!("open:{t}"), "relations.menu_open").with_icon("table"));
        }
        entries.push(
            item(format!("neighbors:{t}"), "relations.menu_neighbors").with_icon("relations"),
        );
        if self.focus.is_some() {
            entries.push(item("all".into(), "relations.menu_all"));
        }
        entries.push(ViewMenuEntry::Separator);
        entries.push(item(format!("copy:{t}"), "relations.menu_copy").with_icon("copy"));
        entries
    }

    /// Ends typing in the filter; Enter centers the first table it matches.
    fn stop_typing(&mut self, center: bool) {
        self.typing = false;
        if center && let Some(&t) = self.shown.tables.first() {
            self.select(t);
        }
    }
}

impl Viewer for RelationsView {
    /// Its source's color (its own, else its folder's).
    fn accent(&self, palette: &ui_components::Palette) -> Option<kernel::Rgba> {
        let color = self.request.as_ref()?.access.color()?;
        ui_components::tag_colors::named(&color.name(), palette)
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(request) = data.downcast_ref::<DiagramRequest>() else { return };
        self._watch = request.events.as_ref().map(|events| {
            let (flag, wake, id) =
                (Arc::clone(&self.changed), self.wake(), request.access.source());
            events.subscribe(move |e: &database::events::DataChanged| {
                if e.source == id && e.structure {
                    flag.store(true, Ordering::Release);
                    wake();
                }
            })
        });
        self.request = Some(request.clone());
        self.graph = None;
        self.history.clear();
        self.refit = true;
        self.read();
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        if self.changed.swap(false, Ordering::AcqRel) {
            self.read();
            changed = true;
        }
        if let Some(reading) = &self.reading {
            match reading.rx.try_recv() {
                Ok(result) => {
                    self.reading = None;
                    match result {
                        Ok(read) => self.show_read(read),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                    changed = true;
                }
                // Its progress is drawn as it goes.
                Err(TryRecvError::Empty) => changed = true,
                Err(TryRecvError::Disconnected) => {
                    self.reading = None;
                    changed = true;
                }
            }
        }
        if let Some((shown, graph, rx)) = self.laying.take() {
            match rx.try_recv() {
                Ok(layout) => {
                    self.take_layout(shown, graph, layout);
                    changed = true;
                }
                Err(TryRecvError::Empty) => self.laying = Some((shown, graph, rx)),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        changed
    }

    fn is_loading(&self) -> bool {
        self.busy()
    }

    fn command(&mut self, id: &str) -> bool {
        // The search has the keys: the app's editing commands are its (⌘A, ⌘Z, ⌘⇧Z, ⌘X).
        if self.typing {
            let edited = match id {
                "select_all" => {
                    self.filter.select_all();
                    return true;
                }
                "undo" => self.filter.undo(),
                "redo" => self.filter.redo(),
                "cut" => {
                    self.filter.insert("");
                    true
                }
                _ => false,
            };
            if edited {
                self.filtered();
                return true;
            }
        }
        match id {
            "fit" => {
                self.view.fit_pending = true;
                true
            }
            "back" => self.back(),
            "find" => {
                self.typing = true;
                self.filter.select_all();
                true
            }
            "refresh" => {
                self.read();
                true
            }
            "all" => {
                self.show_all();
                true
            }
            "found" => {
                self.toggle_list();
                true
            }
            // A line by the column it starts from (`table.column`): selected, its popover
            // open.
            id if id.starts_with("edge:") => {
                let Some((table, column)) = id["edge:".len()..].split_once('.') else {
                    return false;
                };
                let graph = Arc::clone(&self.drawn);
                let link = graph.links.iter().position(|l| {
                    graph.tables.get(l.from).is_some_and(|t| t.table().name == table)
                        && l.columns.first().is_some_and(|c| c.eq_ignore_ascii_case(column))
                });
                link.is_some_and(|l| self.open_edge(l, None))
            }
            id if id.starts_with("kinds:") => match RelationKinds::parse(&id["kinds:".len()..]) {
                Some(kinds) => {
                    self.show_kinds(kinds);
                    true
                }
                None => false,
            },
            // A table by its name (`schema.name` or `name`): its neighbors, or it selected.
            id if id.starts_with("focus-table:") || id.starts_with("select-table:") => {
                let (what, name) = id.split_once(':').unwrap_or_default();
                let table = match name.split_once('.') {
                    Some((schema, name)) => TableRef::in_schema(schema, name),
                    None => TableRef::new(name),
                };
                let Some(t) = self.graph.as_ref().and_then(|g| g.find(&table)) else {
                    return false;
                };
                if what == "focus-table" {
                    self.show_neighbors(t, self.depth);
                } else {
                    self.select(t);
                }
                true
            }
            id => {
                let Some((what, t)) = id.split_once(':') else { return false };
                let Ok(t) = t.parse::<usize>() else { return false };
                match what {
                    "open" => {
                        self.pending = self.open(t);
                        self.pending.is_some()
                    }
                    "neighbors" => {
                        self.show_neighbors(t, self.depth);
                        true
                    }
                    "select" => {
                        self.select(t);
                        true
                    }
                    "depth" => match self.focus {
                        Some(f) => {
                            self.show_neighbors(f, t.clamp(1, 3));
                            true
                        }
                        None => false,
                    },
                    "accept" => self.accept(t, true),
                    "ignore" => self.accept(t, false),
                    "unaccept" => self.undo_decision(t, true),
                    "unignore" => self.undo_decision(t, false),
                    "copy" => {
                        let Some(name) = self
                            .graph
                            .as_ref()
                            .and_then(|g| g.tables.get(t))
                            .map(|t| t.table().to_string())
                        else {
                            return false;
                        };
                        self.pending = Some(ViewerAction::Request(AppRequest::CopyText(name)));
                        true
                    }
                    _ => false,
                }
            }
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.list_open
            && self.hits.iter().any(|(r, h)| *h == Hit::List && r.contains(self.pointer))
        {
            self.list_scroll = (self.list_scroll + dy / self.view.scale.max(1.0)).max(0.0);
            return true;
        }
        self.view.scroll(dx, dy);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.view.zoom_at(factor, at, ZOOM_MIN, ZOOM_MAX)
    }

    fn zoom_level(&self) -> Option<f32> {
        Some(self.view.zoom)
    }

    fn reset_zoom(&mut self) -> bool {
        self.view.reset_zoom(ZOOM_MIN, ZOOM_MAX)
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, _mods: PressMods) -> Option<ViewerAction> {
        let hit = self.hit(p);
        // A popover closes at a click outside it (as the app's popovers do); the click
        // goes on.
        if !hit.is_some_and(Hit::in_popover) {
            self.popover = None;
        }
        if hit == Some(Hit::Field) {
            self.typing = true;
            self.filter.press(p.x, clicks);
            return None;
        }
        self.typing = false;
        // The second click of a double click: the card the first one centered opens.
        if clicks >= 2
            && let Some(t) = self.clicked.take()
        {
            self.view.press = None;
            return self.open(t);
        }
        if hit.and_then(Hit::table).is_none() {
            self.clicked = None;
        }
        // Over the list, a bar or a popover: its buttons act at release, the diagram
        // doesn't pan.
        if hit.is_some_and(|h| !h.in_diagram()) {
            self.view.press(p);
            self.view.moved = false;
            return None;
        }
        self.view.press(p);
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.typing {
            self.filter.extend_to(p.x);
            return true;
        }
        // Pressed on the list or a control: no pan.
        let at = self.view.press.and_then(|at| self.hit(at));
        if at.is_some_and(|h| !h.in_diagram()) {
            return false;
        }
        self.view.drag(p)
    }

    fn release(&mut self) -> bool {
        match self.view.release() {
            Release::None => false,
            Release::Panned => {
                self.clicked = None;
                true
            }
            Release::Click(at) => {
                let Some(hit) = self.hit(at) else {
                    // On nothing: the line selected let go.
                    return self.edge.take().is_some();
                };
                self.activate(hit, at);
                true
            }
        }
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let t = self.hit(p).and_then(Hit::table)?;
        Some(self.card_menu(t))
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let under = self.hit(p)?;
        self.tips
            .iter()
            .rev()
            .find(|(hit, tip)| *hit == under && tip.anchor.contains(p))
            .map(|(_, tip)| tip.clone())
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match self.hit(p)? {
            Hit::Field => Some(ViewerCursor::Text),
            Hit::Bar | Hit::Popover | Hit::List => None,
            _ => Some(ViewerCursor::Pointer),
        }
    }

    /// Its hover says when what it shows changed.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = p;
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn wants_text(&self) -> bool {
        self.typing
    }

    fn text_input(&mut self, text: &str) -> bool {
        if !self.typing {
            return false;
        }
        self.filter.insert(text);
        self.filtered();
        true
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        if self.typing {
            return self.edit_key(key);
        }
        match key {
            // Esc: the popover closed, then the line let go.
            ViewerKey::Escape => self.close_popover() || self.edge.take().is_some(),
            // Tab (⇧: back) goes through the selected table's lines; Enter opens the
            // popover of the one selected.
            ViewerKey::Tab => self.cycle_edges(reach.extend),
            ViewerKey::Enter => match self.selected_edge() {
                Some(_) if self.popover.is_some() => false,
                Some(_) => {
                    let key = self.edge.clone();
                    let link = key.and_then(|k| k.find(&self.drawn));
                    link.is_some_and(|l| self.open_edge(l, None))
                }
                None => false,
            },
            _ => false,
        }
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if !self.typing {
            return self.edit_key_with(key, ui_components::KeyReach::default());
        }
        let before = self.filter.text().to_owned();
        match key {
            ViewerKey::Backspace => self.filter.backspace(false),
            ViewerKey::Delete => self.filter.delete(),
            ViewerKey::Left => self.filter.move_left(false, false),
            ViewerKey::Right => self.filter.move_right(false, false),
            ViewerKey::Home => self.filter.home(false),
            ViewerKey::End => self.filter.end(false),
            ViewerKey::Escape if !before.is_empty() => self.filter.set_text(""),
            ViewerKey::Escape => self.stop_typing(false),
            ViewerKey::Enter => self.stop_typing(true),
            _ => return false,
        }
        if self.filter.text() != before {
            self.filtered();
        }
        true
    }

    fn select_all(&mut self) -> bool {
        self.typing && {
            self.filter.select_all();
            true
        }
    }

    fn status(&self) -> Option<String> {
        self.graph.as_ref()?;
        Some(tr!(
            "relations.status",
            tables = self.shown.tables.len(),
            links = self.shown.links.len()
        ))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let Some(waker) = &ctx.waker {
            *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(waker));
        }
        self.paint_view(ctx);
    }
}

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
