//! A page as it's drawn, made once per version of the document (and theme) and kept: its
//! shapes and edges as outlines in the order draw.io paints them (rasterized in tiles off the
//! UI thread), its texts (drawn by the text system over them), the box around all of it.
//! The cells' places are absolute here (a shape in a group or a lane is placed by its
//! parent's), hidden layers left out; an edge's ends meet its shapes' borders, routed
//! straight or at right angles, through its points, with its markers.

use std::collections::HashMap;

use diagram_model::{Cell, CellKind, Geometry, Page};
use kernel::{Point, Rect, Rgba};
use vector_tiles::{Line, Seg, ShapeKind, outline, polygon};

use crate::label;
use crate::look::{self, Align, EdgeLook, Look, Marker, VAlign};

/// The colors a diagram's defaults take: the theme's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub canvas: Rgba,
    pub fill: Rgba,
    pub stroke: Rgba,
    pub text: Rgba,
}

/// Something drawn: an outline filled and outlined, or only outlined.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub segs: Vec<Seg>,
    pub fill: Option<Rgba>,
    pub line: Option<Line>,
}

/// A text: where (its box, diagram units), what, how.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub area: Rect,
    pub text: String,
    pub size: f32,
    pub color: Rgba,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    pub valign: VAlign,
    pub wrap: bool,
    /// Drawn over a patch of this color (an edge's label over its line).
    pub backdrop: Option<Rgba>,
    /// How far past its box it can reach, across and down, in its font's sizes: its
    /// characters side by side (an em and a bit each), its words one under another.
    pub reach: (f32, f32),
}

/// A page drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawing {
    /// The box around everything (diagram units): the items' outlines are relative to its
    /// corner, the texts and the cells' boxes absolute.
    pub bounds: Rect,
    pub items: Vec<Item>,
    pub texts: Vec<Text>,
    /// Each shape's box (absolute), in drawing order: what's under the mouse.
    pub boxes: Vec<(String, Rect)>,
    /// Each edge's way (absolute), in drawing order.
    pub ways: Vec<(String, Vec<Point>)>,
}

/// The margin around a drawing's content (diagram units).
const MARGIN: f32 = 20.0;
/// How deep parents are followed (a loop in a hostile file stops there).
const MAX_NESTING: usize = 64;

fn rect_of(g: &Geometry) -> Rect {
    Rect::new(g.x as f32, g.y as f32, g.width.max(0.0) as f32, g.height.max(0.0) as f32)
}

/// The page's cells laid out.
struct Layout<'a> {
    cells: HashMap<&'a str, &'a Cell>,
    /// Absolute boxes of the shapes.
    boxes: HashMap<&'a str, (Rect, Look)>,
}

impl<'a> Layout<'a> {
    fn new(page: &'a Page) -> Self {
        let cells = page.cells.iter().map(|c| (c.id.as_str(), c)).collect();
        let mut layout = Self { cells, boxes: HashMap::new() };
        for c in &page.cells {
            if c.kind == CellKind::Vertex
                && layout.shown(c)
                && let Some(g) = c.geometry.as_ref().filter(|g| !g.relative)
            {
                let (dx, dy) = layout.offset(c.parent.as_deref());
                let r = rect_of(g);
                let r = Rect::new(r.min_x() + dx, r.min_y() + dy, r.size.width, r.size.height);
                let look = look::look(&c.style, r.size.width, r.size.height);
                layout.boxes.insert(&c.id, (r, look));
            }
        }
        layout
    }

    /// The cell and every parent it's in are visible.
    fn shown(&self, cell: &Cell) -> bool {
        let mut at = Some(cell);
        for _ in 0..MAX_NESTING {
            let Some(c) = at else { return true };
            if !c.visible {
                return false;
            }
            at = c.parent.as_deref().and_then(|p| self.cells.get(p).copied());
        }
        false
    }

    /// Where a child of `parent` is placed from: its shape's corner (a group, a lane), else
    /// the page's origin.
    fn offset(&self, parent: Option<&str>) -> (f32, f32) {
        let (mut x, mut y) = (0.0, 0.0);
        let mut at = parent;
        for _ in 0..MAX_NESTING {
            let Some(c) = at.and_then(|p| self.cells.get(p)) else { break };
            if c.kind != CellKind::Vertex {
                break;
            }
            if let Some(g) = c.geometry.as_ref().filter(|g| !g.relative) {
                x += g.x as f32;
                y += g.y as f32;
            }
            at = c.parent.as_deref();
        }
        (x, y)
    }
}

/// The point where the line from `from` (inside `r`) toward `to` leaves `r` (an ellipse's
/// border for an ellipse).
fn border(r: Rect, ellipse: bool, to: Point) -> Point {
    let c = r.center();
    let (dx, dy) = (to.x - c.x, to.y - c.y);
    if dx.abs() < 1e-3 && dy.abs() < 1e-3 {
        return c;
    }
    let (hw, hh) = (r.size.width / 2.0, r.size.height / 2.0);
    let t = if ellipse && hw > 0.0 && hh > 0.0 {
        1.0 / ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt()
    } else {
        let tx = if dx.abs() > 1e-6 { hw / dx.abs() } else { f32::INFINITY };
        let ty = if dy.abs() > 1e-6 { hh / dy.abs() } else { f32::INFINITY };
        tx.min(ty)
    };
    let t = t.min(1.0);
    Point::new(c.x + dx * t, c.y + dy * t)
}

/// An edge's way: its ends on its shapes' borders (or its loose points), through its points,
/// at right angles when `orthogonal`.
fn route(
    source: Option<(Rect, bool)>,
    target: Option<(Rect, bool)>,
    points: &[Point],
    loose: (Option<Point>, Option<Point>),
    orthogonal: bool,
) -> Vec<Point> {
    let (src_c, tgt_c) =
        (source.map(|(r, _)| r.center()).or(loose.0), target.map(|(r, _)| r.center()).or(loose.1));
    let (Some(src_c), Some(tgt_c)) = (src_c, tgt_c) else { return Vec::new() };
    let first_toward = points.first().copied().unwrap_or(tgt_c);
    let last_from = points.last().copied().unwrap_or(src_c);
    if orthogonal
        && points.is_empty()
        && let (Some((s, _)), Some((t, _))) = (source, target)
    {
        // An elbow: out of the sides facing each other, across at the middle.
        let (dx, dy) = (tgt_c.x - src_c.x, tgt_c.y - src_c.y);
        if dx.abs() >= dy.abs() {
            let (a, b) = if dx >= 0.0 {
                (Point::new(s.max_x(), src_c.y), Point::new(t.min_x(), tgt_c.y))
            } else {
                (Point::new(s.min_x(), src_c.y), Point::new(t.max_x(), tgt_c.y))
            };
            let mid = (a.x + b.x) / 2.0;
            return dedup(vec![a, Point::new(mid, a.y), Point::new(mid, b.y), b]);
        }
        let (a, b) = if dy >= 0.0 {
            (Point::new(src_c.x, s.max_y()), Point::new(tgt_c.x, t.min_y()))
        } else {
            (Point::new(src_c.x, s.min_y()), Point::new(tgt_c.x, t.max_y()))
        };
        let mid = (a.y + b.y) / 2.0;
        return dedup(vec![a, Point::new(a.x, mid), Point::new(b.x, mid), b]);
    }
    let start = source.map_or(src_c, |(r, e)| border(r, e, first_toward));
    let end = target.map_or(tgt_c, |(r, e)| border(r, e, last_from));
    let mut way = vec![start];
    way.extend_from_slice(points);
    way.push(end);
    if orthogonal {
        // Between points not in line: a corner.
        let mut square = vec![way[0]];
        for pair in way.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if (a.x - b.x).abs() > 0.5 && (a.y - b.y).abs() > 0.5 {
                square.push(Point::new(b.x, a.y));
            }
            square.push(b);
        }
        way = square;
    }
    dedup(way)
}

fn dedup(mut points: Vec<Point>) -> Vec<Point> {
    points.dedup_by(|a, b| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01);
    points
}

/// The point at `t` (0…1) along `way`, and the direction there.
fn along(way: &[Point], t: f32) -> Option<Point> {
    let lengths: Vec<f32> =
        way.windows(2).map(|p| (p[1].x - p[0].x).hypot(p[1].y - p[0].y)).collect();
    let total: f32 = lengths.iter().sum();
    let first = *way.first()?;
    if total <= 0.0 {
        return Some(first);
    }
    let mut left = total * t.clamp(0.0, 1.0);
    for (pair, len) in way.windows(2).zip(&lengths) {
        if left <= *len && *len > 0.0 {
            let f = left / len;
            return Some(Point::new(
                pair[0].x + (pair[1].x - pair[0].x) * f,
                pair[0].y + (pair[1].y - pair[0].y) * f,
            ));
        }
        left -= len;
    }
    way.last().copied()
}

/// A marker at `tip`, coming from `from`: its outline, and where the line should stop.
fn marker(kind: Marker, from: Point, tip: Point, size: f32) -> Option<(Vec<Seg>, Point, bool)> {
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let len = dx.hypot(dy);
    if len <= 0.0 || kind == Marker::None {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy, ux);
    let length = size * 1.5;
    let half = size * 0.6;
    let back = |d: f32| Point::new(tip.x - ux * d, tip.y - uy * d);
    let side = |p: Point, s: f32| Point::new(p.x + nx * s, p.y + ny * s);
    let base = back(length);
    Some(match kind {
        Marker::Classic => {
            let notch = back(length * 0.75);
            (polygon(&[tip, side(base, half), notch, side(base, -half)]), notch, true)
        }
        Marker::Block => (polygon(&[tip, side(base, half), side(base, -half)]), base, true),
        Marker::Open => (
            vec![Seg::Move(side(base, half)), Seg::Line(tip), Seg::Line(side(base, -half))],
            tip,
            false,
        ),
        Marker::Oval => {
            let c = back(size / 2.0);
            let r = Rect::new(c.x - size / 2.0, c.y - size / 2.0, size, size);
            (vector_tiles::ellipse(r), back(size), true)
        }
        Marker::Diamond => {
            let mid = back(length);
            let end = back(length * 2.0);
            (polygon(&[tip, side(mid, half), end, side(mid, -half)]), end, true)
        }
        Marker::None => return None,
    })
}

/// `page` drawn with `theme`'s defaults.
pub fn draw(page: &Page, theme: &Theme) -> Drawing {
    let layout = Layout::new(page);
    let mut items: Vec<Item> = Vec::new();
    let mut texts: Vec<Text> = Vec::new();
    let mut boxes = Vec::new();
    let mut edge_ways = Vec::new();
    // Each edge's way (for the labels on it).
    let mut ways: HashMap<&str, Vec<Point>> = HashMap::new();
    for c in &page.cells {
        if !layout.shown(c) {
            continue;
        }
        match c.kind {
            CellKind::Vertex => {
                if let Some((r, look)) = layout.boxes.get(c.id.as_str()) {
                    shape(*r, look, theme, &mut items);
                    boxes.push((c.id.clone(), *r));
                    if !c.value.is_empty() {
                        let mut area = *r;
                        if let Some(h) = look.header {
                            area =
                                Rect::new(r.min_x(), r.min_y(), r.size.width, h.min(r.size.height));
                        }
                        // The default text on a fill of its own: dark on a light one (as
                        // draw.io's black), the theme's on a dark one.
                        let on = match look.fill {
                            crate::look::Paint::Color(fill) => Some(fill),
                            _ => None,
                        };
                        texts.push(text(c, look, area, theme, None, on));
                    }
                } else if let Some(g) = c.geometry.as_ref().filter(|g| g.relative)
                    && let Some(way) = c.parent.as_deref().and_then(|p| ways.get(p))
                {
                    // A label on its edge: at its place along it (-1…1), moved by its offset.
                    let look = look::look(&c.style, 0.0, 0.0);
                    if let Some(at) = along(way, ((g.x as f32) + 1.0) / 2.0) {
                        let off = g.offset.unwrap_or_default();
                        let at = Point::new(at.x + off.x as f32, at.y + g.y as f32 + off.y as f32);
                        let area = Rect::new(at.x, at.y, 0.0, 0.0);
                        texts.push(text(c, &look, area, theme, Some(theme.canvas), None));
                    }
                }
            }
            CellKind::Edge => {
                let style = look::edge_look(&c.style);
                let g = c.geometry.clone().unwrap_or_default();
                let (dx, dy) = layout.offset(c.parent.as_deref());
                let shift = |p: diagram_model::Point| Point::new(p.x as f32 + dx, p.y as f32 + dy);
                let end_of = |id: &Option<String>| {
                    id.as_deref()
                        .and_then(|i| layout.boxes.get(i))
                        .map(|(r, l)| (*r, l.shape == Some(ShapeKind::Ellipse)))
                };
                let points: Vec<Point> = g.points.iter().copied().map(shift).collect();
                let way = route(
                    end_of(&c.source),
                    end_of(&c.target),
                    &points,
                    (g.source_point.map(shift), g.target_point.map(shift)),
                    style.orthogonal,
                );
                if way.len() < 2 {
                    continue;
                }
                edge(&way, &style, theme, &mut items);
                edge_ways.push((c.id.clone(), way.clone()));
                if !c.value.is_empty() {
                    let look = look::look(&c.style, 0.0, 0.0);
                    if let Some(at) = along(&way, ((g.x as f32) + 1.0) / 2.0) {
                        let off = g.offset.unwrap_or_default();
                        let at = Point::new(at.x + off.x as f32, at.y + g.y as f32 + off.y as f32);
                        let area = Rect::new(at.x, at.y, 0.0, 0.0);
                        texts.push(text(c, &look, area, theme, Some(theme.canvas), None));
                    }
                }
                ways.insert(&c.id, way);
            }
            CellKind::Holder => {}
        }
    }
    finish(items, texts, boxes, edge_ways)
}

/// A shape's items.
pub(crate) fn shape(r: Rect, look: &Look, theme: &Theme, items: &mut Vec<Item>) {
    let Some(kind) = look.shape else {
        // A text with colors of its own: its box.
        let (fill, stroke) = (look.fill.or(theme.fill), look.stroke.or(theme.stroke));
        if fill.is_some() || stroke.is_some() {
            items.push(Item {
                segs: vector_tiles::rounded(r, 0.0),
                fill,
                line: stroke.map(|c| line(c, look.stroke_width, look.dashed)),
            });
        }
        return;
    };
    let o = outline(kind, r);
    let stroke = look.stroke.or(theme.stroke).filter(|_| look.stroke_width > 0.0);
    let line = stroke.map(|c| line(c, look.stroke_width, look.dashed));
    items.push(Item { segs: o.body, fill: look.fill.or(theme.fill), line });
    if !o.details.is_empty() && line.is_some() {
        items.push(Item { segs: o.details, fill: None, line });
    }
    if let (Some(h), Some(line)) = (look.header, line) {
        let y = r.min_y() + h.min(r.size.height);
        items.push(Item {
            segs: vec![Seg::Move(Point::new(r.min_x(), y)), Seg::Line(Point::new(r.max_x(), y))],
            fill: None,
            line: Some(line),
        });
    }
}

fn line(color: Rgba, width: f32, dashed: bool) -> Line {
    if dashed {
        Line { color, width, dash: Some((3.0 * width.max(1.0), 3.0 * width.max(1.0))) }
    } else {
        Line::solid(color, width)
    }
}

/// An edge's items: its line, its markers.
fn edge(way: &[Point], style: &EdgeLook, theme: &Theme, items: &mut Vec<Item>) {
    let Some(color) = style.stroke.or(theme.stroke) else { return };
    let mut way = way.to_vec();
    let n = way.len();
    let mut ends = Vec::new();
    for (kind, filled, from, tip, at) in [
        (style.end.0, style.end.1, way[n - 2], way[n - 1], n - 1),
        (style.start.0, style.start.1, way[1], way[0], 0),
    ] {
        if let Some((segs, stop, closed)) = marker(kind, from, tip, style.marker_size + style.width)
        {
            way[at] = stop;
            let fill = closed.then_some(if filled { color } else { theme.canvas });
            ends.push(Item { segs, fill, line: Some(Line::solid(color, style.width)) });
        }
    }
    let segs = if style.curved { smooth(&way) } else { polyline(&way) };
    items.push(Item { segs, fill: None, line: Some(line(color, style.width, style.dashed)) });
    items.extend(ends);
}

/// The text on a light fill, whatever the theme: near-black.
const DARK_INK: Rgba = Rgba::rgb(0x1a1a1a);

/// The default text's color over `fill`: dark on a light fill, the theme's otherwise.
fn readable_on(fill: Option<Rgba>, theme: &Theme) -> Rgba {
    match fill {
        Some(f) if 0.299 * f.r + 0.587 * f.g + 0.114 * f.b > 0.6 => DARK_INK,
        _ => theme.text,
    }
}

/// Straight lines through `way`.
fn polyline(way: &[Point]) -> Vec<Seg> {
    way.iter()
        .enumerate()
        .map(|(i, p)| if i == 0 { Seg::Move(*p) } else { Seg::Line(*p) })
        .collect()
}

/// A smooth curve through `way` (each piece a cubic whose ends follow its neighbours, as a
/// Catmull–Rom spline): draw.io's curved edges.
fn smooth(way: &[Point]) -> Vec<Seg> {
    if way.len() < 3 {
        return polyline(way);
    }
    let mut segs = vec![Seg::Move(way[0])];
    let at = |i: isize| way[i.clamp(0, way.len() as isize - 1) as usize];
    for i in 0..way.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
        let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);
        segs.push(Seg::Cubic(c1, c2, p2));
    }
    segs
}

/// A cell's text in `area` (an edge's: `area` its point), over a fill of its own `on`.
fn text(
    c: &Cell,
    look: &Look,
    area: Rect,
    theme: &Theme,
    backdrop: Option<Rgba>,
    on: Option<Rgba>,
) -> Text {
    let default = readable_on(on, theme);
    let text = label::plain(&c.value, look.html);
    let reach = (text.chars().count() as f32 * 1.3, text.split([' ', '\n']).count() as f32 * 1.2);
    Text {
        area,
        text,
        reach,
        size: look.font_size,
        color: look.font_color.or(default).unwrap_or(default),
        bold: look.bold,
        italic: look.italic,
        align: look.align,
        valign: look.valign,
        wrap: look.wrap && area.size.width > 0.0,
        backdrop,
    }
}

/// The box around it all; the items moved to be relative to its corner.
fn finish(
    mut items: Vec<Item>,
    texts: Vec<Text>,
    boxes: Vec<(String, Rect)>,
    ways: Vec<(String, Vec<Point>)>,
) -> Drawing {
    let (mut x0, mut y0, mut x1, mut y1) =
        (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut add = |p: Point| {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    };
    for item in &items {
        for s in &item.segs {
            match s {
                Seg::Move(p) | Seg::Line(p) => add(*p),
                Seg::Cubic(a, b, p) => {
                    add(*a);
                    add(*b);
                    add(*p);
                }
                Seg::Close => {}
            }
        }
    }
    for t in &texts {
        add(t.area.origin);
        add(Point::new(t.area.max_x(), t.area.max_y()));
    }
    if !x0.is_finite() {
        return Drawing { bounds: Rect::new(0.0, 0.0, 1.0, 1.0), items, texts, boxes, ways };
    }
    let bounds =
        Rect::new(x0 - MARGIN, y0 - MARGIN, x1 - x0 + 2.0 * MARGIN, y1 - y0 + 2.0 * MARGIN);
    let (ox, oy) = (bounds.min_x(), bounds.min_y());
    let shift = |p: &mut Point| {
        p.x -= ox;
        p.y -= oy;
    };
    for item in &mut items {
        for s in &mut item.segs {
            match s {
                Seg::Move(p) | Seg::Line(p) => shift(p),
                Seg::Cubic(a, b, p) => {
                    shift(a);
                    shift(b);
                    shift(p);
                }
                Seg::Close => {}
            }
        }
    }
    Drawing { bounds, items, texts, boxes, ways }
}

impl Drawing {
    /// Its items as the tiles take them (relative to its corner).
    pub fn shapes(&self) -> impl Iterator<Item = vector_tiles::Shape<'_>> {
        self.items.iter().map(|i| vector_tiles::Shape {
            segs: &i.segs,
            fill: i.fill,
            line: i.line.as_ref(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/drawing.rs"]
pub(crate) mod tests;
