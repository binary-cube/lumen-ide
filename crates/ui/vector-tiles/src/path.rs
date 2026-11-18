//! Outlines: a path's pieces (moves, lines, cubic curves, a close), how its outline is
//! drawn (color, width, dashes), and the closed shapes built from them (a box with rounded
//! corners, an ellipse, a polygon) and a line flowing through points.

use kernel::{Point, Rect, Rgba};

/// A piece of a path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

/// How a path's outline is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub color: Rgba,
    pub width: f32,
    /// Dash and gap lengths.
    pub dash: Option<(f32, f32)>,
}

impl Line {
    pub fn solid(color: Rgba, width: f32) -> Self {
        Self { color, width, dash: None }
    }

    pub fn dashed(color: Rgba, width: f32) -> Self {
        Self { color, width, dash: Some((5.0, 4.0)) }
    }
}

/// A rectangle with rounded corners (`radius` 0: square), as a closed path.
pub fn rounded(r: Rect, radius: f32) -> Vec<Seg> {
    let (x0, y0, x1, y1) = (r.min_x(), r.min_y(), r.max_x(), r.max_y());
    let rad = radius.min(r.size.width / 2.0).min(r.size.height / 2.0).max(0.0);
    if rad <= 0.0 {
        return vec![
            Seg::Move(Point::new(x0, y0)),
            Seg::Line(Point::new(x1, y0)),
            Seg::Line(Point::new(x1, y1)),
            Seg::Line(Point::new(x0, y1)),
            Seg::Close,
        ];
    }
    // A quarter circle as a cubic: the control points at this fraction of the radius.
    let k = rad * 0.552_284_8;
    let p = Point::new;
    vec![
        Seg::Move(p(x0 + rad, y0)),
        Seg::Line(p(x1 - rad, y0)),
        Seg::Cubic(p(x1 - rad + k, y0), p(x1, y0 + rad - k), p(x1, y0 + rad)),
        Seg::Line(p(x1, y1 - rad)),
        Seg::Cubic(p(x1, y1 - rad + k), p(x1 - rad + k, y1), p(x1 - rad, y1)),
        Seg::Line(p(x0 + rad, y1)),
        Seg::Cubic(p(x0 + rad - k, y1), p(x0, y1 - rad + k), p(x0, y1 - rad)),
        Seg::Line(p(x0, y0 + rad)),
        Seg::Cubic(p(x0, y0 + rad - k), p(x0 + rad - k, y0), p(x0 + rad, y0)),
        Seg::Close,
    ]
}

/// An ellipse in `r`.
pub fn ellipse(r: Rect) -> Vec<Seg> {
    let (cx, cy) = (r.center().x, r.center().y);
    let (rx, ry) = (r.size.width / 2.0, r.size.height / 2.0);
    let (kx, ky) = (rx * 0.552_284_8, ry * 0.552_284_8);
    let p = Point::new;
    vec![
        Seg::Move(p(cx + rx, cy)),
        Seg::Cubic(p(cx + rx, cy + ky), p(cx + kx, cy + ry), p(cx, cy + ry)),
        Seg::Cubic(p(cx - kx, cy + ry), p(cx - rx, cy + ky), p(cx - rx, cy)),
        Seg::Cubic(p(cx - rx, cy - ky), p(cx - kx, cy - ry), p(cx, cy - ry)),
        Seg::Cubic(p(cx + kx, cy - ry), p(cx + rx, cy - ky), p(cx + rx, cy)),
        Seg::Close,
    ]
}

/// A closed polygon.
pub fn polygon(points: &[Point]) -> Vec<Seg> {
    let mut segs: Vec<Seg> = points
        .iter()
        .enumerate()
        .map(|(i, p)| if i == 0 { Seg::Move(*p) } else { Seg::Line(*p) })
        .collect();
    segs.push(Seg::Close);
    segs
}

/// A smooth line through `points`: each step a cubic leaving and entering along the main
/// axis (`vertical`: down / up), as the layered diagrams' edges flow.
pub fn flowing(points: &[Point], vertical: bool) -> Vec<Seg> {
    let mut segs = Vec::with_capacity(points.len());
    let Some(first) = points.first() else { return segs };
    segs.push(Seg::Move(*first));
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (c1, c2) = if vertical {
            let my = (a.y + b.y) / 2.0;
            (Point::new(a.x, my), Point::new(b.x, my))
        } else {
            let mx = (a.x + b.x) / 2.0;
            (Point::new(mx, a.y), Point::new(mx, b.y))
        };
        segs.push(Seg::Cubic(c1, c2, b));
    }
    segs
}
