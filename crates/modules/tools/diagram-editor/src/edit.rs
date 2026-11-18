//! What editing measures, in diagram units: the cell under a point (an edge near enough, a
//! shape's box, the topmost first), the cells a selection band holds, a box's handles and the
//! box a handle dragged makes, and a move snapped — to the other shapes' edges and middles
//! (with the guides shown), else to the grid.

use kernel::{Point, Rect};

use crate::drawing::Drawing;

/// A box's handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    pub const ALL: [Handle; 8] =
        [Self::NW, Self::N, Self::NE, Self::E, Self::SE, Self::S, Self::SW, Self::W];

    /// Where on the box (0, ½, 1 across and down).
    fn at(self) -> (f32, f32) {
        match self {
            Self::NW => (0.0, 0.0),
            Self::N => (0.5, 0.0),
            Self::NE => (1.0, 0.0),
            Self::E => (1.0, 0.5),
            Self::SE => (1.0, 1.0),
            Self::S => (0.5, 1.0),
            Self::SW => (0.0, 1.0),
            Self::W => (0.0, 0.5),
        }
    }

    /// It moves the left / right / top / bottom side.
    fn sides(self) -> (bool, bool, bool, bool) {
        let (x, y) = self.at();
        (x == 0.0, x == 1.0, y == 0.0, y == 1.0)
    }
}

/// `r`'s handles, each a square `size` wide centered on its place.
pub fn handles(r: Rect, size: f32) -> [(Handle, Rect); 8] {
    Handle::ALL.map(|h| {
        let (fx, fy) = h.at();
        let (cx, cy) = (r.min_x() + r.size.width * fx, r.min_y() + r.size.height * fy);
        (h, Rect::new(cx - size / 2.0, cy - size / 2.0, size, size))
    })
}

/// `start` with `handle` dragged by (`dx`, `dy`): its sides moved, never smaller than 1;
/// the moved sides on the grid (`grid`).
pub fn resized(start: Rect, handle: Handle, (dx, dy): (f32, f32), grid: Option<f32>) -> Rect {
    let (left, right, top, bottom) = handle.sides();
    let snap = |v: f32| grid.filter(|g| *g > 0.0).map_or(v, |g| (v / g).round() * g);
    let (mut x0, mut y0, mut x1, mut y1) =
        (start.min_x(), start.min_y(), start.max_x(), start.max_y());
    if left {
        x0 = snap(x0 + dx).min(x1 - 1.0);
    }
    if right {
        x1 = snap(x1 + dx).max(x0 + 1.0);
    }
    if top {
        y0 = snap(y0 + dy).min(y1 - 1.0);
    }
    if bottom {
        y1 = snap(y1 + dy).max(y0 + 1.0);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// The distance from `p` to segment `a`–`b`.
fn to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx * dx + dy * dy;
    let t =
        if len > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// The cell at `p`: an edge within `near` of it first (they're thin), else the topmost shape
/// whose box holds it.
pub fn hit(drawing: &Drawing, p: Point, near: f32) -> Option<String> {
    let edge = drawing
        .ways
        .iter()
        .rev()
        .find(|(_, way)| way.windows(2).any(|s| to_segment(p, s[0], s[1]) <= near))
        .map(|(id, _)| id.clone());
    edge.or_else(|| {
        drawing.boxes.iter().rev().find(|(_, r)| r.contains(p)).map(|(id, _)| id.clone())
    })
}

/// The cells `band` holds whole: shapes and edges.
pub fn within(drawing: &Drawing, band: Rect) -> Vec<String> {
    let inside = |p: Point| {
        p.x >= band.min_x() && p.x <= band.max_x() && p.y >= band.min_y() && p.y <= band.max_y()
    };
    let shapes = drawing
        .boxes
        .iter()
        .filter(|(_, r)| inside(r.origin) && inside(Point::new(r.max_x(), r.max_y())))
        .map(|(id, _)| id.clone());
    let edges = drawing
        .ways
        .iter()
        .filter(|(_, way)| way.iter().all(|p| inside(*p)))
        .map(|(id, _)| id.clone());
    shapes.chain(edges).collect()
}

/// A shape's connection points: its sides' middles (where a new edge is drawn from).
pub fn connection_points(r: Rect) -> [Point; 4] {
    let c = r.center();
    [
        Point::new(c.x, r.min_y()),
        Point::new(r.max_x(), c.y),
        Point::new(c.x, r.max_y()),
        Point::new(r.min_x(), c.y),
    ]
}

/// A chosen edge's handle: one of its ends, one of its points, or a segment's middle (dragged:
/// a point put in there).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeHandle {
    Source,
    Target,
    Point(usize),
    /// Between points `i - 1` and `i` (a new point `i`).
    Insert(usize),
}

/// A chosen edge's handles: its ends (on its way), its points (absolute), the middles between
/// consecutive ones.
pub fn edge_handles(way: &[Point], points: &[Point]) -> Vec<(EdgeHandle, Point)> {
    let (Some(first), Some(last)) = (way.first(), way.last()) else { return Vec::new() };
    let mut out = vec![(EdgeHandle::Source, *first), (EdgeHandle::Target, *last)];
    out.extend(points.iter().enumerate().map(|(i, p)| (EdgeHandle::Point(i), *p)));
    let chain: Vec<Point> = std::iter::once(*first)
        .chain(points.iter().copied())
        .chain(std::iter::once(*last))
        .collect();
    for (i, pair) in chain.windows(2).enumerate() {
        let mid = Point::new((pair[0].x + pair[1].x) / 2.0, (pair[0].y + pair[1].y) / 2.0);
        out.push((EdgeHandle::Insert(i), mid));
    }
    out
}

/// A guide shown while moving: a vertical line at `x`, or a horizontal one at `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Guide {
    X(f32),
    Y(f32),
}

/// A move of `moving` by (`dx`, `dy`) snapped: its sides or middle to another shape's within
/// `near` (that line a guide), else its corner to the grid (`grid`).
pub fn snap_move(
    moving: Rect,
    others: &[Rect],
    (dx, dy): (f32, f32),
    grid: Option<f32>,
    near: f32,
) -> (f32, f32, Vec<Guide>) {
    let lines = |r: &Rect, across: bool| {
        if across {
            [r.min_x(), r.center().x, r.max_x()]
        } else {
            [r.min_y(), r.center().y, r.max_y()]
        }
    };
    let mut guides = Vec::new();
    let mut axis = |across: bool, d: f32| -> f32 {
        let moved: Vec<f32> = lines(&moving, across).iter().map(|v| v + d).collect();
        let mut best: Option<(f32, f32)> = None;
        for other in others {
            for line in lines(other, across) {
                for m in &moved {
                    let gap = line - m;
                    if gap.abs() <= near && best.is_none_or(|(g, _)| gap.abs() < g.abs()) {
                        best = Some((gap, line));
                    }
                }
            }
        }
        if let Some((gap, line)) = best {
            guides.push(if across { Guide::X(line) } else { Guide::Y(line) });
            return d + gap;
        }
        let start = if across { moving.min_x() } else { moving.min_y() };
        grid.filter(|g| *g > 0.0).map_or(d, |g| ((start + d) / g).round() * g - start)
    };
    let dx = axis(true, dx);
    let dy = axis(false, dy);
    (dx, dy, guides)
}

/// The box around `a` and `b`.
pub(crate) fn union(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.min_x().min(b.min_x()), a.min_y().min(b.min_y()));
    let (x1, y1) = (a.max_x().max(b.max_x()), a.max_y().max(b.max_y()));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
#[path = "../tests/unit/edit.rs"]
mod tests;
