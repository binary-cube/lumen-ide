//! A graph laid out in layers (Sugiyama's way), for the diagrams whose boxes point to one
//! another in any pattern (a flowchart, a state machine, classes and tables): the cycles
//! broken (a back edge reversed for the layout only), every node a level (longest path,
//! sources pulled down to what they feed), the long edges through a hidden node per level
//! crossed (so they bend around the boxes, and a label sits on one of them), the order in
//! each level found by medians (fewer crossings, the best of several sweeps kept), the
//! positions across a level by an ordered least-squares fit to the neighbors (isotonic
//! regression: boxes never overlap, edges as straight as the order allows).
//!
//! Everything is in "down" coordinates (levels top to bottom) and turned at the end for
//! the other directions.

use kernel::{Point, Rect};

/// Which way the levels go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[default]
    Down,
    Up,
    Right,
    Left,
}

impl Direction {
    fn across(self) -> bool {
        matches!(self, Self::Right | Self::Left)
    }
}

/// An edge to lay out: from node to node, spanning at least `min_len` levels, with a label
/// of this size (diagram pixels) to place on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSpec {
    pub from: usize,
    pub to: usize,
    pub min_len: usize,
    pub label: Option<(f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub direction: Direction,
    /// Between two boxes of a level.
    pub node_gap: f32,
    /// Between two levels.
    pub rank_gap: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { direction: Direction::Down, node_gap: 40.0, rank_gap: 50.0 }
    }
}

/// An edge laid out: the points it goes through, from its source's center to its
/// target's (the hidden nodes' centers between), and its label's place.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedEdge {
    pub points: Vec<Point>,
    pub label: Option<Rect>,
}

/// The layout: each node's box, each edge's points, the whole size (from 0, 0).
#[derive(Debug, Clone, PartialEq)]
pub struct Layered {
    pub nodes: Vec<Rect>,
    pub edges: Vec<PlacedEdge>,
    pub width: f32,
    pub height: f32,
    /// Each node's level (0 first).
    pub ranks: Vec<usize>,
}

/// An edge as laid out: from, to (turned if it went back), its levels, its label's size,
/// whether it was turned.
type Forward = (usize, usize, usize, Option<(f32, f32)>, bool);

/// An edge's chain of vertices, its label's vertex, whether it was turned.
type Chain = Option<(Vec<usize>, Option<usize>, bool)>;

/// A node of the layered graph: a real one (its index) or a hidden one on an edge.
#[derive(Debug, Clone, Copy)]
struct Vertex {
    /// Across, along (the levels' axis).
    w: f32,
    h: f32,
    rank: usize,
    /// The real node, or `None` for a hidden one.
    node: Option<usize>,
}

/// Lays out `sizes` (each node's width and height) and `edges`.
pub fn layout(sizes: &[(f32, f32)], edges: &[EdgeSpec], options: Options) -> Layered {
    let n = sizes.len();
    let across = options.direction.across();
    // A node with an edge to itself keeps room on its right for the loop and its label (else
    // its neighbor stands under them).
    let mut room = vec![0.0f32; n];
    for e in edges.iter().filter(|e| e.from == e.to && e.from < n) {
        let label = e.label.map_or(0.0, |(w, _)| w + LOOP_LABEL_GAP);
        room[e.from] = room[e.from].max(LOOP_OUT + label + LOOP_MARGIN);
    }
    // Sizes in "down" coordinates: across the level, along it (the room on the screen's
    // right: across when the levels go down, along when they go sideways).
    let dims: Vec<(f32, f32)> = sizes
        .iter()
        .zip(&room)
        .map(|(&(w, h), &extra)| if across { (h, w + extra) } else { (w + extra, h) })
        .collect();
    let labeled = edges.iter().any(|e| e.label.is_some());
    // With labels, every edge spans twice the levels: its label sits on the level between.
    let factor = if labeled { 2 } else { 1 };

    // 1. Cycles broken: an edge back to a node on the walk's path is turned around.
    let mut forward: Vec<Forward> = Vec::new();
    let reversed = back_edges(n, edges);
    for (i, e) in edges.iter().enumerate() {
        if e.from == e.to {
            continue;
        }
        let (a, b) = if reversed[i] { (e.to, e.from) } else { (e.from, e.to) };
        forward.push((a, b, e.min_len.max(1) * factor, e.label, reversed[i]));
    }

    // 2. Levels: the longest path from the sources, then sources pulled down.
    let ranks = rank(n, &forward);

    // 3. Hidden nodes on the long edges (one per level crossed).
    let mut vertices: Vec<Vertex> = (0..n)
        .map(|i| Vertex { w: dims[i].0, h: dims[i].1, rank: ranks[i], node: Some(i) })
        .collect();
    // Each edge (in `edges`' order) as its chain of vertices, and where its label is.
    let mut chains: Vec<Chain> = vec![None; edges.len()];
    let mut links: Vec<(usize, usize)> = Vec::new();
    let mut f = 0;
    for (i, e) in edges.iter().enumerate() {
        if e.from == e.to {
            continue;
        }
        let (a, b, _, label, rev) = forward[f];
        f += 1;
        let (ra, rb) = (ranks[a], ranks[b]);
        let mut chain = vec![a];
        let mut label_vertex = None;
        let middle = ra + (rb - ra) / 2;
        for r in ra + 1..rb {
            let (w, h) = match label {
                Some((lw, lh)) if r == middle => {
                    label_vertex = Some(vertices.len());
                    if across { (lh, lw) } else { (lw, lh) }
                }
                _ => (0.0, 0.0),
            };
            chain.push(vertices.len());
            vertices.push(Vertex { w, h, rank: r, node: None });
        }
        chain.push(b);
        for pair in chain.windows(2) {
            links.push((pair[0], pair[1]));
        }
        chains[i] = Some((chain, label_vertex, rev));
    }

    // 4. The order within each level.
    let levels = order(&vertices, &links);

    // 5. Positions: along (level by level), then across.
    let rank_count = levels.len();
    let mut tops = vec![0.0f32; rank_count];
    let mut heights = vec![0.0f32; rank_count];
    for (r, level) in levels.iter().enumerate() {
        heights[r] = level.iter().map(|&v| vertices[v].h).fold(0.0, f32::max);
    }
    let mut y = 0.0;
    for r in 0..rank_count {
        tops[r] = y;
        // An empty level (labels' between) takes less room.
        let gap = if labeled { options.rank_gap * 0.5 } else { options.rank_gap };
        y += heights[r] + gap;
    }
    let total_along =
        (y - if labeled { options.rank_gap * 0.5 } else { options.rank_gap }).max(0.0);
    let xs = across_positions(&vertices, &levels, &links, options.node_gap);

    let center = |v: usize| -> Point {
        let r = vertices[v].rank;
        Point::new(xs[v], tops[r] + heights[r] / 2.0)
    };
    let min_x =
        (0..vertices.len()).map(|v| xs[v] - vertices[v].w / 2.0).fold(f32::INFINITY, f32::min);
    let max_x =
        (0..vertices.len()).map(|v| xs[v] + vertices[v].w / 2.0).fold(f32::NEG_INFINITY, f32::max);
    let (min_x, max_x) = if min_x.is_finite() { (min_x, max_x) } else { (0.0, 0.0) };
    let shift = -min_x;
    let width_down = max_x - min_x;

    // In "down" space, then turned.
    let turn = |p: Point| -> Point {
        let p = Point::new(p.x + shift, p.y);
        match options.direction {
            Direction::Down => p,
            Direction::Up => Point::new(p.x, total_along - p.y),
            Direction::Right => Point::new(p.y, p.x),
            Direction::Left => Point::new(total_along - p.y, p.x),
        }
    };
    let rect_of = |v: usize| -> Rect {
        let c = turn(center(v));
        let (w, h) =
            if across { (vertices[v].h, vertices[v].w) } else { (vertices[v].w, vertices[v].h) };
        Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h)
    };
    // Each node at its own size, at the left of its place (its loop's room on its right).
    let nodes: Vec<Rect> = (0..n)
        .map(|i| {
            let r = rect_of(i);
            Rect::new(r.min_x(), r.min_y(), (r.size.width - room[i]).max(0.0), r.size.height)
        })
        .collect();
    let placed_edges = edges
        .iter()
        .enumerate()
        .map(|(i, e)| match &chains[i] {
            Some((chain, label, rev)) => {
                let mut points: Vec<Point> = chain.iter().map(|&v| turn(center(v))).collect();
                if *rev {
                    points.reverse();
                }
                PlacedEdge { points, label: label.map(rect_of) }
            }
            None => self_loop(nodes[e.from], e.label),
        })
        .collect();
    let (width, height) =
        if across { (total_along, width_down) } else { (width_down, total_along) };
    Layered { nodes, edges: placed_edges, width, height, ranks }
}

/// How far a node's loop to itself goes out of its right side, the gap before its label,
/// and the margin after it (logical pixels of the diagram).
const LOOP_OUT: f32 = 24.0;
const LOOP_LABEL_GAP: f32 = 4.0;
const LOOP_MARGIN: f32 = 8.0;

/// A node's edge to itself: a loop out of its right side and back (in the room the layout
/// kept there).
fn self_loop(r: Rect, label: Option<(f32, f32)>) -> PlacedEdge {
    let out = LOOP_OUT;
    let (y0, y1) = (r.min_y() + r.size.height * 0.3, r.min_y() + r.size.height * 0.7);
    let x = r.max_x() + out;
    let points = vec![
        Point::new(r.max_x(), y0),
        Point::new(x, y0 - 6.0),
        Point::new(x, y1 + 6.0),
        Point::new(r.max_x(), y1),
    ];
    let label = label.map(|(w, h)| Rect::new(x + LOOP_LABEL_GAP, r.center().y - h / 2.0, w, h));
    PlacedEdge { points, label }
}

/// Which edges are turned around to break the cycles (a depth-first walk in the nodes'
/// order: an edge to a node still on its path goes back).
fn back_edges(n: usize, edges: &[EdgeSpec]) -> Vec<bool> {
    let mut out_edges = vec![Vec::new(); n];
    for (i, e) in edges.iter().enumerate() {
        if e.from != e.to && e.from < n && e.to < n {
            out_edges[e.from].push(i);
        }
    }
    let mut state = vec![0u8; n]; // 0 new, 1 on the path, 2 done
    let mut reversed = vec![false; edges.len()];
    for start in 0..n {
        if state[start] != 0 {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        state[start] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&e) = out_edges[v].get(*next) {
                *next += 1;
                let w = edges[e].to;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => reversed[e] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    reversed
}

/// Each node's level: the longest path from a source, then each source moved down to
/// just above what it feeds first (shorter edges).
fn rank(n: usize, edges: &[Forward]) -> Vec<usize> {
    let mut indegree = vec![0usize; n];
    let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    let mut inc: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for &(a, b, len, ..) in edges {
        indegree[b] += 1;
        out[a].push((b, len));
        inc[b].push((a, len));
    }
    let mut ranks = vec![0usize; n];
    let mut queue: std::collections::VecDeque<usize> =
        (0..n).filter(|&v| indegree[v] == 0).collect();
    let mut topo = Vec::with_capacity(n);
    while let Some(v) = queue.pop_front() {
        topo.push(v);
        for &(w, len) in &out[v] {
            ranks[w] = ranks[w].max(ranks[v] + len);
            indegree[w] -= 1;
            if indegree[w] == 0 {
                queue.push_back(w);
            }
        }
    }
    // Sources pulled down toward what they feed (never past a level 0 chain).
    for &v in topo.iter().rev() {
        if inc[v].is_empty() && !out[v].is_empty() {
            let lowest = out[v].iter().map(|&(w, len)| ranks[w].saturating_sub(len)).min();
            if let Some(r) = lowest {
                ranks[v] = ranks[v].max(r);
            }
        }
    }
    ranks
}

/// The vertices of each level in order: first as a breadth-first walk meets them, then
/// sweeps down and up placing each by the median of its neighbors on the level before,
/// the order with the fewest crossings kept.
fn order(vertices: &[Vertex], links: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let ranks = vertices.iter().map(|v| v.rank).max().map_or(0, |m| m + 1);
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    for &(a, b) in links {
        down[a].push(b);
        up[b].push(a);
    }
    // Breadth-first from the top level's vertices (in the nodes' order).
    let mut levels: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    let mut seen = vec![false; vertices.len()];
    let mut starts: Vec<usize> = (0..vertices.len()).collect();
    starts.sort_by_key(|&v| (vertices[v].rank, v));
    for s in starts {
        if seen[s] {
            continue;
        }
        let mut queue = std::collections::VecDeque::from([s]);
        seen[s] = true;
        while let Some(v) = queue.pop_front() {
            levels[vertices[v].rank].push(v);
            for &w in &down[v] {
                if !seen[w] {
                    seen[w] = true;
                    queue.push_back(w);
                }
            }
        }
    }
    let mut best = levels.clone();
    let mut best_crossings = crossings(&levels, &down, vertices.len());
    let rounds = if vertices.len() > 400 { 4 } else { 16 };
    for round in 0..rounds {
        let downward = round % 2 == 0;
        let mut position = vec![0.0f32; vertices.len()];
        for level in &levels {
            for (i, &v) in level.iter().enumerate() {
                position[v] = i as f32;
            }
        }
        let range: Vec<usize> = if downward {
            (1..ranks).collect()
        } else {
            (0..ranks.saturating_sub(1)).rev().collect()
        };
        for r in range {
            let neighbors = if downward { &up } else { &down };
            let mut keyed: Vec<(f32, usize)> = levels[r]
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let mut ps: Vec<f32> = neighbors[v].iter().map(|&w| position[w]).collect();
                    ps.sort_by(f32::total_cmp);
                    let key = if ps.is_empty() {
                        i as f32
                    } else if ps.len() % 2 == 1 {
                        ps[ps.len() / 2]
                    } else {
                        (ps[ps.len() / 2 - 1] + ps[ps.len() / 2]) / 2.0
                    };
                    (key, v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            levels[r] = keyed.into_iter().map(|(_, v)| v).collect();
            for (i, &v) in levels[r].iter().enumerate() {
                position[v] = i as f32;
            }
        }
        let c = crossings(&levels, &down, vertices.len());
        if c < best_crossings {
            best_crossings = c;
            best = levels.clone();
        }
        if best_crossings == 0 {
            break;
        }
    }
    best
}

/// How many pairs of edges cross between consecutive levels.
fn crossings(levels: &[Vec<usize>], down: &[Vec<usize>], count: usize) -> usize {
    let mut position = vec![0usize; count];
    for level in levels {
        for (i, &v) in level.iter().enumerate() {
            position[v] = i;
        }
    }
    let mut total = 0;
    for level in levels {
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &v in level {
            for &w in &down[v] {
                pairs.push((position[v], position[w]));
            }
        }
        pairs.sort_unstable();
        for i in 0..pairs.len() {
            for j in i + 1..pairs.len() {
                if pairs[j].0 > pairs[i].0 && pairs[j].1 < pairs[i].1 {
                    total += 1;
                }
            }
        }
    }
    total
}

/// Each vertex's center across its level: packed in order first, then moved toward its
/// neighbors' (the level above, then below, a few times), each level fitted in order
/// with its gaps kept.
fn across_positions(
    vertices: &[Vertex],
    levels: &[Vec<usize>],
    links: &[(usize, usize)],
    gap: f32,
) -> Vec<f32> {
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    for &(a, b) in links {
        down[a].push(b);
        up[b].push(a);
    }
    let sep = |a: usize, b: usize| {
        let g =
            if vertices[a].node.is_none() && vertices[b].node.is_none() { gap * 0.35 } else { gap };
        (vertices[a].w + vertices[b].w) / 2.0 + g
    };
    let mut x = vec![0.0f32; vertices.len()];
    for level in levels {
        let mut at = 0.0;
        for (i, &v) in level.iter().enumerate() {
            if i > 0 {
                at += sep(level[i - 1], v);
            }
            x[v] = at;
        }
    }
    // Center each level on the widest one first.
    for _ in 0..6 {
        for (pass, neighbors) in [(0, &up), (1, &down), (2, &up), (3, &down)] {
            let range: Vec<usize> = if pass % 2 == 0 {
                (0..levels.len()).collect()
            } else {
                (0..levels.len()).rev().collect()
            };
            for r in range {
                let level = &levels[r];
                if level.is_empty() {
                    continue;
                }
                let both = pass >= 2;
                let desired: Vec<f32> = level
                    .iter()
                    .map(|&v| {
                        let mut sum = 0.0;
                        let mut count = 0.0;
                        let mut add = |list: &Vec<usize>| {
                            for &w in list {
                                sum += x[w];
                                count += 1.0;
                            }
                        };
                        add(&neighbors[v]);
                        if both {
                            add(if std::ptr::eq(neighbors, &up) { &down[v] } else { &up[v] });
                        }
                        if count > 0.0 { sum / count } else { x[v] }
                    })
                    .collect();
                let weights: Vec<f32> = level
                    .iter()
                    .map(|&v| {
                        let linked = !up[v].is_empty() || !down[v].is_empty();
                        match (vertices[v].node, linked) {
                            (None, _) => 4.0,
                            (Some(_), true) => 1.0,
                            (Some(_), false) => 0.1,
                        }
                    })
                    .collect();
                let mut offsets = vec![0.0f32; level.len()];
                for i in 1..level.len() {
                    offsets[i] = offsets[i - 1] + sep(level[i - 1], level[i]);
                }
                let values: Vec<f32> = desired.iter().zip(&offsets).map(|(d, o)| d - o).collect();
                let fitted = isotonic(&values, &weights);
                for (i, &v) in level.iter().enumerate() {
                    x[v] = fitted[i] + offsets[i];
                }
            }
        }
    }
    x
}

/// The non-decreasing sequence closest to `values` (weighted least squares): pools of
/// adjacent violators averaged.
fn isotonic(values: &[f32], weights: &[f32]) -> Vec<f32> {
    // Each block: its mean, its weight, how many values it holds.
    let mut blocks: Vec<(f32, f32, usize)> = Vec::with_capacity(values.len());
    for (i, &v) in values.iter().enumerate() {
        let w = weights[i].max(1e-3);
        blocks.push((v, w, 1));
        while blocks.len() > 1 {
            let (m2, w2, c2) = blocks[blocks.len() - 1];
            let (m1, w1, c1) = blocks[blocks.len() - 2];
            if m1 <= m2 {
                break;
            }
            blocks.pop();
            let last = blocks.len() - 1;
            blocks[last] = ((m1 * w1 + m2 * w2) / (w1 + w2), w1 + w2, c1 + c2);
        }
    }
    blocks.into_iter().flat_map(|(m, _, c)| std::iter::repeat_n(m, c)).collect()
}

#[cfg(test)]
#[path = "../../tests/unit/diagram/layered.rs"]
mod tests;
