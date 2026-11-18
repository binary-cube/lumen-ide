//! Where the tables' cards and the links' lines go, in diagram space — computed in the
//! background, once for each set of tables shown. Tables that point to others stand right
//! of them: each table's column is one past the farthest it points to (a cycle broken where
//! it's met), the tables of a column ordered after those they point to (fewer crossings),
//! a column too tall continued beside it; the tables linked to nothing are a grid under
//! the rest. A line leaves the row of its column on the pointing card's side and enters
//! the row of the referenced column on the other card's, axis-aligned, the lines between
//! two columns spread across the gap between them.

use std::collections::BTreeMap;

use database::diagram::SchemaGraph;
use kernel::{Point, Rect};
use ui_components::diagram::{self, Card, Dir, HEADER_H, ROW_H, card_height};

/// A card's width.
pub const CARD_W: f32 = 250.0;
/// Between two columns of linked tables (their lines run there), between the cards of a
/// column, between a column and its continuation.
const GAP_X: f32 = 190.0;
const GAP_Y: f32 = 34.0;
const GAP_WRAP: f32 = 60.0;
/// The columns listed on a card: the table the diagram is about lists more.
const ROWS: usize = 10;
const FOCUS_ROWS: usize = 18;
/// Where a line leaves or loops beside a card.
const LOOP: f32 = 22.0;

/// A diagram's layout: its cards (one per table shown, `node` its index in the graph),
/// its edges (`kind` the link's index), and each card's columns listed (indexes in its
/// structure, in its order).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiagramLayout {
    pub layout: diagram::Layout<usize>,
    pub rows: Vec<Vec<usize>>,
}

impl DiagramLayout {
    /// The card showing table `table` (its place among the cards).
    pub fn card_of(&self, table: usize) -> Option<usize> {
        self.layout.cards.iter().position(|c| c.node == table)
    }

    /// Where the row of `table`'s column `column` is (its middle), in diagram space; its
    /// header's middle when it isn't listed.
    pub fn row_y(&self, card: usize, column: usize) -> f32 {
        let c = &self.layout.cards[card];
        match self.rows[card].iter().position(|&i| i == column) {
            Some(at) => c.rect.min_y() + HEADER_H + 4.0 + at as f32 * ROW_H + ROW_H / 2.0,
            None => c.rect.min_y() + HEADER_H / 2.0,
        }
    }
}

/// What's shown: the tables (indexes in the graph), the links (indexes), the table the
/// diagram is about (if any).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Shown {
    pub tables: Vec<usize>,
    pub links: Vec<usize>,
    pub focus: Option<usize>,
}

/// The columns a card lists: its key's and its links', then the rest, at most `limit` (in
/// the table's order); and how many more there are.
fn listed(graph: &SchemaGraph, table: usize, links: &[usize], limit: usize) -> (Vec<usize>, usize) {
    let s = &graph.tables[table].structure;
    let mut wanted = vec![false; s.columns.len()];
    for (i, c) in s.columns.iter().enumerate() {
        wanted[i] = c.in_key;
    }
    for &l in links {
        let link = &graph.links[l];
        let names = match (link.from == table, link.to == table) {
            (true, true) => [&link.columns[..], &link.references[..]].concat(),
            (true, false) => link.columns.clone(),
            (false, true) => link.references.clone(),
            _ => continue,
        };
        for name in names {
            if let Some(i) = s.columns.iter().position(|c| c.name == name) {
                wanted[i] = true;
            }
        }
    }
    let mut chosen: Vec<usize> = (0..s.columns.len()).filter(|&i| wanted[i]).take(limit).collect();
    let rest: Vec<usize> = (0..s.columns.len()).filter(|&i| !wanted[i]).collect();
    let room = limit.saturating_sub(chosen.len());
    chosen.extend(rest.into_iter().take(room));
    chosen.sort_unstable();
    let more = s.columns.len() - chosen.len();
    (chosen, more)
}

/// Each table's column: one past the farthest table it points to (a link back to a table
/// still being placed is a cycle: left out of the ranking). Tables linked to nothing: none.
fn ranks(n: usize, parents: &[Vec<usize>], linked: &[bool]) -> Vec<Option<usize>> {
    let mut rank: Vec<Option<usize>> = vec![None; n];
    let mut state = vec![0u8; n]; // 0: not seen, 1: being placed, 2: placed
    for start in 0..n {
        if !linked[start] || state[start] != 0 {
            continue;
        }
        // Depth first without recursion (a long chain of tables mustn't overflow).
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        state[start] = 1;
        while let Some(&mut (t, ref mut next)) = stack.last_mut() {
            if let Some(&p) = parents[t].get(*next) {
                *next += 1;
                if state[p] == 0 {
                    state[p] = 1;
                    stack.push((p, 0));
                }
                continue;
            }
            let r = parents[t]
                .iter()
                .filter(|&&p| state[p] == 2)
                .filter_map(|&p| rank[p])
                .map(|r| r + 1)
                .max()
                .unwrap_or(0);
            rank[t] = Some(r);
            state[t] = 2;
            stack.pop();
        }
    }
    rank
}

/// Orders the tables of `columns` (those of `range`, in its order) by the mean place of
/// those they're linked to `by`, their own place otherwise; their places follow.
fn order(
    columns: &mut [Vec<usize>],
    place: &mut [f32],
    by: &[Vec<usize>],
    range: impl Iterator<Item = usize>,
) {
    for r in range {
        let mut keyed: Vec<(f32, usize)> = columns[r]
            .iter()
            .map(|&t| {
                let around = &by[t];
                let key = if around.is_empty() {
                    place[t]
                } else {
                    around.iter().map(|&o| place[o]).sum::<f32>() / around.len() as f32
                };
                (key, t)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        columns[r] = keyed.into_iter().map(|(_, t)| t).collect();
        let len = columns[r].len().max(1) as f32;
        for (i, &t) in columns[r].iter().enumerate() {
            place[t] = i as f32 / len;
        }
    }
}

/// Lays out `shown` of `graph` (see the module's doc).
pub fn lay_out(graph: &SchemaGraph, shown: &Shown) -> DiagramLayout {
    let n = shown.tables.len();
    let local: BTreeMap<usize, usize> =
        shown.tables.iter().enumerate().map(|(i, &t)| (t, i)).collect();
    // Who points to whom, among those shown (a table to itself left out).
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut linked = vec![false; n];
    for &l in &shown.links {
        let link = &graph.links[l];
        let (Some(&c), Some(&p)) = (local.get(&link.from), local.get(&link.to)) else { continue };
        if c != p {
            parents[c].push(p);
            children[p].push(c);
            linked[c] = true;
            linked[p] = true;
        }
    }
    for list in parents.iter_mut().chain(children.iter_mut()) {
        list.sort_unstable();
        list.dedup();
    }
    let rank = ranks(n, &parents, &linked);

    // The columns, each table's place in its own.
    let depth = rank.iter().flatten().max().map_or(0, |r| r + 1);
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for (t, r) in rank.iter().enumerate() {
        if let Some(r) = r {
            columns[*r].push(t);
        }
    }
    let mut place = vec![0.0f32; n];
    for column in &columns {
        let len = column.len().max(1) as f32;
        for (i, &t) in column.iter().enumerate() {
            place[t] = i as f32 / len;
        }
    }
    // Down the columns by what they point to, then back by what points to them.
    order(&mut columns, &mut place, &parents, 1..depth);
    order(&mut columns, &mut place, &children, (0..depth.saturating_sub(1)).rev());
    order(&mut columns, &mut place, &parents, 1..depth);

    // The cards: each column stacked (continued beside it past `per`), centered on 0.
    let focus = shown.focus.and_then(|f| local.get(&f).copied());
    let lists: Vec<(Vec<usize>, usize)> = shown
        .tables
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            listed(graph, t, &shown.links, if Some(i) == focus { FOCUS_ROWS } else { ROWS })
        })
        .collect();
    let height = |t: usize| card_height(lists[t].0.len(), lists[t].1);
    let per = ((n as f32).sqrt() * 0.9).ceil().max(6.0) as usize;
    let mut rects: Vec<Option<Rect>> = vec![None; n];
    let mut x = 0.0;
    for column in &columns {
        for (w, chunk) in column.chunks(per).enumerate() {
            if w > 0 {
                x += CARD_W + GAP_WRAP;
            }
            let total: f32 = chunk.iter().map(|&t| height(t)).sum::<f32>()
                + GAP_Y * chunk.len().saturating_sub(1) as f32;
            let mut y = -total / 2.0;
            for &t in chunk {
                rects[t] = Some(Rect::new(x, y.round(), CARD_W, height(t)));
                y += height(t) + GAP_Y;
            }
        }
        x += CARD_W + GAP_X;
    }
    // The tables linked to nothing: a grid under the rest.
    let alone: Vec<usize> = (0..n).filter(|&t| rank[t].is_none()).collect();
    if !alone.is_empty() {
        let placed = diagram::bounds_of(rects.iter().flatten().copied());
        let top =
            if rects.iter().any(Option::is_some) { placed.max_y() + GAP_Y * 3.0 } else { 0.0 };
        let left = if rects.iter().any(Option::is_some) { placed.min_x() } else { 0.0 };
        let across = ((alone.len() as f32).sqrt().ceil() as usize).max(4).min(alone.len());
        let mut y = top;
        for row in alone.chunks(across) {
            let tallest = row.iter().map(|&t| height(t)).fold(0.0f32, f32::max);
            for (i, &t) in row.iter().enumerate() {
                let x = left + i as f32 * (CARD_W + GAP_WRAP);
                rects[t] = Some(Rect::new(x, y, CARD_W, height(t)));
            }
            y += tallest + GAP_Y;
        }
    }

    let mut cards = Vec::with_capacity(n);
    let mut rows = Vec::with_capacity(n);
    for (i, &t) in shown.tables.iter().enumerate() {
        let Some(rect) = rects[i] else { continue };
        let (list, more) = lists[i].clone();
        cards.push(Card { node: t, rect, shown: list.len(), more });
        rows.push(list);
    }
    let mut out = DiagramLayout { layout: diagram::Layout::new(cards, Vec::new()), rows };
    out.layout.edges = edges(graph, shown, &out);
    out
}

/// The links' lines (see the module's doc).
fn edges(graph: &SchemaGraph, shown: &Shown, out: &DiagramLayout) -> Vec<diagram::Edge<usize>> {
    let card_of: BTreeMap<usize, usize> =
        out.layout.cards.iter().enumerate().map(|(i, c)| (c.node, i)).collect();
    let column = |table: usize, name: &str| {
        graph.tables[table]
            .structure
            .columns
            .iter()
            .position(|c| c.name == name)
            .unwrap_or(usize::MAX)
    };
    // Each line's ends first, then the lanes it gets in its gap.
    struct Draft {
        link: usize,
        from: Rect,
        to: Rect,
        y0: f32,
        y1: f32,
    }
    let mut drafts = Vec::new();
    for &l in &shown.links {
        let link = &graph.links[l];
        let (Some(&a), Some(&b)) = (card_of.get(&link.from), card_of.get(&link.to)) else {
            continue;
        };
        let first = |t: usize, names: &[String]| names.first().map_or(usize::MAX, |n| column(t, n));
        let y0 = out.row_y(a, first(link.from, &link.columns));
        let y1 = out.row_y(b, first(link.to, &link.references));
        drafts.push(Draft {
            link: l,
            from: out.layout.cards[a].rect,
            to: out.layout.cards[b].rect,
            y0,
            y1,
        });
    }
    // The lines sharing a gap (between the same two x's) spread across it, ordered by
    // where they end (fewer crossings).
    let mut gaps: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    let mut loops: BTreeMap<i64, usize> = BTreeMap::new();
    for (i, d) in drafts.iter().enumerate() {
        if d.to.max_x() < d.from.min_x() {
            gaps.entry((d.to.max_x() as i64, d.from.min_x() as i64)).or_default().push(i);
        } else if d.from.max_x() < d.to.min_x() {
            gaps.entry((d.from.max_x() as i64, d.to.min_x() as i64)).or_default().push(i);
        }
    }
    let mut lane = vec![(0usize, 1usize); drafts.len()];
    for members in gaps.values_mut() {
        members.sort_by(|&a, &b| {
            let (da, db) = (&drafts[a], &drafts[b]);
            da.y1.total_cmp(&db.y1).then(da.y0.total_cmp(&db.y0)).then(da.link.cmp(&db.link))
        });
        let count = members.len();
        for (k, &i) in members.iter().enumerate() {
            lane[i] = (k, count);
        }
    }
    let mut edges = Vec::with_capacity(drafts.len());
    for (i, d) in drafts.iter().enumerate() {
        let (k, count) = lane[i];
        let spread = |x0: f32, x1: f32| {
            let t = if count <= 1 { 0.5 } else { 0.2 + 0.6 * k as f32 / (count - 1) as f32 };
            (x0 + (x1 - x0) * t).round()
        };
        let (points, dir) = if d.to.max_x() < d.from.min_x() {
            // The usual way: left, into the referenced table's right side.
            let mid = spread(d.to.max_x(), d.from.min_x());
            (
                vec![
                    Point::new(d.from.min_x(), d.y0),
                    Point::new(mid, d.y0),
                    Point::new(mid, d.y1),
                    Point::new(d.to.max_x(), d.y1),
                ],
                Dir::Left,
            )
        } else if d.from.max_x() < d.to.min_x() {
            // Back to the right (a cycle).
            let mid = spread(d.from.max_x(), d.to.min_x());
            (
                vec![
                    Point::new(d.from.max_x(), d.y0),
                    Point::new(mid, d.y0),
                    Point::new(mid, d.y1),
                    Point::new(d.to.min_x(), d.y1),
                ],
                Dir::Right,
            )
        } else {
            // The same column (or the same table): around their right side.
            let right = d.from.max_x().max(d.to.max_x());
            let n = loops.entry(right as i64).or_insert(0);
            let x = right + LOOP + *n as f32 * 8.0;
            *n += 1;
            let y1 = if (d.y1 - d.y0).abs() < 1.0 { d.y1 + ROW_H / 3.0 } else { d.y1 };
            (
                vec![
                    Point::new(d.from.max_x(), d.y0),
                    Point::new(x, d.y0),
                    Point::new(x, y1),
                    Point::new(d.to.max_x(), y1),
                ],
                Dir::Left,
            )
        };
        edges.push(diagram::Edge { points, dir, kind: d.link });
    }
    edges
}

#[cfg(test)]
#[path = "../tests/unit/layout.rs"]
mod tests;
