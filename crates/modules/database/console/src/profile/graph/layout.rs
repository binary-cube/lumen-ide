//! The diagram's layout, worked out once per profile, what's open and the flow: a card
//! per step (its lines, its height), the folded ones, the edges as thick as their rows.

use super::*;

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

/// The edge `e` points into its parent (for tests: an arrow toward the roots).
pub fn points_to_parent(e: &Edge<(usize, usize)>, flow: Flow) -> bool {
    matches!((flow, e.dir), (Flow::Down, Dir::Up) | (Flow::Right, Dir::Left))
}
