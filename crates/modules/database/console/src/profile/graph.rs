//! A profile as a diagram (plan 39h5, on [`ui_components::diagram`]): a card per step, laid
//! out tidily ([`diagram::tidy`]) top down or left to right — whichever shows more of it in
//! the room, or as chosen —, the edges from each child into its parent (the way the rows
//! flow), as thick as the rows they carry (`log10`, 1–8 px) and labelled with them, the
//! costliest branch in the accent.
//!
//! - **A card:** its top in the heat of its own time, the kind's icon and the engine's
//!   words, a problem's sign in its corner (its border in the severity's color, the hot
//!   spot's flame); its lines — the object (or what it works on), its own time and its time
//!   in all with its share, the rows expected → real with the factor when far off, the cost
//!   and the loops. The tooltip says the exact figures and the finding.
//! - **What's folded in the tree** is folded here too: a "+N steps" card under it (a click
//!   opens it). The tree's filter dims what doesn't match.
//! - **Looking:** dragged to pan, the wheel pans, ⌘⌥ + wheel (in an editor tab) or + / −
//!   zoom, 0 fits; the controls in its corner zoom, fit, center the chosen step and turn the
//!   flow. A click chooses a step in every view (the tree's choice, the findings' card);
//!   a double click folds or opens it; the arrows walk the tree.
//!
//! Only the cards in view are drawn; the layout is worked out once per profile, what's
//! open and the flow ([`kernel::Memo`]).

use database::profile::derive::ProfileReport;
use database::profile::export;
use database::profile::node::{NodeId, ObjectKind};
use database::profile::rules::Finding;
use kernel::{Memo, Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::diagram::{self, Dir, Edge, EdgeStyle, Flow, Gaps, Head, Release, Viewport};
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx, Tone, radius};
use ui_components::{Level, Palette, Tip, TreeTable};
use ui_kit::{draw_icon, draw_text};

use crate::profile::figures::{self, level};
use crate::profile::{analysis, tree};

/// A step's card (diagram pixels at 100 %).
pub const CARD_W: f32 = 236.0;
pub const HEADER_H: f32 = 30.0;
pub const LINE_H: f32 = 19.0;
pub const PAD: f32 = 8.0;
/// The heat stripe across a card's top.
pub const STRIPE: f32 = 3.0;
/// The pills on the cards and the edges: the room on each side of their label, a card's
/// pill's height and fill (its color's alpha), an edge's outline (its color's alpha).
const PILL_PAD: f32 = 5.0;
const CARD_PILL_H: f32 = 15.0;
const CARD_PILL_FILL: f32 = 0.2;
const EDGE_PILL_BORDER: f32 = 0.35;
/// The rule under a card's header: its alpha of the cards' border.
const CARD_RULE: f32 = 0.7;
/// A folded subtree's card.
pub const FOLD_W: f32 = 150.0;
pub const FOLD_H: f32 = 30.0;
/// The zoom's bounds, and those a fit keeps (readable).
pub const ZOOM_MIN: f32 = 0.15;
pub const ZOOM_MAX: f32 = 2.0;
pub const FIT_MIN: f32 = 0.75;
pub const FIT_MAX: f32 = 1.0;

/// An edge's thickness (logical pixels at 100 %) for the rows it carries: `1 + log10`,
/// between 1 and 8.
pub fn edge_width(rows: f64) -> f32 {
    if !rows.is_finite() || rows <= 1.0 {
        return 1.0;
    }
    (1.0 + rows.log10() as f32).clamp(1.0, 8.0)
}

/// What a card stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Step(NodeId),
    /// The steps folded under `under` (open in the tree: shown again).
    Folded {
        under: NodeId,
        count: usize,
    },
}

/// A card laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub item: Item,
    pub rect: Rect,
}

/// An edge's rows: how thick, its label and where it goes (`None`: no room clear of the
/// cards; shown under the mouse).
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeInfo {
    pub rows: Option<f64>,
    pub width: f32,
    pub hot: bool,
    pub label: Option<String>,
    pub label_at: Option<Rect>,
}

/// The diagram laid out: its cards, edges (child → parent, by card), each edge's rows, its
/// bounds, the flow.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphLayout {
    pub flow: Flow,
    pub cards: Vec<Card>,
    pub edges: Vec<Edge<(usize, usize)>>,
    pub info: Vec<EdgeInfo>,
    pub bounds: Rect,
    /// Step → its card (`usize::MAX`: not shown, folded away).
    pub card_of: Vec<usize>,
}

/// How many lines a step's card has in `report` (the same for every card).
pub fn line_count(report: &ProfileReport) -> usize {
    let measured = report.totals.measure.is_some();
    let rows =
        report.profile.nodes.iter().any(|n| n.estimates.rows.is_some() || n.actuals.rows.is_some());
    let cost = report.profile.nodes.iter().any(|n| n.estimates.total_cost.is_some());
    1 + usize::from(measured) + usize::from(rows) + usize::from(cost)
}

/// A step's card's height.
pub fn card_h(lines: usize) -> f32 {
    HEADER_H + lines as f32 * LINE_H + PAD
}

/// The rows a step gives its parent (real in all its loops, else expected).
pub fn rows_out(report: &ProfileReport, id: NodeId) -> Option<f64> {
    let node = report.profile.node(id);
    report
        .derived(id)
        .total_rows
        .or_else(|| node.estimates.rows.map(|r| r * node.actuals.loops.unwrap_or(1.0).max(1.0)))
}

/// `report` laid out with `flow`, the steps under a folded one (`open(id)` false) left out
/// for a "+N" card.
pub fn layout(report: &ProfileReport, open: &dyn Fn(NodeId) -> bool, flow: Flow) -> GraphLayout {
    let p = &report.profile;
    let n = p.nodes.len();
    let lines = line_count(report);
    let step_size = (CARD_W, card_h(lines));
    let mut items: Vec<Item> = Vec::new();
    let mut sizes: Vec<(f32, f32)> = Vec::new();
    let mut children: Vec<Vec<usize>> = Vec::new();
    let mut card_of = vec![usize::MAX; n];
    let mut roots = Vec::new();
    // Below each step, how many there are (for "+N").
    let mut below = vec![0usize; n];
    for &id in report.order.iter().rev() {
        if let Some(parent) = p.node(id).parent {
            below[parent.index()] += below[id.index()] + 1;
        }
    }
    for &id in &report.order {
        let node = p.node(id);
        let parent_card = match node.parent {
            Some(parent) => {
                let at = card_of[parent.index()];
                if at == usize::MAX || !open(parent) {
                    continue;
                }
                Some(at)
            }
            None => None,
        };
        let at = items.len();
        items.push(Item::Step(id));
        sizes.push(step_size);
        children.push(Vec::new());
        card_of[id.index()] = at;
        match parent_card {
            Some(pc) => children[pc].push(at),
            None => roots.push(at),
        }
        if !node.children.is_empty() && !open(id) {
            let fold = items.len();
            items.push(Item::Folded { under: id, count: below[id.index()] });
            sizes.push((FOLD_W, FOLD_H));
            children.push(Vec::new());
            children[at].push(fold);
        }
    }
    let gaps = match flow {
        Flow::Down => Gaps { sibling: 28.0, subtree: 40.0, level: 64.0 },
        Flow::Right => Gaps { sibling: 22.0, subtree: 34.0, level: 80.0 },
    };
    let rects = diagram::tidy(&children, &roots, &sizes, flow, gaps);
    let edges = diagram::links(&children, &rects, flow);
    let cards: Vec<Card> =
        items.iter().zip(&rects).map(|(item, rect)| Card { item: *item, rect: *rect }).collect();
    let obstacles: Vec<Rect> = cards.iter().map(|c| diagram::inflate(c.rect, 4.0)).collect();
    let mut info: Vec<EdgeInfo> = edges
        .iter()
        .map(|e| {
            let (child, parent) = e.kind;
            let (rows, hot) = match (items[child], items[parent]) {
                (Item::Step(c), Item::Step(pa)) => {
                    (rows_out(report, c), report.on_hot_path(c) && report.on_hot_path(pa))
                }
                _ => (None, false),
            };
            let label = rows.map(figures::count);
            EdgeInfo { rows, width: rows.map_or(1.0, edge_width), hot, label, label_at: None }
        })
        .collect();
    // The labels where they're clear of the cards and of each other, the thickest first.
    let mut by_weight: Vec<usize> = (0..edges.len()).collect();
    by_weight.sort_by(|&a, &b| info[b].width.total_cmp(&info[a].width));
    let spots: Vec<Vec<Rect>> = by_weight
        .iter()
        .map(|&i| match &info[i].label {
            Some(label) => {
                let size = (label.chars().count() as f32 * 7.0 + 14.0, 18.0);
                diagram::label_spots(&edges[i].points, size, 4.0 + info[i].width / 2.0)
            }
            None => Vec::new(),
        })
        .collect();
    let placed = diagram::place_labels(&spots, &obstacles);
    for (k, &i) in by_weight.iter().enumerate() {
        info[i].label_at = placed[k].map(|s| spots[k][s]);
    }
    let bounds = diagram::bounds_of(cards.iter().map(|c| c.rect));
    GraphLayout { flow, cards, edges, info, bounds, card_of }
}

/// A part of the diagram under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphHit {
    Card(usize),
    ZoomIn,
    ZoomOut,
    Zoom,
    Fit,
    Center,
    Flow,
}

/// What a click on the diagram meant for the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphEvent {
    /// A step chosen.
    Selected(NodeId),
    /// A step folded or opened (a double click, a "+N" card).
    Toggle(NodeId),
}

/// What the diagram is drawn from.
pub struct GraphPaint<'a, 'b> {
    pub area: Rect,
    pub scale: f32,
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'b Palette,
    pub icons: &'b icons::Icons,
    pub report: &'b ProfileReport,
    pub findings: &'b [&'b Finding],
    pub table: &'b mut TreeTable,
    /// Step → its row in the table.
    pub row_of: &'b [usize],
    pub selected: Option<NodeId>,
}

impl std::fmt::Debug for GraphPaint<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphPaint").field("area", &self.area).finish_non_exhaustive()
    }
}

/// The diagram's state (see the module's doc).
#[derive(Debug)]
pub struct GraphView {
    pub view: Viewport,
    /// The flow chosen (`None`: whichever shows more).
    pub chosen: Option<Flow>,
    flow: Flow,
    laid: [Memo<(database::profile::ProfileId, u64), GraphLayout>; 2],
    fitted: Option<(database::profile::ProfileId, Flow)>,
    last_selected: Option<NodeId>,
    hover: Option<GraphHit>,
    controls: Vec<(Rect, GraphHit, String)>,
    tips: Vec<(Rect, String)>,
    area: Rect,
    /// The worst finding about each step (for its sign and tooltip), by step.
    worst: Vec<Option<usize>>,
}

impl Default for GraphView {
    fn default() -> Self {
        Self {
            view: Viewport::new(0.0),
            chosen: None,
            flow: Flow::Right,
            laid: [Memo::default(), Memo::default()],
            fitted: None,
            last_selected: None,
            hover: None,
            controls: Vec::new(),
            tips: Vec::new(),
            area: Rect::default(),
            worst: Vec::new(),
        }
    }
}

fn flow_slot(flow: Flow) -> usize {
    match flow {
        Flow::Down => 0,
        Flow::Right => 1,
    }
}

impl GraphView {
    /// The flow drawn.
    pub fn flow(&self) -> Flow {
        self.flow
    }

    /// The layout drawn last.
    pub fn laid(&self) -> Option<&GraphLayout> {
        self.laid[flow_slot(self.flow)].value()
    }

    /// The layouts of both flows for `report` (what's open in `table`), and the flow that
    /// shows more of it in `area` (or the one chosen).
    fn lay_out(&mut self, report: &ProfileReport, table: &mut TreeTable, row_of: &[usize]) {
        let key = (report.profile.id, table.generation());
        let open = |id: NodeId| row_of.get(id.index()).is_none_or(|&r| table.is_open(r));
        for flow in [Flow::Down, Flow::Right] {
            self.laid[flow_slot(flow)].get_or_update(key, || layout(report, &open, flow));
        }
        self.flow = self.chosen.unwrap_or_else(|| {
            let fit = |flow: Flow| {
                let b = self.laid[flow_slot(flow)].value().map_or(Rect::default(), |l| l.bounds);
                let (w, h) = (
                    self.area.size.width / self.view.scale.max(0.01),
                    self.area.size.height / self.view.scale.max(0.01),
                );
                if b.size.width <= 0.0 {
                    0.0
                } else {
                    (w / b.size.width).min(h / b.size.height.max(1.0))
                }
            };
            let (down, right) = (fit(Flow::Down), fit(Flow::Right));
            // One that shows it whole and readable: the larger; else the one along the
            // room's longer side (more levels in view around the chosen step).
            if down.max(right) >= FIT_MIN {
                if down > right * 1.05 { Flow::Down } else { Flow::Right }
            } else if self.area.size.width >= self.area.size.height * 1.2 {
                Flow::Right
            } else {
                Flow::Down
            }
        });
    }

    /// Fitted at the next paint (the flow turned, a new profile, asked).
    pub fn refit(&mut self) {
        self.fitted = None;
    }

    /// The flow turned (from automatic to the other one).
    pub fn turn(&mut self) {
        self.chosen = Some(match self.flow {
            Flow::Down => Flow::Right,
            Flow::Right => Flow::Down,
        });
        self.refit();
    }

    fn card_rect_of(&self, id: NodeId) -> Option<Rect> {
        let l = self.laid()?;
        let &c = l.card_of.get(id.index())?;
        l.cards.get(c).map(|c| c.rect)
    }

    /// Pans so the chosen step is in the middle.
    pub fn center_on(&mut self, id: NodeId) -> bool {
        let Some(r) = self.card_rect_of(id) else { return false };
        self.view.center_on(r);
        true
    }

    /// Fits the diagram (around `focus` when it's too big to read whole).
    pub fn fit(&mut self, focus: Option<NodeId>) {
        let Some(bounds) = self.laid().map(|l| l.bounds) else { return };
        let focus = focus.and_then(|f| self.card_rect_of(f));
        self.view.fit(bounds, focus, FIT_MIN, FIT_MAX);
        self.keep_filled(bounds);
    }

    /// The pan kept so the diagram fills what's visible: along an axis it overflows, no empty
    /// room past its edges; along one it fits, centered.
    fn keep_filled(&mut self, bounds: Rect) {
        let (a, v) = (self.view.area(), self.view.visible());
        let k = self.view.k();
        if k <= 0.0 {
            return;
        }
        let margin = 24.0;
        let axis = |pan: f32, view_min: f32, view_len: f32, lo: f32, len: f32| -> f32 {
            // `pan` is the diagram point at the area's start; the visible part starts later.
            let start = pan + view_min / k;
            let span = view_len / k;
            let new_start = if len + 2.0 * margin <= span {
                lo + len / 2.0 - span / 2.0
            } else {
                start.clamp(lo - margin, lo + len + margin - span)
            };
            new_start - view_min / k
        };
        self.view.pan = Point::new(
            axis(
                self.view.pan.x,
                v.min_x() - a.min_x(),
                v.size.width,
                bounds.min_x(),
                bounds.size.width,
            ),
            axis(
                self.view.pan.y,
                v.min_y() - a.min_y(),
                v.size.height,
                bounds.min_y(),
                bounds.size.height,
            ),
        );
    }

    pub fn zoom_by(&mut self, factor: f32, at: Option<Point>) -> bool {
        let a = self.view.visible();
        let at = at
            .unwrap_or(Point::new(a.min_x() + a.size.width / 2.0, a.min_y() + a.size.height / 2.0));
        self.view.zoom_at(factor, at, ZOOM_MIN, ZOOM_MAX)
    }

    /// The card at screen point `p`.
    fn card_at(&self, p: Point) -> Option<usize> {
        if !self.area.contains(p) {
            return None;
        }
        let d = self.view.to_diagram(p);
        self.laid()?.cards.iter().position(|c| c.rect.contains(d))
    }

    fn hit(&self, p: Point) -> Option<GraphHit> {
        if let Some((_, h, _)) = self.controls.iter().find(|(r, ..)| r.contains(p)) {
            return Some(*h);
        }
        self.card_at(p).map(GraphHit::Card)
    }

    /// A press: a control acts at once (`true`); a double click on a card folds it; else a
    /// pan may begin (a click told at the release).
    pub fn press(
        &mut self,
        p: Point,
        clicks: u32,
        selected: Option<NodeId>,
    ) -> (bool, Option<GraphEvent>) {
        match self.hit(p) {
            Some(GraphHit::ZoomIn) => return (self.zoom_by(1.25, None), None),
            Some(GraphHit::ZoomOut) => return (self.zoom_by(0.8, None), None),
            Some(GraphHit::Zoom) => {
                return (self.view.reset_zoom(ZOOM_MIN, ZOOM_MAX), None);
            }
            Some(GraphHit::Fit) => {
                self.fit(selected);
                return (true, None);
            }
            Some(GraphHit::Center) => return (selected.is_some_and(|s| self.center_on(s)), None),
            Some(GraphHit::Flow) => {
                self.turn();
                return (true, None);
            }
            Some(GraphHit::Card(c)) if clicks >= 2 => {
                if let Some(Item::Step(id)) =
                    self.laid().and_then(|l| l.cards.get(c)).map(|c| c.item)
                {
                    return (true, Some(GraphEvent::Toggle(id)));
                }
            }
            _ => {}
        }
        if self.area.contains(p) {
            self.view.press(p);
        }
        (false, None)
    }

    pub fn drag(&mut self, p: Point) -> bool {
        self.view.drag(p)
    }

    /// The mouse released: a click on a card chooses its step (a "+N" one opens it).
    pub fn release(&mut self) -> Option<GraphEvent> {
        let Release::Click(at) = self.view.release() else { return None };
        let c = self.card_at(at)?;
        match self.laid()?.cards.get(c)?.item {
            Item::Step(id) => Some(GraphEvent::Selected(id)),
            Item::Folded { under, .. } => Some(GraphEvent::Toggle(under)),
        }
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hit = self.hit(p);
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn leave(&mut self) -> bool {
        self.hover.take().is_some()
    }

    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.view.scroll(dx, dy);
        true
    }

    /// The step whose card is at `p`.
    pub fn node_at(&self, p: Point) -> Option<NodeId> {
        let c = self.card_at(p)?;
        match self.laid()?.cards.get(c)?.item {
            Item::Step(id) => Some(id),
            Item::Folded { .. } => None,
        }
    }

    pub fn is_control(&self, p: Point) -> bool {
        self.controls.iter().any(|(r, ..)| r.contains(p))
    }

    pub fn is_over_card(&self, p: Point) -> bool {
        self.card_at(p).is_some()
    }

    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        if let Some((r, _, tip)) = self.controls.iter().find(|(r, ..)| r.contains(p)) {
            return Some(Tip::new(*r, tip.clone()));
        }
        if !self.area.contains(p) {
            return None;
        }
        let (r, tip) = self.tips.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(Tip::new(*r, tip.clone()))
    }

    /// The step the arrows lead to from `from`: the parent toward the roots, the first
    /// child away, the siblings across (by the flow's axes).
    pub fn step_from(
        report: &ProfileReport,
        from: NodeId,
        key: ui_components::ViewerKey,
        flow: Flow,
    ) -> Option<NodeId> {
        use ui_components::ViewerKey as K;
        let p = &report.profile;
        let node = p.node(from);
        let (to_parent, to_child, back, forth) = match flow {
            Flow::Down => (K::Up, K::Down, K::Left, K::Right),
            Flow::Right => (K::Left, K::Right, K::Up, K::Down),
        };
        let siblings = |id: NodeId| match p.node(id).parent {
            Some(parent) => p.node(parent).children.clone(),
            None => p.roots.clone(),
        };
        if key == to_parent {
            node.parent
        } else if key == to_child {
            node.children.first().copied()
        } else if key == back || key == forth {
            let s = siblings(from);
            let at = s.iter().position(|&x| x == from)?;
            let to = if key == back { at.checked_sub(1)? } else { at + 1 };
            s.get(to).copied()
        } else {
            None
        }
    }

    // ─── Drawing ───

    pub fn paint(&mut self, ctx: GraphPaint<'_, '_>) {
        let GraphPaint {
            area,
            scale,
            text,
            scene,
            palette: p,
            icons,
            report,
            findings,
            table,
            row_of,
            selected,
        } = ctx;
        let ds = Ctx::new(p, scale);
        self.area = area;
        self.view.bounds = area;
        self.view.scale = scale;
        self.controls.clear();
        self.tips.clear();
        self.worst = tree::worst_of(report, findings);
        self.lay_out(report, table, row_of);
        let base = scene.push_layer(Some(area));
        base.quad(Quad::filled(area, p.editor_background));
        let Some(laid) = self.laid().cloned() else { return };
        if laid.cards.is_empty() {
            ds::empty_state(base, text, &ds, area, &i18n::t("console.plan_empty"), None);
            return;
        }
        // Fitted once per profile and flow; the chosen step followed into view.
        if self.fitted != Some((report.profile.id, self.flow)) {
            let focus = selected.or_else(|| report.hot_path.last().copied());
            self.fit(focus);
            self.fitted = Some((report.profile.id, self.flow));
            self.last_selected = selected;
        } else if selected != self.last_selected {
            self.last_selected = selected;
            if let Some(r) = selected.and_then(|s| self.card_rect_of(s)) {
                let on = self.view.rect_to_screen(r);
                if !diagram::contains_rect(self.view.visible(), on) {
                    self.view.center_on(r);
                }
            }
        }
        let k = self.view.k();
        let zoom = self.view.zoom;
        // A dot grid: the canvas's surface, the same at any zoom.
        let origin = self.view.to_screen(Point::new(0.0, 0.0));
        diagram::dot_grid(base, p, area, origin, self.view.scale);
        let view_rect = Rect::new(
            self.view.to_diagram(Point::new(area.min_x(), area.min_y())).x,
            self.view.to_diagram(Point::new(area.min_x(), area.min_y())).y,
            area.size.width / k,
            area.size.height / k,
        );
        let filter = !table.filter().is_empty();
        let matches =
            |id: NodeId| !filter || row_of.get(id.index()).is_some_and(|&r| table.matches(r));
        // The edges first, under the cards: the thin ones, then the thick, the hot path last.
        let edges_layer = scene.push_layer(Some(area));
        let mut order: Vec<usize> = (0..laid.edges.len()).collect();
        order.sort_by(|&a, &b| {
            (laid.info[a].hot, laid.info[a].width)
                .partial_cmp(&(laid.info[b].hot, laid.info[b].width))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &i in &order {
            let e = &laid.edges[i];
            if e.bounds().intersect(&diagram::inflate(view_rect, 20.0)).is_none()
                && !rect_holds(view_rect, e.bounds())
            {
                continue;
            }
            let info = &laid.info[i];
            let dim = match laid.cards[e.kind.0].item {
                Item::Step(c) => !matches(c),
                Item::Folded { .. } => false,
            };
            let mut color = if info.hot { p.accent } else { p.text_muted.with_alpha(0.55) };
            if dim {
                color = color.with_alpha(color.a * 0.35);
            }
            let style = EdgeStyle {
                color,
                dashed: matches!(laid.cards[e.kind.0].item, Item::Folded { .. }),
                head: Head::Arrow { hollow: None },
                width: info.width,
            };
            let points: Vec<Point> = e.points.iter().map(|&pt| self.view.to_screen(pt)).collect();
            diagram::edge(edges_layer, &points, e.dir, k, style);
        }
        // The labels of the rows, on a pill so they read over the lines.
        let label_font =
            Font::new(p.ui_font.clone(), ((p.ui_font_size - 2.5) * k).round().max(6.0));
        if zoom >= 0.4 {
            for info in &laid.info {
                let (Some(label), Some(at)) = (&info.label, info.label_at) else { continue };
                if at.intersect(&view_rect).is_none() {
                    continue;
                }
                let r = self.view.rect_to_screen(at);
                let color = if info.hot { p.accent } else { p.text_muted };
                // Its sizes in the diagram's units, zoomed with it.
                let pill = ds::Badge::new(label)
                    .color(color)
                    .fill(p.editor_background)
                    .border(color.with_alpha(EDGE_PILL_BORDER))
                    .font(&label_font)
                    .size(ds::BadgeSize::Custom(at.size.height))
                    .pad(PILL_PAD)
                    .zoom(k / ds.scale);
                let w = pill.width(text, &ds);
                let left = r.min_x() + (r.size.width - w) / 2.0;
                pill.paint(edges_layer, text, &ds, left, r.min_y() + r.size.height / 2.0);
            }
        }
        // The cards in view.
        let cards_layer = scene.push_layer(Some(area));
        let fonts = Fonts::new(p, k);
        for (ci, card) in laid.cards.iter().enumerate() {
            if card.rect.intersect(&view_rect).is_none() {
                continue;
            }
            let r = self.view.rect_to_screen(card.rect);
            let hovered = self.hover == Some(GraphHit::Card(ci));
            match card.item {
                Item::Folded { under, count } => {
                    paint_folded(
                        &mut ui_components::LayerPainter::new(cards_layer, text, icons, ds),
                        &fonts,
                        (r, k),
                        (report, under),
                        count,
                        hovered,
                    );
                    self.tips.push((r, i18n::t("profile.view.graph_open_folded")));
                }
                Item::Step(id) => {
                    let finding = self
                        .worst
                        .get(id.index())
                        .copied()
                        .flatten()
                        .and_then(|f| findings.get(f).copied());
                    let look = Look {
                        selected: selected == Some(id),
                        hovered,
                        hot: report.on_hot_path(id),
                        dim: !matches(id),
                        hot_spot: hot_spot(findings) == Some(id),
                    };
                    paint_card(
                        &mut ui_components::LayerPainter::new(cards_layer, text, icons, ds),
                        &fonts,
                        (r, k),
                        (report, id),
                        finding,
                        look,
                    );
                    self.tips.push((r, card_tip(report, id, finding)));
                }
            }
        }
        // The controls, floating in the corner: zoom, fit, center, the flow.
        let controls = scene.push_layer(Some(area));
        self.paint_controls(controls, text, icons, &ds, area, selected.is_some());
        self.paint_legend(controls, text, &ds, area);
    }

    fn paint_controls(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        area: Rect,
        can_center: bool,
    ) {
        let px = |v: f32| ds.px(v);
        let h = px(30.0);
        let font = ds.ui_font(-1.5);
        let pct = format!("{} %", (self.view.zoom * 100.0).round() as i64);
        let pct_w = text.shape_line("200 %", &font).width + px(14.0);
        let b = px(28.0);
        let w = b * 5.0 + pct_w + px(4.0) * 2.0 + px(10.0);
        let r = Rect::new(
            (area.max_x() - w - px(12.0)).round(),
            (area.max_y() - h - px(12.0)).round(),
            w.round(),
            h,
        );
        if r.min_x() < area.min_x() + px(8.0) || r.min_y() < area.min_y() + px(8.0) {
            return;
        }
        ds::surface(layer, ds, r, ds::Elevation::Low, px(radius::MD));
        let mut x = r.min_x() + px(3.0);
        let flow_icon = match self.flow {
            Flow::Down => "arrow-down",
            Flow::Right => "arrow-right",
        };
        let flow_tip = match self.flow {
            Flow::Down => i18n::t("profile.view.graph_flow_right"),
            Flow::Right => i18n::t("profile.view.graph_flow_down"),
        };
        let items: [(GraphHit, Option<&str>, String); 6] = [
            (GraphHit::ZoomOut, Some("minus"), i18n::t("profile.view.graph_zoom_out")),
            (GraphHit::Zoom, None, i18n::t("profile.view.graph_zoom_reset")),
            (GraphHit::ZoomIn, Some("plus"), i18n::t("profile.view.graph_zoom_in")),
            (GraphHit::Fit, Some("maximize-2"), i18n::t("profile.view.graph_fit")),
            (GraphHit::Center, Some("crosshair"), i18n::t("profile.view.graph_center")),
            (GraphHit::Flow, Some(flow_icon), flow_tip),
        ];
        for (i, (hit, icon, tip)) in items.into_iter().enumerate() {
            if i == 3 {
                ds::divider_vertical(
                    layer,
                    ds,
                    (x + px(2.0)).round(),
                    r.min_y() + px(7.0),
                    r.max_y() - px(7.0),
                );
                x += px(4.0);
            }
            let bw = if icon.is_none() { pct_w } else { b };
            let br = Rect::new(x, r.min_y() + px(2.0), bw, h - px(4.0));
            let enabled = hit != GraphHit::Center || can_center;
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .enabled(enabled)
                .hovered(self.hover == Some(hit));
            // The zoom's figure (a reset) reads as a label, muted; the rest are icons.
            let spec = match icon {
                Some(icon) => spec.icon(icon),
                None => spec.label(&pct).tone(Tone::Neutral),
            };
            ds::button(layer, text, icons, ds, br, spec);
            self.controls.push((br, hit, tip));
            x += bw;
        }
    }

    /// What the lines and colors mean, in the bottom left corner (when there's room).
    fn paint_legend(&mut self, layer: &mut Layer, text: &mut TextSystem, ds: &Ctx<'_>, area: Rect) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-2.0);
        let rows = text.shape_line(&i18n::t("profile.view.graph_legend_rows"), &font);
        let hot = text.shape_line(&i18n::t("profile.view.graph_legend_hot"), &font);
        let heat = text.shape_line(&i18n::t("profile.view.graph_legend_heat"), &font);
        let sample = px(26.0);
        let ramp = px(44.0);
        let w = rows.width + hot.width + heat.width + sample * 2.0 + ramp + px(60.0);
        let controls_w = self.controls.first().map_or(0.0, |(r, ..)| area.max_x() - r.min_x());
        if w + controls_w + px(40.0) > area.size.width {
            return;
        }
        let h = px(26.0);
        let r = Rect::new(
            (area.min_x() + px(12.0)).round(),
            (area.max_y() - h - px(14.0)).round(),
            w.round(),
            h,
        );
        layer.quad(
            Quad::filled(r, p.editor_background.with_alpha(0.92))
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.card_border),
        );
        let mid = (r.min_y() + h / 2.0).round();
        let base = ds::optical_baseline(r.min_y(), h, &font);
        let mut x = r.min_x() + px(10.0);
        // Thin to thick: the rows.
        for (i, t) in [1.0f32, 2.5, 4.5].iter().enumerate() {
            let tw = px(*t).max(1.0);
            layer.quad(Quad::filled(
                Rect::new(
                    (x + i as f32 * px(9.0)).round(),
                    (mid - tw / 2.0).round(),
                    px(7.0),
                    tw.round().max(1.0),
                ),
                p.text_muted,
            ));
        }
        x += sample + px(4.0);
        draw_text(layer, &rows, x.round(), base, p.text_muted);
        x += rows.width + px(12.0);
        let tw = px(2.5).round();
        layer.quad(Quad::filled(
            Rect::new(x.round(), (mid - tw / 2.0).round(), sample, tw),
            p.accent,
        ));
        x += sample + px(6.0);
        draw_text(layer, &hot, x.round(), base, p.text_muted);
        x += hot.width + px(12.0);
        let steps = 10;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let sw = ramp / steps as f32;
            layer.quad(Quad::filled(
                Rect::new((x + sw * i as f32).round(), mid - px(4.0), sw.ceil(), px(8.0)),
                p.heat(t),
            ));
        }
        x += ramp + px(6.0);
        draw_text(layer, &heat, x.round(), base, p.text_muted);
    }
}

/// `outer` holds `inner` whole.
fn rect_holds(outer: Rect, inner: Rect) -> bool {
    diagram::contains_rect(outer, inner)
}

/// The step the hot-spot finding is about.
fn hot_spot(findings: &[&Finding]) -> Option<NodeId> {
    findings.iter().find(|f| !f.is_problem()).and_then(|f| f.nodes.first().copied())
}

/// The cards' fonts at scale `k` (diagram → screen).
#[derive(Debug)]
pub struct Fonts {
    title: Font,
    title_strong: Font,
    line: Font,
    line_strong: Font,
    code: Font,
    pill: Font,
}

impl Fonts {
    pub fn new(p: &Palette, k: f32) -> Self {
        let size = |delta: f32| ((p.ui_font_size + delta) * k).round().max(5.0);
        let title = Font::new(p.ui_font.clone(), size(-0.5));
        Self {
            title_strong: title.clone().with_weight(Font::SEMIBOLD),
            title,
            line: Font::new(p.ui_font.clone(), size(-1.5)),
            line_strong: Font::new(p.ui_font.clone(), size(-1.5)).with_weight(Font::SEMIBOLD),
            code: Font::new(p.code_font.clone(), ((p.code_font_size - 2.0) * k).round().max(5.0)),
            pill: Font::new(p.ui_font.clone(), size(-2.5)).with_weight(Font::SEMIBOLD),
        }
    }
}

/// How a card stands out.
#[derive(Debug, Clone, Copy, Default)]
struct Look {
    selected: bool,
    hovered: bool,
    hot: bool,
    dim: bool,
    hot_spot: bool,
}

/// A piece of a card's line.
enum Piece {
    Text(String, Rgba, bool),
    Code(String),
    Pill(String, Rgba),
}

/// A step's lines (the same count for every card of the profile).
fn lines_of(report: &ProfileReport, id: NodeId, p: &Palette) -> Vec<(Vec<Piece>, Option<Piece>)> {
    let node = report.profile.node(id);
    let d = report.derived(id);
    let mut out = Vec::new();
    // What it works on.
    let mut what: Vec<Piece> = Vec::new();
    if let Some(object) = node.object.as_ref().filter(|o| o.kind != ObjectKind::Index) {
        what.push(Piece::Text(object.label(), p.text, true));
        if let Some(index) = &node.index {
            what.push(Piece::Text(index.clone(), p.text_muted, false));
        }
    } else if let Some(index) = &node.index {
        what.push(Piece::Text(index.clone(), p.text, true));
    } else if let Some(cond) = tree::condition(node) {
        what.push(Piece::Code(cond.to_owned()));
    } else if let Some(sub) = &node.subplan_name {
        what.push(Piece::Text(sub.clone(), p.text_muted, false));
    } else {
        what.push(Piece::Text(tree::op_kind_text(node.op), p.text_muted, false));
    }
    out.push((what, None));
    let measured = report.totals.measure.is_some();
    let timed = report.totals.measure == Some(database::plan::Measure::Time);
    if measured {
        let (own, all, share) = if timed {
            (d.self_ms.map(figures::ms), d.inclusive_ms.map(figures::ms), d.self_pct)
        } else {
            (
                d.self_cost.map(|c| figures::number(c.round())),
                node.estimates.total_cost.map(|c| figures::number(c.round())),
                d.cost_pct,
            )
        };
        let mut left = Vec::new();
        match own {
            Some(own) => left.push(Piece::Text(own, p.text, true)),
            None => left.push(Piece::Text("—".into(), p.text_muted, false)),
        }
        if let Some(all) = all {
            left.push(Piece::Text(format!("/ {all}"), p.text_muted, false));
        }
        let right = share.map(|s| Piece::Pill(figures::share(s), p.heat(s as f32)));
        out.push((left, right));
    }
    let has_rows =
        report.profile.nodes.iter().any(|n| n.estimates.rows.is_some() || n.actuals.rows.is_some());
    if has_rows {
        let est = node.estimates.rows.map(|r| r * node.actuals.loops.unwrap_or(1.0).max(1.0));
        let mut left = Vec::new();
        let mut right = None;
        match (est, d.total_rows) {
            (Some(est), Some(actual)) => {
                left.push(Piece::Text(figures::count(est), p.text_muted, false));
                left.push(Piece::Text("→".into(), p.text_muted, false));
                left.push(Piece::Text(figures::count(actual), p.text, true));
                let ratio = d.misestimate.unwrap_or(1.0);
                let off = if ratio >= 1.0 { ratio } else { 1.0 / ratio.max(1e-9) };
                if off >= 10.0 {
                    let l = if off >= 1000.0 { Level::Critical } else { Level::Warning };
                    right = Some(Piece::Pill(figures::factor(ratio), l.color(p)));
                }
            }
            (Some(est), None) => left.push(Piece::Text(figures::count(est), p.text, false)),
            (None, Some(actual)) => left.push(Piece::Text(figures::count(actual), p.text, true)),
            (None, None) => left.push(Piece::Text("—".into(), p.text_muted, false)),
        }
        left.insert(0, Piece::Text(i18n::t("profile.view.graph_rows"), p.text_muted, false));
        out.push((left, right));
    }
    let has_cost = report.profile.nodes.iter().any(|n| n.estimates.total_cost.is_some());
    if has_cost {
        let mut left = vec![Piece::Text(i18n::t("profile.view.graph_cost"), p.text_muted, false)];
        match node.estimates.total_cost {
            Some(c) => left.push(Piece::Text(figures::number(c.round()), p.text, false)),
            None => left.push(Piece::Text("—".into(), p.text_muted, false)),
        }
        let loops = node.actuals.loops.filter(|l| *l > 1.0);
        let right = loops.map(|l| {
            Piece::Text(
                i18n::tr!("profile.view.graph_loops", loops = figures::count(l).as_str()),
                p.text_muted,
                false,
            )
        });
        out.push((left, right));
    }
    out
}

fn paint_card(
    pen: &mut ui_components::LayerPainter<'_>,
    fonts: &Fonts,
    (r, k): (Rect, f32),
    (report, id): (&ProfileReport, NodeId),
    finding: Option<&Finding>,
    look: Look,
) {
    let (layer, text, ds) = (&mut *pen.layer, &mut *pen.text, &pen.ds);
    let icons = pen.icons;
    let p = ds.palette;
    let node = report.profile.node(id);
    let d = report.derived(id);
    let share = d.self_pct.or(d.cost_pct);
    let heat = share.map_or(p.text_muted.with_alpha(0.3), |s| p.heat(s as f32));
    let severity = finding.map(|f| level(f.severity));
    let rad = (8.0 * k).round().max(2.0);
    let opacity = if look.dim { 0.35 } else { 1.0 };
    let fade = |c: Rgba| c.with_alpha(c.a * opacity);
    if look.selected {
        ds::surface(layer, ds, diagram::inflate(r, 1.0), ds::Elevation::Medium, rad);
    }
    let (border, bw) = if look.selected {
        (p.accent, (2.0 * ds.px(1.0)).round())
    } else if let Some(l) = severity {
        (l.color(p), (1.5 * ds.px(1.0)).round().max(1.0))
    } else if look.hovered {
        (p.accent.with_alpha(0.6), ds.line())
    } else if look.hot {
        (p.accent.with_alpha(0.45), ds.line())
    } else {
        (p.card_border, ds.line())
    };
    // The heat on top: the card filled with it, its body over all but a stripe.
    layer.quad(Quad::filled(r, fade(heat)).rounded(rad).bordered(bw, fade(border)));
    let stripe = (STRIPE * k).round().max(2.0);
    let body = Rect::new(
        r.min_x() + bw,
        r.min_y() + stripe,
        r.size.width - 2.0 * bw,
        r.size.height - stripe - bw,
    );
    let fill = match severity {
        Some(l) => p.editor_background.mix(l.color(p), 0.06),
        None => p.editor_background,
    };
    layer.quad(Quad::filled(body, fade(fill)).rounded((rad - bw).max(1.0)));
    if r.size.width < 70.0 * ds.px(1.0) {
        return;
    }
    // The header: the kind's icon, the engine's words, the sign in the corner.
    let header =
        Rect::new(r.min_x(), r.min_y() + stripe, r.size.width, (HEADER_H * k).round() - stripe);
    let mid = header.min_y() + header.size.height / 2.0;
    let s = (14.0 * k).round();
    let mut x = r.min_x() + 10.0 * k;
    if let Some(icon) = icons.id(tree::op_icon(node.op)) {
        let color = if look.hot { p.accent } else { p.text_muted };
        draw_icon(
            layer,
            icons,
            icon,
            Rect::new(x.round(), (mid - s / 2.0).round(), s, s),
            fade(color),
        );
    }
    x += s + 7.0 * k;
    let mut right = r.max_x() - 10.0 * k;
    let sign = match (severity, look.hot_spot) {
        (Some(l), _) => Some((l.icon(), l.color(p))),
        (None, true) => Some(("flame", heat)),
        _ => None,
    };
    if let Some((icon, color)) = sign
        && let Some(sid) = icons.id(icon)
    {
        let ss = (15.0 * k).round();
        let sr = Rect::new((right - ss).round(), (mid - ss / 2.0).round(), ss, ss);
        draw_icon(layer, icons, sid, sr, fade(color));
        right = sr.min_x() - 6.0 * k;
    }
    let mut words = node.name.clone();
    if let Some(join) = node.join
        && join != database::profile::node::JoinKind::Inner
        && !words.contains(join.words())
    {
        words = format!("{} {words}", join.words());
    }
    let title_font = if look.hot { &fonts.title_strong } else { &fonts.title };
    let shown = ds::ellipsize(text, &words, title_font, (right - x).max(0.0));
    let shaped = text.shape_line(&shown, title_font);
    draw_text(
        layer,
        &shaped,
        x.round(),
        ds::optical_baseline(header.min_y(), header.size.height, title_font),
        fade(p.text),
    );
    if node.actuals.never_executed {
        // Never run: said in its words' place when there's room after them.
        let note = text.shape_line(&i18n::t("profile.view.never_executed"), &fonts.pill);
        if x + shaped.width + 8.0 * k + note.width < right {
            draw_text(
                layer,
                &note,
                (x + shaped.width + 8.0 * k).round(),
                ds::optical_baseline(header.min_y(), header.size.height, &fonts.pill),
                fade(p.text_muted),
            );
        }
    }
    let divider_y = (r.min_y() + HEADER_H * k).round();
    ds::divider_in(
        layer,
        &ds.with_opacity(opacity),
        (r.min_x() + bw, r.max_x() - bw, divider_y),
        p.card_border.with_alpha(CARD_RULE),
    );
    if k < 0.42 * ds.px(1.0) {
        return;
    }
    // The lines.
    let mut y = divider_y + 4.0 * k;
    let lh = LINE_H * k;
    for (left, right_piece) in lines_of(report, id, p) {
        let base = ds::optical_baseline(y, lh, &fonts.line);
        let mut lx = r.min_x() + 12.0 * k;
        let mut end = r.max_x() - 12.0 * k;
        if let Some(piece) = right_piece {
            match piece {
                Piece::Pill(label, color) => {
                    // Zoomed with the card (its `k` over the screen's scale), dimmed with it.
                    let pr = ds::Badge::new(&label)
                        .color(readable_on(color, p))
                        .fill(color.with_alpha(CARD_PILL_FILL))
                        .font(&fonts.pill)
                        .size(ds::BadgeSize::Custom(CARD_PILL_H))
                        .pad(PILL_PAD)
                        .zoom(k / ds.scale)
                        .paint_right(layer, text, &ds.with_opacity(opacity), end, y + lh / 2.0);
                    end = pr.min_x() - 6.0 * k;
                }
                Piece::Text(label, color, _) => {
                    let shaped = text.shape_line(&label, &fonts.line);
                    draw_text(layer, &shaped, (end - shaped.width).round(), base, fade(color));
                    end -= shaped.width + 6.0 * k;
                }
                Piece::Code(_) => {}
            }
        }
        for (i, piece) in left.into_iter().enumerate() {
            if i > 0 {
                lx += 5.0 * k;
            }
            let room = end - lx;
            if room < 12.0 * k {
                break;
            }
            let (label, font, color) = match piece {
                Piece::Text(label, color, strong) => {
                    (label, if strong { &fonts.line_strong } else { &fonts.line }, color)
                }
                Piece::Code(label) => (label, &fonts.code, p.text_muted),
                Piece::Pill(label, color) => (label, &fonts.pill, color),
            };
            let shown = ds::ellipsize(text, &label, font, room);
            let shaped = text.shape_line(&shown, font);
            draw_text(layer, &shaped, lx.round(), base, fade(color));
            lx += shaped.width;
        }
        y += lh;
    }
}

/// A pill's words on a tint of `color`: the color itself, deepened on a light theme.
fn readable_on(color: Rgba, p: &Palette) -> Rgba {
    if p.is_dark() {
        color.mix(Rgba::rgb(0xffffff), 0.15)
    } else {
        color.mix(Rgba::rgb(0x000000), 0.25)
    }
}

fn paint_folded(
    pen: &mut ui_components::LayerPainter<'_>,
    fonts: &Fonts,
    (r, k): (Rect, f32),
    (report, under): (&ProfileReport, NodeId),
    count: usize,
    hovered: bool,
) {
    let (layer, text, ds) = (&mut *pen.layer, &mut *pen.text, &pen.ds);
    let p = ds.palette;
    let rad = r.size.height / 2.0;
    let border = if hovered { p.accent.with_alpha(0.7) } else { p.card_border };
    layer.quad(
        Quad::filled(r, p.text.with_alpha(if hovered { 0.07 } else { 0.035 }))
            .rounded(rad)
            .bordered(ds.line(), border),
    );
    let total = report.totals.time_ms.or(report.totals.cost).unwrap_or(0.0);
    let d = report.derived(under);
    let measure_time = report.totals.measure == Some(database::plan::Measure::Time);
    let weight =
        if measure_time { d.inclusive_ms } else { report.profile.node(under).estimates.total_cost };
    let label = match weight.filter(|_| total > 0.0) {
        Some(w) => i18n::tr!(
            "profile.view.folded_share",
            count = count as i64,
            share = figures::share((w / total).min(1.0)).as_str()
        ),
        None => i18n::tr!("profile.view.folded", count = count as i64),
    };
    let shown = ds::ellipsize(text, &label, &fonts.line, r.size.width - 20.0 * k);
    let shaped = text.shape_line(&shown, &fonts.line);
    draw_text(
        layer,
        &shaped,
        (r.min_x() + (r.size.width - shaped.width) / 2.0).round(),
        ds::optical_baseline(r.min_y(), r.size.height, &fonts.line),
        p.text_muted,
    );
}

/// A card's tooltip: the step's words and kind, its exact figures, its finding.
pub fn card_tip(report: &ProfileReport, id: NodeId, finding: Option<&Finding>) -> String {
    let node = report.profile.node(id);
    let d = report.derived(id);
    let mut lines = vec![export::head(node), tree::op_kind_text(node.op)];
    if let (Some(own), Some(share)) = (d.self_ms, d.self_pct) {
        lines.push(i18n::tr!(
            "profile.view.self_tip",
            own = figures::ms_exact(own).as_str(),
            share = figures::share_exact(share).as_str(),
            total = figures::ms_exact(report.totals.time_ms.unwrap_or(0.0)).as_str()
        ));
    } else if let (Some(own), Some(share)) = (d.self_cost, d.cost_pct) {
        lines.push(i18n::tr!(
            "profile.view.self_cost_tip",
            own = figures::number(own).as_str(),
            share = figures::share_exact(share).as_str()
        ));
    }
    if let Some(total) = d.inclusive_ms {
        let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
        lines.push(if loops > 1.0 {
            i18n::tr!(
                "profile.view.total_tip",
                total = figures::ms_exact(total).as_str(),
                per = figures::ms_exact(node.actuals.total_ms.unwrap_or(0.0)).as_str(),
                loops = figures::count(loops).as_str()
            )
        } else {
            format!("{}: {}", i18n::t("profile.view.col.total_time"), figures::ms_exact(total))
        });
    }
    let est = node.estimates.rows.map(|r| r * node.actuals.loops.unwrap_or(1.0).max(1.0));
    match (est, d.total_rows) {
        (Some(est), Some(actual)) => lines.push(i18n::tr!(
            "profile.view.rows_tip",
            expected = figures::count(est).as_str(),
            actual = figures::count(actual).as_str(),
            factor = figures::factor(d.misestimate.unwrap_or(1.0)).as_str()
        )),
        (Some(est), None) => lines
            .push(i18n::tr!("profile.view.rows_est_tip", expected = figures::count(est).as_str())),
        _ => {}
    }
    if let Some(total) = node.estimates.total_cost {
        lines.push(i18n::tr!(
            "profile.view.cost_tip",
            startup = figures::number(node.estimates.startup_cost.unwrap_or(0.0)).as_str(),
            total = figures::number(total).as_str()
        ));
    }
    if d.approx {
        lines.push(i18n::t("profile.view.approx"));
    }
    if let Some(f) = finding {
        let t = analysis::texts(f);
        lines.push(format!("{} · {}", analysis::severity(f.severity), t.title));
    }
    lines.join("\n")
}

/// The edge `e` points into its parent (for tests: an arrow toward the roots).
pub fn points_to_parent(e: &Edge<(usize, usize)>, flow: Flow) -> bool {
    matches!((flow, e.dir), (Flow::Down, Dir::Up) | (Flow::Right, Dir::Left))
}

#[cfg(test)]
#[path = "../../tests/unit/profile/graph.rs"]
mod tests;
