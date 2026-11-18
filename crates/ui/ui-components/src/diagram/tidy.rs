//! A tree laid out tidily ([`tidy`]): each node's card at its given size, its children in
//! the next level after it — under it ([`Flow::Down`]) or to its right ([`Flow::Right`]) —,
//! a parent centered over its children, the subtrees packed as close as their contours
//! allow without ever overlapping (Reingold–Tilford), in O(n): each subtree's contour (its
//! extent on every level) is kept deepest level first with a lazy offset, so a merge costs
//! the shorter one. The levels line up: a level is as deep as its deepest card.
//!
//! [`links`] then draws the edges the diagram way ([`Edge`]): axis-aligned, from a child to
//! its parent (the way a plan's rows flow), each child entering its parent at a lane of its
//! own along the side facing it.

use kernel::{Point, Rect};

use super::{Dir, Edge, bounds_of};

/// Which way the levels go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Flow {
    /// The roots on top, children under their parent.
    #[default]
    Down,
    /// The roots on the left, children to their parent's right.
    Right,
}

/// The room between cards (diagram pixels): beside one another on a level, and between
/// levels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gaps {
    pub sibling: f32,
    /// Between two subtrees (more than between siblings: their parts stay apart).
    pub subtree: f32,
    pub level: f32,
}

impl Default for Gaps {
    fn default() -> Self {
        Self { sibling: 24.0, subtree: 36.0, level: 56.0 }
    }
}

/// A subtree's contour: its extent `(lo, hi)` on each of its levels, the deepest first (so a
/// parent's own level is a push), relative to `offset` added on reading.
#[derive(Debug, Default)]
struct Contour {
    offset: f32,
    lo: Vec<f32>,
    hi: Vec<f32>,
}

impl Contour {
    fn len(&self) -> usize {
        self.lo.len()
    }

    /// Level `l` from the top (0: the subtree's root), as read.
    fn at(&self, l: usize) -> (f32, f32) {
        let i = self.len() - 1 - l;
        (self.lo[i] + self.offset, self.hi[i] + self.offset)
    }
}

/// The cards of a forest laid out: `children[i]` node `i`'s children in their order,
/// `roots` the trees in theirs, `sizes[i]` its card's (width, height). Returns each node's
/// rectangle (diagram space, the first root's top left at the origin; a node reached from
/// no root keeps an empty one).
pub fn tidy(
    children: &[Vec<usize>],
    roots: &[usize],
    sizes: &[(f32, f32)],
    flow: Flow,
    gaps: Gaps,
) -> Vec<Rect> {
    let n = sizes.len();
    // Breadth (across the levels) and depth (along them) of each card.
    let breadth = |i: usize| match flow {
        Flow::Down => sizes[i].0,
        Flow::Right => sizes[i].1,
    };
    let depth_of = |i: usize| match flow {
        Flow::Down => sizes[i].1,
        Flow::Right => sizes[i].0,
    };
    // Depth first, parents before children (iteratively: plans may be deep).
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut level = vec![usize::MAX; n];
    let mut parent = vec![usize::MAX; n];
    let mut stack: Vec<usize> = roots.iter().rev().copied().filter(|&r| r < n).collect();
    for &r in &stack {
        level[r] = 0;
    }
    while let Some(i) = stack.pop() {
        order.push(i);
        for &c in children[i].iter().rev() {
            if c < n && level[c] == usize::MAX {
                level[c] = level[i] + 1;
                parent[c] = i;
                stack.push(c);
            }
        }
    }
    let parent_of = &parent;
    let kids = |i: usize| children[i].iter().copied().filter(move |&c| c < n && parent_of[c] == i);
    // Bottom up: each subtree's contour and its children's places relative to it.
    let mut rel = vec![0.0f32; n];
    let mut contours: Vec<Option<Contour>> = (0..n).map(|_| None).collect();
    for &i in order.iter().rev() {
        let own = breadth(i) / 2.0;
        let mut merged: Option<Contour> = None;
        let mut places: Vec<(usize, f32)> = Vec::new();
        for c in kids(i) {
            let Some(mut next) = contours[c].take() else { continue };
            let Some(mut acc) = merged.take() else {
                places.push((c, 0.0));
                merged = Some(next);
                continue;
            };
            // How far right of what's placed it must go: on every common level.
            let common = acc.len().min(next.len());
            let mut shift = f32::MIN;
            for l in 0..common {
                let gap = if l == 0 { gaps.sibling } else { gaps.subtree };
                shift = shift.max(acc.at(l).1 - next.at(l).0 + gap);
            }
            places.push((c, shift));
            next.offset += shift;
            // The longer one keeps its vectors; the shorter's levels are written into it.
            if next.len() > acc.len() {
                for l in 0..common {
                    let i_next = next.len() - 1 - l;
                    next.lo[i_next] = acc.at(l).0 - next.offset;
                }
                merged = Some(next);
            } else {
                for l in 0..common {
                    let i_acc = acc.len() - 1 - l;
                    acc.hi[i_acc] = next.at(l).1 - acc.offset;
                }
                merged = Some(acc);
            }
        }
        let mut contour = match merged {
            Some(mut m) => {
                // The parent centered over its first and last children.
                let (first, last) = (places[0].1, places[places.len() - 1].1);
                let center = (first + last) / 2.0;
                for (c, at) in &places {
                    rel[*c] = at - center;
                }
                m.offset -= center;
                m
            }
            None => Contour::default(),
        };
        contour.lo.push(-own - contour.offset);
        contour.hi.push(own - contour.offset);
        contours[i] = Some(contour);
    }
    // The roots side by side, as their contours allow.
    let mut root_at: Vec<(usize, f32)> = Vec::new();
    let mut acc: Option<Contour> = None;
    for &r in roots.iter().filter(|&&r| r < n) {
        let Some(mut next) = contours[r].take() else { continue };
        match acc.take() {
            None => {
                root_at.push((r, 0.0));
                acc = Some(next);
            }
            Some(mut a) => {
                let common = a.len().min(next.len());
                let mut shift = f32::MIN;
                for l in 0..common {
                    shift = shift.max(a.at(l).1 - next.at(l).0 + gaps.subtree);
                }
                root_at.push((r, shift));
                next.offset += shift;
                if next.len() > a.len() {
                    for l in 0..common {
                        let k = next.len() - 1 - l;
                        next.lo[k] = a.at(l).0 - next.offset;
                    }
                    acc = Some(next);
                } else {
                    for l in 0..common {
                        let k = a.len() - 1 - l;
                        a.hi[k] = next.at(l).1 - a.offset;
                    }
                    acc = Some(a);
                }
            }
        }
    }
    // Each level's start along the depth: as deep as its deepest card.
    let levels = order.iter().map(|&i| level[i] + 1).max().unwrap_or(0);
    let mut deepest = vec![0.0f32; levels];
    for &i in &order {
        deepest[level[i]] = deepest[level[i]].max(depth_of(i));
    }
    let mut starts = vec![0.0f32; levels];
    for l in 1..levels {
        starts[l] = starts[l - 1] + deepest[l - 1] + gaps.level;
    }
    // Top down: the centers along the breadth.
    let mut center = vec![0.0f32; n];
    for &(r, at) in &root_at {
        center[r] = at;
    }
    let mut out = vec![Rect::default(); n];
    for &i in &order {
        if parent[i] != usize::MAX {
            center[i] = center[parent[i]] + rel[i];
        }
        let (w, h) = sizes[i];
        let lead = starts[level[i]];
        // A card centered on its level's band (a shorter one in its middle).
        let inset = (deepest[level[i]] - depth_of(i)) / 2.0;
        out[i] = match flow {
            Flow::Down => Rect::new(center[i] - w / 2.0, lead + inset, w, h),
            Flow::Right => Rect::new(lead + inset, center[i] - h / 2.0, w, h),
        };
    }
    // The origin at the drawing's top left.
    let placed: Vec<Rect> = order.iter().map(|&i| out[i]).collect();
    let b = bounds_of(placed);
    for &i in &order {
        let r = out[i];
        out[i] =
            Rect::new(r.min_x() - b.min_x(), r.min_y() - b.min_y(), r.size.width, r.size.height);
    }
    out
}

/// The edges of a laid-out forest: one per child, from its side facing its parent to the
/// parent's side facing it (an arrow pointing into the parent), axis-aligned, bent halfway
/// between the levels; `kind` says which child and which parent it joins. Children enter
/// their parent at lanes of their own, spread over the middle of its side.
pub fn links(children: &[Vec<usize>], rects: &[Rect], flow: Flow) -> Vec<Edge<(usize, usize)>> {
    let mut out = Vec::new();
    for (parent, kids) in children.iter().enumerate() {
        let Some(&pr) = rects.get(parent) else { continue };
        if pr.size.width <= 0.0 {
            continue;
        }
        let kids: Vec<usize> = kids
            .iter()
            .copied()
            .filter(|&c| rects.get(c).is_some_and(|r| r.size.width > 0.0))
            .collect();
        let k = kids.len();
        for (lane, &child) in kids.iter().enumerate() {
            let cr = rects[child];
            // Where it enters: lanes over the middle 60 % of the parent's side.
            let t = if k == 1 { 0.5 } else { 0.2 + 0.6 * lane as f32 / (k - 1) as f32 };
            let edge = match flow {
                Flow::Down => {
                    let from = Point::new(cr.min_x() + cr.size.width / 2.0, cr.min_y());
                    let to = Point::new(pr.min_x() + pr.size.width * t, pr.max_y());
                    let mid = (pr.max_y() + cr.min_y()) / 2.0;
                    Edge {
                        points: dedup(vec![
                            from,
                            Point::new(from.x, mid),
                            Point::new(to.x, mid),
                            to,
                        ]),
                        dir: Dir::Up,
                        kind: (child, parent),
                    }
                }
                Flow::Right => {
                    let from = Point::new(cr.min_x(), cr.min_y() + cr.size.height / 2.0);
                    let to = Point::new(pr.max_x(), pr.min_y() + pr.size.height * t);
                    let mid = (pr.max_x() + cr.min_x()) / 2.0;
                    Edge {
                        points: dedup(vec![
                            from,
                            Point::new(mid, from.y),
                            Point::new(mid, to.y),
                            to,
                        ]),
                        dir: Dir::Left,
                        kind: (child, parent),
                    }
                }
            };
            out.push(edge);
        }
    }
    out
}

/// The points with a repeated one (a straight edge's bend) left out.
fn dedup(mut points: Vec<Point>) -> Vec<Point> {
    points.dedup_by(|a, b| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01);
    points
}

#[cfg(test)]
#[path = "../../tests/unit/diagram/tidy.rs"]
mod tests;
