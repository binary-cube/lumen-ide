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

mod card;
mod input;
mod layout;
mod paint;

pub use card::{Fonts, card_tip};
pub use layout::{
    Card, EdgeInfo, GraphLayout, Item, card_h, edge_width, layout, line_count, points_to_parent,
    rows_out,
};

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
}

/// `outer` holds `inner` whole.
fn rect_holds(outer: Rect, inner: Rect) -> bool {
    diagram::contains_rect(outer, inner)
}

/// The step the hot-spot finding is about.
fn hot_spot(findings: &[&Finding]) -> Option<NodeId> {
    findings.iter().find(|f| !f.is_problem()).and_then(|f| f.nodes.first().copied())
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/graph.rs"]
mod tests;
