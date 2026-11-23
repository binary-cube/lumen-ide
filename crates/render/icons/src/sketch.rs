//! The sketch style: an icon drawn again as if by hand — each line traced twice, a little
//! off and wavering, past its ends; each closed shape hatched with fine diagonal strokes; the
//! small filled shapes (dots) kept filled, a tint (a fill seen through) as light as it was.
//! The drawing is read with `usvg` (every shape as
//! points, its transforms applied), so any icon of the set comes out so. The same icon is
//! always drawn the same (its wobble seeded by its own text), and only once: what's drawn
//! is rasterized and kept like every other style's.

use resvg::usvg;
use usvg::tiny_skia_path::{PathSegment, Point};

/// A small random source, seeded (the same drawing for the same icon).
struct Wobble(u64);

impl Wobble {
    fn of(text: &str) -> Self {
        // Stable across runs and builds.
        Self(kernel::hash::fnv64(text) | 1)
    }

    /// A number in `-1.0..1.0`.
    fn next(&mut self) -> f32 {
        // xorshift64
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32 / (1u64 << 23) as f32) - 1.0
    }
}

/// A shape as points: closed or not, filled (no stroke of its own) or not — then how
/// opaque (a tint under the lines: less than 1) —, its line's width.
struct Shape {
    points: Vec<Point>,
    closed: bool,
    filled: bool,
    opacity: f32,
    width: f32,
}

/// Curves cut into this many straight pieces (an icon is small: enough to look round).
const CURVE_STEPS: usize = 8;
/// How far a line wavers and how far past its ends it goes (in the icon's units, of 24).
const WAVER: f32 = 0.35;
const PAST_ENDS: f32 = 0.45;
/// The hatching: its angle, the gap between its strokes, its strokes' width and strength;
/// a shape smaller than this isn't hatched (its outline says enough).
const HATCH_ANGLE: f32 = -0.72;
const HATCH_GAP: f32 = 2.1;
const HATCH_WIDTH: f32 = 0.75;
const HATCH_OPACITY: f32 = 0.5;
const HATCH_AREA_MIN: f32 = 7.0;

/// `svg` drawn in the sketch style; `None` if it can't be read (drawn as it is then).
pub fn sketch(svg: &str) -> Option<String> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let mut shapes = Vec::new();
    collect(tree.root(), &mut shapes);
    let size = tree.size();
    let mut wobble = Wobble::of(svg);
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" fill=\"none\" \
         stroke=\"#000\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
        num(size.width()),
        num(size.height())
    );
    for shape in &shapes {
        if shape.filled {
            out.push_str(&format!(
                "<path stroke=\"none\" fill=\"#000\" fill-opacity=\"{}\" d=\"{}\"/>",
                num(shape.opacity),
                path(&shape.points, true)
            ));
            continue;
        }
        if shape.closed && area(&shape.points).abs() >= HATCH_AREA_MIN {
            for (a, b) in hatch(&shape.points) {
                let line = wavering(&[a, b], false, 0.4, &mut wobble);
                out.push_str(&format!(
                    "<path stroke-width=\"{}\" stroke-opacity=\"{}\" d=\"{}\"/>",
                    num(HATCH_WIDTH),
                    num(HATCH_OPACITY),
                    path(&line, false)
                ));
            }
        }
        // Traced twice: the first firm, the second lighter and a little off.
        for (k, opacity) in [(0.85, 1.0), (0.6, 0.65)] {
            let line = wavering(&shape.points, shape.closed, 1.0, &mut wobble);
            out.push_str(&format!(
                "<path stroke-width=\"{}\" stroke-opacity=\"{}\" d=\"{}\"/>",
                num(shape.width * k),
                num(opacity),
                path(&line, false)
            ));
        }
    }
    out.push_str("</svg>");
    Some(out)
}

/// Every shape of `group` (and of the groups in it), as points where they're drawn.
fn collect(group: &usvg::Group, shapes: &mut Vec<Shape>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, shapes),
            usvg::Node::Path(p) if p.is_visible() => {
                let transform = p.abs_transform();
                let width = p.stroke().map_or(2.0, |s| s.width().get());
                let filled = p.stroke().is_none() && p.fill().is_some();
                let opacity = p.fill().map_or(1.0, |f| f.opacity().get());
                for (mut points, closed) in subpaths(p.data()) {
                    transform.map_points(&mut points);
                    if points.len() >= 2 {
                        shapes.push(Shape { points, closed, filled, opacity, width });
                    }
                }
            }
            _ => {}
        }
    }
}

/// A path's pieces as points (curves cut straight), each closed or not.
fn subpaths(data: &usvg::tiny_skia_path::Path) -> Vec<(Vec<Point>, bool)> {
    let mut out = Vec::new();
    let mut points: Vec<Point> = Vec::new();
    let end = |points: &mut Vec<Point>, closed: bool, out: &mut Vec<(Vec<Point>, bool)>| {
        if points.len() >= 2 {
            out.push((std::mem::take(points), closed));
        } else {
            points.clear();
        }
    };
    for segment in data.segments() {
        let last = points.last().copied().unwrap_or_default();
        match segment {
            PathSegment::MoveTo(p) => {
                end(&mut points, false, &mut out);
                points.push(p);
            }
            PathSegment::LineTo(p) => points.push(p),
            PathSegment::QuadTo(c, p) => {
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    points.push(Point::from_xy(
                        u * u * last.x + 2.0 * u * t * c.x + t * t * p.x,
                        u * u * last.y + 2.0 * u * t * c.y + t * t * p.y,
                    ));
                }
            }
            PathSegment::CubicTo(c1, c2, p) => {
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    points.push(Point::from_xy(
                        a * last.x + b * c1.x + c * c2.x + d * p.x,
                        a * last.y + b * c1.y + c * c2.y + d * p.y,
                    ));
                }
            }
            PathSegment::Close => {
                let first = points.first().copied();
                end(&mut points, true, &mut out);
                // A path drawn on after its close starts where it closed.
                points.extend(first);
            }
        }
    }
    end(&mut points, false, &mut out);
    out
}

/// `points` as a hand draws them: each piece bowed a little at its middle, each point a little
/// off, an open line going past its ends, a closed one closing past its start (`k`: how much).
fn wavering(points: &[Point], closed: bool, k: f32, wobble: &mut Wobble) -> Vec<Point> {
    let mut line: Vec<Point> = points.to_vec();
    if closed && let Some(first) = line.first().copied() {
        line.push(first);
    }
    let mut out = Vec::with_capacity(line.len() * 2 + 2);
    let off = |p: Point, wobble: &mut Wobble| {
        Point::from_xy(p.x + wobble.next() * WAVER * 0.5 * k, p.y + wobble.next() * WAVER * 0.5 * k)
    };
    // Past its start (an open line) / its end (both: a closed one overlaps where it began).
    if let [a, b, ..] = line[..] {
        out.push(off(beyond(b, a, PAST_ENDS * k * (1.0 + wobble.next() * 0.3)), wobble));
    }
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if out.is_empty() {
            out.push(off(a, wobble));
        }
        let length = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        // A long piece bows at its middle; a short one (a curve's) only wavers.
        if length > 3.0 {
            let (nx, ny) = (-(b.y - a.y) / length, (b.x - a.x) / length);
            let bow = wobble.next() * WAVER * k;
            out.push(Point::from_xy((a.x + b.x) / 2.0 + nx * bow, (a.y + b.y) / 2.0 + ny * bow));
        }
        out.push(off(b, wobble));
    }
    if let [.., a, b] = line[..] {
        out.push(off(beyond(a, b, PAST_ENDS * k * (1.0 + wobble.next() * 0.3)), wobble));
    }
    out
}

/// The point `by` past `to`, on the line from `from`.
fn beyond(from: Point, to: Point, by: f32) -> Point {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 0.01 {
        return to;
    }
    Point::from_xy(to.x + dx / length * by, to.y + dy / length * by)
}

/// The signed area of a closed shape.
fn area(points: &[Point]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f32>()
        / 2.0
}

/// The hatching of a closed shape: parallel strokes at [`HATCH_ANGLE`], [`HATCH_GAP`] apart,
/// each the part of its line inside the shape.
fn hatch(points: &[Point]) -> Vec<(Point, Point)> {
    let (sin, cos) = HATCH_ANGLE.sin_cos();
    // Turned so the strokes lie flat: each is then a row of the turned shape.
    let turned: Vec<Point> = points
        .iter()
        .map(|p| Point::from_xy(p.x * cos + p.y * sin, -p.x * sin + p.y * cos))
        .collect();
    let back = |p: Point| Point::from_xy(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
    let (top, bottom) =
        turned.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    let mut strokes = Vec::new();
    let mut y = top + HATCH_GAP / 2.0;
    let n = turned.len();
    while y < bottom {
        let mut xs: Vec<f32> = (0..n)
            .filter_map(|i| {
                let (a, b) = (turned[i], turned[(i + 1) % n]);
                // Half-open: a row through a corner counts it once.
                if (a.y <= y) == (b.y <= y) {
                    return None;
                }
                Some(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x))
            })
            .collect();
        xs.sort_by(f32::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            // Kept in from the outline a little (the outline is drawn over it).
            let (x0, x1) = (pair[0] + 0.5, pair[1] - 0.5);
            if x1 > x0 {
                strokes.push((back(Point::from_xy(x0, y)), back(Point::from_xy(x1, y))));
            }
        }
        y += HATCH_GAP;
    }
    strokes
}

/// `points` as path data.
fn path(points: &[Point], closed: bool) -> String {
    let mut d = String::with_capacity(points.len() * 12);
    for (i, p) in points.iter().enumerate() {
        d.push(if i == 0 { 'M' } else { 'L' });
        d.push_str(&num(p.x));
        d.push(' ');
        d.push_str(&num(p.y));
    }
    if closed {
        d.push('Z');
    }
    d
}

/// A number written short (two decimals at most).
fn num(v: f32) -> String {
    let v = (v * 100.0).round() / 100.0;
    format!("{v}")
}

#[cfg(test)]
#[path = "../tests/unit/sketch.rs"]
mod tests;
