//! Where a diagram's boxes and edges go, in diagram space (logical pixels, before pan and
//! zoom) — computed once per diagram: the ancestors in rows above (the farthest on top), the
//! type in the middle, its children in rows below, its constructor's dependencies in a
//! column on its right. Edges are axis-aligned polylines (cheap to draw, easy to follow).

use code_intelligence::{ClassDiagram, DiagramRole, Relation};
use kernel::{Point, Rect};
use ui_components::diagram::{self, Card, Dir, HEADER_H, card_height};

pub const CARD_W: f32 = 230.0;
const GAP_X: f32 = 36.0;
const GAP_Y: f32 = 84.0;
/// Boxes per row before wrapping.
const PER_ROW: usize = 5;
/// Members shown: the type the diagram is about shows more.
const FOCUS_MEMBERS: usize = 14;
const MEMBERS: usize = 6;
/// The dependencies' column, right of the focus.
const DEPS_GAP: f32 = 130.0;

/// The diagram's edges are its relations.
pub type Edge = diagram::Edge<Relation>;

/// Where the boxes and edges go.
#[derive(Debug, Clone, Default)]
pub struct Layout(pub diagram::Layout<Relation>);

impl std::ops::Deref for Layout {
    type Target = diagram::Layout<Relation>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Layout {
    pub fn new(diagram: &ClassDiagram) -> Self {
        let mut cards: Vec<Option<Card>> = vec![None; diagram.nodes.len()];
        // The rows, top to bottom, and each box's row (its top and bottom).
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let depth = diagram
            .nodes
            .iter()
            .filter_map(|n| match n.role {
                DiagramRole::Ancestor(d) => Some(d),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let with_role = |role: DiagramRole| -> Vec<usize> {
            (0..diagram.nodes.len()).filter(|&i| diagram.nodes[i].role == role).collect()
        };
        for d in (1..=depth).rev() {
            rows.extend(with_role(DiagramRole::Ancestor(d)).chunks(PER_ROW).map(<[usize]>::to_vec));
        }
        let focus_row = rows.len();
        rows.push(vec![0]);
        rows.extend(with_role(DiagramRole::Child).chunks(PER_ROW).map(<[usize]>::to_vec));

        let size = |i: usize| {
            let limit = if i == 0 { FOCUS_MEMBERS } else { MEMBERS };
            let total = diagram.nodes[i].members.len();
            let shown = total.min(limit);
            (shown, total - shown)
        };
        let mut row_span: Vec<(f32, f32)> = Vec::new();
        let mut y = 0.0;
        for row in &rows {
            let height = row
                .iter()
                .map(|&i| {
                    let (shown, more) = size(i);
                    card_height(shown, more)
                })
                .fold(0.0f32, f32::max);
            let total = row.len() as f32 * CARD_W + (row.len() as f32 - 1.0) * GAP_X;
            let mut x = -total / 2.0;
            for &i in row {
                let (shown, more) = size(i);
                let rect = Rect::new(x, y, CARD_W, card_height(shown, more));
                cards[i] = Some(Card { node: i, rect, shown, more });
                x += CARD_W + GAP_X;
            }
            row_span.push((y, y + height));
            y += height + GAP_Y;
        }
        let row_of = |i: usize| rows.iter().position(|r| r.contains(&i));

        // The dependencies, stacked on the focus's right.
        let focus = cards[0].as_ref().map_or(Rect::default(), |c| c.rect);
        let mut dy = focus.min_y();
        for i in with_role(DiagramRole::Dependency) {
            let (shown, more) = size(i);
            let rect = Rect::new(focus.max_x() + DEPS_GAP, dy, CARD_W, card_height(shown, more));
            dy += rect.size.height + 16.0;
            cards[i] = Some(Card { node: i, rect, shown, more });
        }

        let mut edges = Vec::new();
        for e in &diagram.edges {
            let (Some(from), Some(to)) = (&cards[e.from], &cards[e.to]) else { continue };
            let (a, b) = (from.rect, to.rect);
            if e.relation == Relation::Depends {
                let (y0, y1) = (a.min_y() + HEADER_H / 2.0, b.min_y() + HEADER_H / 2.0);
                let mid = a.max_x() + DEPS_GAP / 2.0;
                edges.push(Edge {
                    points: vec![
                        Point::new(a.max_x(), y0),
                        Point::new(mid, y0),
                        Point::new(mid, y1),
                        Point::new(b.min_x(), y1),
                    ],
                    dir: Dir::Right,
                    kind: e.relation,
                });
                continue;
            }
            let (cx_a, cx_b) = (a.min_x() + a.size.width / 2.0, b.min_x() + b.size.width / 2.0);
            let (ra, rb) = (row_of(e.from).unwrap_or(focus_row), row_of(e.to).unwrap_or(focus_row));
            let (points, dir) = if rb < ra {
                // The target is above: up from the source, across under the target's row.
                let mid = row_span[rb].1 + GAP_Y / 2.0;
                let points = vec![
                    Point::new(cx_a, a.min_y()),
                    Point::new(cx_a, mid),
                    Point::new(cx_b, mid),
                    Point::new(cx_b, b.max_y()),
                ];
                (points, Dir::Up)
            } else {
                // Same row (or below): over the row, then down into the target.
                let top = row_span[ra].0.min(row_span[rb].0) - GAP_Y / 3.0;
                let points = vec![
                    Point::new(cx_a, a.min_y()),
                    Point::new(cx_a, top),
                    Point::new(cx_b, top),
                    Point::new(cx_b, b.min_y()),
                ];
                (points, Dir::Down)
            };
            edges.push(Edge { points, dir, kind: e.relation });
        }

        let cards: Vec<Card> = cards.into_iter().flatten().collect();
        Self(diagram::Layout::new(cards, edges))
    }
}

#[cfg(test)]
#[path = "../tests/unit/layout.rs"]
mod tests;
