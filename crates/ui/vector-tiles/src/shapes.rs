//! The shapes diagrams are drawn with (draw.io's "General" library), each as an outline in
//! its box: a body (filled, then outlined) and details drawn over it, outlined only (a
//! cylinder's rim, a process's bars, a note's folded corner). Their geometry is draw.io's
//! own, so a shape drawn here looks as it does there; what a shape is called in a style is
//! the caller's business.

use kernel::{Point, Rect};

use crate::path::{Seg, ellipse, polygon, rounded};

/// A shape, with its proportions (in its box's units, as draw.io's style gives them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeKind {
    /// A box, its corners rounded by `radius`.
    Rect {
        radius: f32,
    },
    Ellipse,
    Rhombus,
    /// Its sides slanted by `slant`.
    Parallelogram {
        slant: f32,
    },
    /// Its points `inset` from the sides.
    Hexagon {
        inset: f32,
    },
    /// Pointing right.
    Triangle,
    /// Its rim's ellipse `depth` high.
    Cylinder {
        depth: f32,
    },
    Cloud,
    /// Its bottom waving by `wave` (a fraction of its height).
    Document {
        wave: f32,
    },
    /// Its bars `inset` (a fraction of its width) from the sides.
    Process {
        inset: f32,
    },
    /// A person's bust.
    Actor,
    /// Its corner folded by `fold`.
    Note {
        fold: f32,
    },
    /// Pointing right: its shaft `shaft` of its height, its head `head` of its width.
    Arrow {
        shaft: f32,
        head: f32,
    },
    /// Both ways.
    DoubleArrow {
        shaft: f32,
        head: f32,
    },
    /// Narrower at the top, its top corners `inset` in.
    Trapezoid {
        inset: f32,
    },
    /// A chevron pointing right, its point `depth` deep.
    Step {
        depth: f32,
    },
    /// Its top left corner cut by `cut`.
    Card {
        cut: f32,
    },
    /// A speech bubble: its pointer `tail` high, its base from `position` (a fraction of the
    /// width) `base` wide, its tip at `tip` (a fraction of the width).
    Callout {
        tail: f32,
        position: f32,
        tip: f32,
        base: f32,
    },
    /// Its top and bottom waving by `wave` (a fraction of its height).
    Tape {
        wave: f32,
    },
    /// Flat on the left, round on the right.
    Delay,
    /// A box pointing down, its point `depth` deep.
    OffPage {
        depth: f32,
    },
    /// Its top slanting up to the right by `rise`.
    ManualInput {
        rise: f32,
    },
}

/// A shape's outline: its body, and its details over it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outline {
    pub body: Vec<Seg>,
    pub details: Vec<Seg>,
}

/// A quadratic curve as the cubic it is.
fn quad(from: Point, control: Point, to: Point) -> Seg {
    let c1 = Point::new(
        from.x + (control.x - from.x) * 2.0 / 3.0,
        from.y + (control.y - from.y) * 2.0 / 3.0,
    );
    let c2 =
        Point::new(to.x + (control.x - to.x) * 2.0 / 3.0, to.y + (control.y - to.y) * 2.0 / 3.0);
    Seg::Cubic(c1, c2, to)
}

/// `kind`'s outline in box `r`.
pub fn outline(kind: ShapeKind, r: Rect) -> Outline {
    let (x, y, w, h) = (r.min_x(), r.min_y(), r.size.width.max(0.0), r.size.height.max(0.0));
    // A point in the box's own units.
    let p = |px: f32, py: f32| Point::new(x + px, y + py);
    let body_only = |body| Outline { body, details: Vec::new() };
    match kind {
        ShapeKind::Rect { radius } => body_only(rounded(r, radius)),
        ShapeKind::Ellipse => body_only(ellipse(r)),
        ShapeKind::Rhombus => {
            body_only(polygon(&[p(w / 2.0, 0.0), p(w, h / 2.0), p(w / 2.0, h), p(0.0, h / 2.0)]))
        }
        ShapeKind::Parallelogram { slant } => {
            let s = slant.clamp(0.0, w / 2.0);
            body_only(polygon(&[p(s, 0.0), p(w, 0.0), p(w - s, h), p(0.0, h)]))
        }
        ShapeKind::Hexagon { inset } => {
            let s = inset.clamp(0.0, w / 2.0);
            body_only(polygon(&[
                p(s, 0.0),
                p(w - s, 0.0),
                p(w, h / 2.0),
                p(w - s, h),
                p(s, h),
                p(0.0, h / 2.0),
            ]))
        }
        ShapeKind::Triangle => body_only(polygon(&[p(0.0, 0.0), p(w, h / 2.0), p(0.0, h)])),
        ShapeKind::Cylinder { depth } => {
            // The rim: an ellipse `depth` high at the top; the bottom, its lower half.
            let d = depth.clamp(0.0, h / 2.0);
            let k = 0.552_284_8;
            let (rx, ry) = (w / 2.0, d);
            let top = y + d;
            let bottom = y + h - d;
            let cx = x + rx;
            let pt = Point::new;
            let body = vec![
                Seg::Move(pt(x, top)),
                // The rim's upper half.
                Seg::Cubic(pt(x, top - ry * k), pt(cx - rx * k, top - ry), pt(cx, top - ry)),
                Seg::Cubic(pt(cx + rx * k, top - ry), pt(x + w, top - ry * k), pt(x + w, top)),
                Seg::Line(pt(x + w, bottom)),
                // The bottom's lower half.
                Seg::Cubic(
                    pt(x + w, bottom + ry * k),
                    pt(cx + rx * k, bottom + ry),
                    pt(cx, bottom + ry),
                ),
                Seg::Cubic(pt(cx - rx * k, bottom + ry), pt(x, bottom + ry * k), pt(x, bottom)),
                Seg::Close,
            ];
            // The rim's lower half, seen over the body.
            let details = vec![
                Seg::Move(pt(x, top)),
                Seg::Cubic(pt(x, top + ry * k), pt(cx - rx * k, top + ry), pt(cx, top + ry)),
                Seg::Cubic(pt(cx + rx * k, top + ry), pt(x + w, top + ry * k), pt(x + w, top)),
            ];
            Outline { body, details }
        }
        ShapeKind::Cloud => {
            // draw.io's cloud (`mxCloud`), in its box's fractions.
            let f = |fx: f32, fy: f32| p(fx * w, fy * h);
            body_only(vec![
                Seg::Move(f(0.25, 0.25)),
                Seg::Cubic(f(0.05, 0.25), f(0.0, 0.5), f(0.16, 0.55)),
                Seg::Cubic(f(0.0, 0.66), f(0.18, 0.9), f(0.31, 0.8)),
                Seg::Cubic(f(0.4, 1.0), f(0.7, 1.0), f(0.8, 0.8)),
                Seg::Cubic(f(1.0, 0.8), f(1.0, 0.6), f(0.875, 0.5)),
                Seg::Cubic(f(1.0, 0.3), f(0.8, 0.1), f(0.625, 0.2)),
                Seg::Cubic(f(0.5, 0.05), f(0.3, 0.05), f(0.25, 0.25)),
                Seg::Close,
            ])
        }
        ShapeKind::Document { wave } => {
            // draw.io's document: its bottom two quadratic waves.
            let dy = h * wave.clamp(0.0, 1.0);
            let fy = 1.4;
            let base = h - dy / 2.0;
            body_only(vec![
                Seg::Move(p(0.0, 0.0)),
                Seg::Line(p(w, 0.0)),
                Seg::Line(p(w, base)),
                quad(p(w, base), p(3.0 * w / 4.0, h - dy * fy), p(w / 2.0, base)),
                quad(p(w / 2.0, base), p(w / 4.0, h - dy * (1.0 - fy)), p(0.0, base)),
                Seg::Close,
            ])
        }
        ShapeKind::Process { inset } => {
            let s = (w * inset.clamp(0.0, 0.5)).max(0.0);
            Outline {
                body: rounded(r, 0.0),
                details: vec![
                    Seg::Move(p(s, 0.0)),
                    Seg::Line(p(s, h)),
                    Seg::Move(p(w - s, 0.0)),
                    Seg::Line(p(w - s, h)),
                ],
            }
        }
        ShapeKind::Actor => {
            // draw.io's actor (`mxActor`): a head on shoulders, one path.
            let width = w / 3.0;
            body_only(vec![
                Seg::Move(p(0.0, h)),
                Seg::Cubic(p(0.0, 3.0 * h / 5.0), p(0.0, 2.0 * h / 5.0), p(w / 2.0, 2.0 * h / 5.0)),
                Seg::Cubic(
                    p(w / 2.0 - width, 2.0 * h / 5.0),
                    p(w / 2.0 - width, 0.0),
                    p(w / 2.0, 0.0),
                ),
                Seg::Cubic(
                    p(w / 2.0 + width, 0.0),
                    p(w / 2.0 + width, 2.0 * h / 5.0),
                    p(w / 2.0, 2.0 * h / 5.0),
                ),
                Seg::Cubic(p(w, 2.0 * h / 5.0), p(w, 3.0 * h / 5.0), p(w, h)),
                Seg::Close,
            ])
        }
        ShapeKind::Note { fold } => {
            let s = fold.clamp(0.0, w.min(h));
            Outline {
                body: polygon(&[p(0.0, 0.0), p(w - s, 0.0), p(w, s), p(w, h), p(0.0, h)]),
                details: vec![Seg::Move(p(w - s, 0.0)), Seg::Line(p(w - s, s)), Seg::Line(p(w, s))],
            }
        }
        ShapeKind::Arrow { shaft, head } => {
            let (sh, hd) = (h * shaft.clamp(0.0, 1.0), w * head.clamp(0.0, 1.0));
            let (t, b) = ((h - sh) / 2.0, (h + sh) / 2.0);
            body_only(polygon(&[
                p(0.0, t),
                p(w - hd, t),
                p(w - hd, 0.0),
                p(w, h / 2.0),
                p(w - hd, h),
                p(w - hd, b),
                p(0.0, b),
            ]))
        }
        ShapeKind::Trapezoid { inset } => {
            let s = inset.clamp(0.0, w / 2.0);
            body_only(polygon(&[p(s, 0.0), p(w - s, 0.0), p(w, h), p(0.0, h)]))
        }
        ShapeKind::Step { depth } => {
            let s = depth.clamp(0.0, w / 2.0);
            body_only(polygon(&[
                p(0.0, 0.0),
                p(w - s, 0.0),
                p(w, h / 2.0),
                p(w - s, h),
                p(0.0, h),
                p(s, h / 2.0),
            ]))
        }
        ShapeKind::Card { cut } => {
            let s = cut.clamp(0.0, w.min(h));
            body_only(polygon(&[p(s, 0.0), p(w, 0.0), p(w, h), p(0.0, h), p(0.0, s)]))
        }
        ShapeKind::Callout { tail, position, tip, base } => {
            // draw.io's callout: the box, then its pointer from `position` to `position +
            // base` along the bottom, its tip at `tip`.
            let t = tail.clamp(0.0, h);
            let bottom = h - t;
            let a = w * position.clamp(0.0, 1.0);
            let c = w * tip.clamp(0.0, 1.0);
            let b = base.clamp(0.0, w);
            body_only(polygon(&[
                p(0.0, 0.0),
                p(w, 0.0),
                p(w, bottom),
                p((a + b).min(w), bottom),
                p(c, h),
                p(a.max(0.0), bottom),
                p(0.0, bottom),
            ]))
        }
        ShapeKind::Tape { wave } => {
            // draw.io's tape: both edges two quadratic waves (the top dips first).
            let dy = h * wave.clamp(0.0, 1.0);
            let (top, bottom) = (dy / 2.0, h - dy / 2.0);
            body_only(vec![
                Seg::Move(p(0.0, top)),
                quad(p(0.0, top), p(w / 4.0, 1.4 * dy), p(w / 2.0, top)),
                quad(p(w / 2.0, top), p(3.0 * w / 4.0, dy * (1.0 - 1.4)), p(w, top)),
                Seg::Line(p(w, bottom)),
                quad(p(w, bottom), p(3.0 * w / 4.0, h - 1.4 * dy), p(w / 2.0, bottom)),
                quad(p(w / 2.0, bottom), p(w / 4.0, h - dy * (1.0 - 1.4)), p(0.0, bottom)),
                Seg::Close,
            ])
        }
        ShapeKind::Delay => {
            // draw.io's delay: its right end two quadratic curves, `min(w, h / 2)` deep.
            let r = w.min(h / 2.0);
            body_only(vec![
                Seg::Move(p(0.0, 0.0)),
                Seg::Line(p(w - r, 0.0)),
                quad(p(w - r, 0.0), p(w, 0.0), p(w, h / 2.0)),
                quad(p(w, h / 2.0), p(w, h), p(w - r, h)),
                Seg::Line(p(0.0, h)),
                Seg::Close,
            ])
        }
        ShapeKind::OffPage { depth } => {
            let s = depth.clamp(0.0, h);
            body_only(polygon(&[p(0.0, 0.0), p(w, 0.0), p(w, h - s), p(w / 2.0, h), p(0.0, h - s)]))
        }
        ShapeKind::ManualInput { rise } => {
            let s = rise.clamp(0.0, h);
            body_only(polygon(&[p(0.0, s), p(w, 0.0), p(w, h), p(0.0, h)]))
        }
        ShapeKind::DoubleArrow { shaft, head } => {
            let (sh, hd) = (h * shaft.clamp(0.0, 1.0), (w * head.clamp(0.0, 0.5)).min(w / 2.0));
            let (t, b) = ((h - sh) / 2.0, (h + sh) / 2.0);
            body_only(polygon(&[
                p(0.0, h / 2.0),
                p(hd, 0.0),
                p(hd, t),
                p(w - hd, t),
                p(w - hd, 0.0),
                p(w, h / 2.0),
                p(w - hd, h),
                p(w - hd, b),
                p(hd, b),
                p(hd, h),
            ]))
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/shapes.rs"]
mod tests;
