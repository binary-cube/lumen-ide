//! A laid-out chart written as SVG (its export; a PNG is rasterized from it): the same
//! geometry the screen draws — grid, areas, lines, bars, points, slices, ticks' labels,
//! legend — in the colors it's given (the theme's, read by whoever calls). Pure: a string.

use std::fmt::Write as _;

use kernel::{Point, Rgba};

use crate::geometry::{Geometry, MarkShape, stepped};
use crate::prepare::Prepared;
use crate::spec::ChartKind;

/// The colors and text a chart is written in.
#[derive(Debug, Clone)]
pub struct SvgLook {
    /// The series' colors, in order (past the list: again).
    pub colors: Vec<Rgba>,
    pub text: Rgba,
    pub muted: Rgba,
    pub grid: Rgba,
    pub zero: Rgba,
    pub background: Rgba,
    pub font_family: String,
    /// The labels' size (the geometry's pixels).
    pub font_size: f32,
    pub line_width: f32,
    /// An area's fill alpha, a stacked one's.
    pub area_alpha: (f32, f32),
}

impl SvgLook {
    fn series(&self, i: usize) -> Rgba {
        self.colors.get(i % self.colors.len().max(1)).copied().unwrap_or(self.text)
    }
}

/// `g` (laid out from `p`) as an SVG document as big as its area.
pub fn svg(p: &Prepared, g: &Geometry, look: &SvgLook) -> String {
    let o = g.area.origin;
    let at = |q: Point| (q.x - o.x, q.y - o.y);
    let (w, h) = (g.area.size.width.max(1.0), g.area.size.height.max(1.0));
    let mut s = String::with_capacity(16 * 1024);
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.1} {h:.1}" font-family="{}" font-size="{:.1}">"#,
        escape(&look.font_family),
        look.font_size
    );
    let _ = write!(s, r#"<rect width="100%" height="100%" fill="{}"/>"#, color(look.background));
    for t in &g.y_ticks {
        let y = t.at - o.y;
        let (x0, x1) = (g.plot.min_x() - o.x, g.plot.max_x() - o.x);
        let _ = write!(
            s,
            r#"<line x1="{x0:.1}" y1="{y:.1}" x2="{x1:.1}" y2="{y:.1}" stroke="{}" stroke-width="1"/>"#,
            color(look.grid)
        );
        let _ = write!(
            s,
            r#"<text x="{:.1}" y="{y:.1}" text-anchor="end" dominant-baseline="middle" fill="{}">{}</text>"#,
            x0 - look.font_size * 0.6,
            color(look.muted),
            escape(&t.label)
        );
    }
    if let Some(zero) = g.zero {
        let y = zero - o.y;
        let (x0, x1) = (g.plot.min_x() - o.x, g.plot.max_x() - o.x);
        let _ = write!(
            s,
            r#"<line x1="{x0:.1}" y1="{y:.1}" x2="{x1:.1}" y2="{y:.1}" stroke="{}" stroke-width="1"/>"#,
            color(look.zero)
        );
    }
    for t in &g.x_ticks {
        let _ = write!(
            s,
            r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" dominant-baseline="hanging" fill="{}">{}</text>"#,
            t.at - o.x,
            g.plot.max_y() - o.y + look.font_size * 0.4,
            color(look.muted),
            escape(&t.label)
        );
    }
    let stacked = p.kind.stacked();
    for line in &g.lines {
        let c = look.series(line.series);
        if let Some(floors) = &line.floor {
            let alpha = if stacked { look.area_alpha.1 } else { look.area_alpha.0 };
            for (run, floor) in line.runs.iter().zip(floors) {
                let mut points: Vec<(f32, f32)> =
                    stepped(run, line.stepped).into_iter().map(at).collect();
                points.extend(stepped(floor, line.stepped).into_iter().rev().map(at));
                let _ = write!(
                    s,
                    r#"<polygon points="{}" fill="{}" fill-opacity="{alpha:.2}"/>"#,
                    list(&points),
                    color(c)
                );
            }
        }
        for run in &line.runs {
            let points: Vec<(f32, f32)> = stepped(run, line.stepped).into_iter().map(at).collect();
            let _ = write!(
                s,
                r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="{:.2}" stroke-linejoin="round" stroke-linecap="round"/>"#,
                list(&points),
                color(c),
                look.line_width
            );
        }
    }
    for m in &g.marks {
        let c = if p.kind == ChartKind::Pie { look.series(m.index) } else { look.series(m.series) };
        match m.shape {
            MarkShape::Bar(r) => {
                let _ = write!(
                    s,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="2" fill="{}"/>"#,
                    r.min_x() - o.x,
                    r.min_y() - o.y,
                    r.size.width,
                    r.size.height,
                    color(c)
                );
            }
            MarkShape::Point { center, radius } => {
                let (x, y) = at(center);
                let _ = write!(
                    s,
                    r#"<circle cx="{x:.1}" cy="{y:.1}" r="{radius:.1}" fill="{}" fill-opacity="0.8"/>"#,
                    color(c)
                );
            }
            MarkShape::Slice { center, radius, inner, start, end } => {
                let (cx, cy) = at(center);
                let point = |r: f32, a: f32| (cx + r * a.sin(), cy - r * a.cos());
                let large = u8::from(end - start > std::f32::consts::PI);
                let (fill, edge) = (color(c), color(look.background));
                let width = look.line_width;
                let whole = end - start >= std::f32::consts::TAU - 1e-4;
                match (whole, inner > 0.0) {
                    (true, false) => {
                        let _ = write!(
                            s,
                            r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{radius:.1}" fill="{fill}"/>"#
                        );
                    }
                    (true, true) => {
                        // A whole ring: a circle's line as wide as the ring.
                        let (r, w) = ((radius + inner) / 2.0, radius - inner);
                        let _ = write!(
                            s,
                            r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.1}" fill="none" stroke="{fill}" stroke-width="{w:.1}"/>"#
                        );
                    }
                    (false, false) => {
                        let ((ax, ay), (bx, by)) = (point(radius, start), point(radius, end));
                        let _ = write!(
                            s,
                            r#"<path d="M{cx:.1},{cy:.1} L{ax:.1},{ay:.1} A{radius:.1},{radius:.1} 0 {large} 1 {bx:.1},{by:.1} Z" fill="{fill}" stroke="{edge}" stroke-width="{width:.2}"/>"#
                        );
                    }
                    (false, true) => {
                        let ((ax, ay), (bx, by)) = (point(radius, start), point(radius, end));
                        let ((ix, iy), (jx, jy)) = (point(inner, end), point(inner, start));
                        let _ = write!(
                            s,
                            r#"<path d="M{ax:.1},{ay:.1} A{radius:.1},{radius:.1} 0 {large} 1 {bx:.1},{by:.1} L{ix:.1},{iy:.1} A{inner:.1},{inner:.1} 0 {large} 0 {jx:.1},{jy:.1} Z" fill="{fill}" stroke="{edge}" stroke-width="{width:.2}"/>"#
                        );
                    }
                }
            }
        }
    }
    for item in &g.legend {
        let c = look.series(item.index);
        let r = item.swatch;
        let _ = write!(
            s,
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="2" fill="{}"/>"#,
            r.min_x() - o.x,
            r.min_y() - o.y,
            r.size.width,
            r.size.height,
            if item.hidden { "none".to_owned() } else { color(c) }
        );
        let _ = write!(
            s,
            r#"<text x="{:.1}" y="{:.1}" dominant-baseline="middle" fill="{}">{}</text>"#,
            item.label.x - o.x,
            item.label.y - o.y,
            color(if item.hidden { look.muted } else { look.text }),
            escape(&item.name)
        );
    }
    s.push_str("</svg>");
    s
}

fn list(points: &[(f32, f32)]) -> String {
    let mut s = String::with_capacity(points.len() * 12);
    for (x, y) in points {
        let _ = write!(s, "{x:.1},{y:.1} ");
    }
    s
}

/// A color as SVG writes it (its alpha apart: `#rrggbb` with `-opacity` would lose none).
fn color(c: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if c.a >= 0.999 {
        format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
    } else {
        format!("rgba({},{},{},{:.3})", byte(c.r), byte(c.g), byte(c.b), c.a.clamp(0.0, 1.0))
    }
}

/// A text as SVG holds it.
fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
#[path = "../tests/unit/svg.rs"]
mod tests;
