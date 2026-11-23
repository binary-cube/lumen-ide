//! What a chart can't draw from quads (the renderer draws no diagonal): its lines, the areas
//! under them and a pie's slices, rasterized smooth (`vector-tiles`) on a worker into alpha
//! masks in the theme's colors. Made again only when the layout or the colors change; while
//! a new one is made the last one stays drawn. A pie's slices are made one by one, so the
//! one under the mouse can be drawn moved out without making anything again.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, LazyLock};

use charts_model::geometry::{Geometry, Line as ChartLine, MarkShape, stepped};
use kernel::{Point, Rect, Rgba};
use render_api::ImageData;
use vector_tiles::{Line, Seg, Shape};

use super::look::ChartLook;

/// The charts' rasterizing, shared by all.
static RASTER: LazyLock<kernel::Pool> =
    LazyLock::new(|| kernel::Pool::new("chart-raster", 2, kernel::work::IDLE));

type Wake = Arc<dyn Fn() + Send + Sync>;

/// A mask and where it goes from the chart's corner; a pie's slice says which.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub slice: Option<usize>,
    pub image: ImageData,
    pub at: Rect,
}

/// A shape to rasterize, owned (sent to the worker).
struct Owned {
    segs: Vec<Seg>,
    fill: Option<Rgba>,
    line: Option<Line>,
    slice: Option<usize>,
}

/// A chart's masks: the ones drawn, the ones being made.
#[derive(Default)]
pub(crate) struct Raster {
    made: Option<(u64, Arc<Vec<Piece>>)>,
    waiting: Option<(u64, Receiver<Vec<Piece>>)>,
    waker: Option<Wake>,
}

impl std::fmt::Debug for Raster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Raster").field("busy", &self.waiting.is_some()).finish_non_exhaustive()
    }
}

impl Raster {
    pub fn set_waker(&mut self, waker: Option<Wake>) {
        if waker.is_some() {
            self.waker = waker;
        }
    }

    /// The masks to draw (the last made).
    pub fn pieces(&self) -> Option<Arc<Vec<Piece>>> {
        self.made.as_ref().map(|(_, p)| Arc::clone(p))
    }

    /// A new layout (`key`: its and the colors' fingerprint): its masks asked of the worker
    /// unless they're the ones made or being made. A layout with nothing to rasterize
    /// (bars, points) lets the masks go.
    pub fn ask(&mut self, key: u64, g: &Geometry, look: &ChartLook, stacked: bool) {
        if self.made.as_ref().is_some_and(|m| m.0 == key)
            || self.waiting.as_ref().is_some_and(|w| w.0 == key)
        {
            return;
        }
        let shapes = shapes(g, look, stacked);
        if shapes.is_empty() {
            self.made = Some((key, Arc::new(Vec::new())));
            self.waiting = None;
            return;
        }
        let size = (g.area.size.width, g.area.size.height);
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.waker.clone();
        RASTER.spawn(kernel::Priority::Interactive, None, move || {
            let _ = tx.send(rasterize(&shapes, size));
            if let Some(wake) = wake {
                wake();
            }
        });
        self.waiting = Some((key, rx));
    }

    /// The masks being made taken once ready: `true` when they were (draw again).
    pub fn poll(&mut self) -> bool {
        let Some((key, rx)) = &self.waiting else { return false };
        match rx.try_recv() {
            Ok(pieces) => {
                self.made = Some((*key, Arc::new(pieces)));
                self.waiting = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.waiting = None;
                false
            }
        }
    }

    /// Masks are being made.
    pub fn busy(&self) -> bool {
        self.waiting.is_some()
    }

    /// Everything let go (the chart hidden).
    pub fn clear(&mut self) {
        self.made = None;
        self.waiting = None;
    }
}

/// What a layout rasterizes, from the chart's corner: areas, then lines over them, then a
/// pie's slices (each parted from the next by the background).
fn shapes(g: &Geometry, look: &ChartLook, stacked: bool) -> Vec<Owned> {
    let o = g.area.origin;
    let local = |p: &Point| Point::new(p.x - o.x, p.y - o.y);
    let mut out = Vec::new();
    for line in &g.lines {
        if let Some(floors) = &line.floor {
            for (run, floor) in line.runs.iter().zip(floors) {
                let mut points: Vec<Point> = stepped(run, line.stepped).iter().map(local).collect();
                points.extend(stepped(floor, line.stepped).iter().rev().map(local));
                if points.len() >= 3 {
                    let fill = Some(look.area(line.series, stacked));
                    out.push(Owned {
                        segs: vector_tiles::polygon(&points),
                        fill,
                        line: None,
                        slice: None,
                    });
                }
            }
        }
    }
    for line in &g.lines {
        out.extend(strokes(line, look, &local));
    }
    for mark in &g.marks {
        if let MarkShape::Slice { center, radius, inner, start, end } = mark.shape {
            let segs = slice(local(&center), (radius, inner), start, end);
            // The edge between slices; a whole turn has no other to part from (its edge
            // would show as a seam at the top).
            let line = (!super::whole_turn(start, end))
                .then(|| Line::solid(look.background, look.line_width));
            out.push(Owned {
                segs,
                fill: Some(look.series(mark.index)),
                line,
                slice: Some(mark.index),
            });
        }
    }
    out
}

/// A series' line, a run at a time.
fn strokes(line: &ChartLine, look: &ChartLook, local: &dyn Fn(&Point) -> Point) -> Vec<Owned> {
    let color = look.series(line.series);
    line.runs
        .iter()
        .filter(|run| run.len() >= 2)
        .map(|run| {
            let points: Vec<Point> = stepped(run, line.stepped).iter().map(local).collect();
            let mut segs = Vec::with_capacity(points.len());
            segs.push(Seg::Move(points[0]));
            segs.extend(points[1..].iter().map(|&p| Seg::Line(p)));
            Owned { segs, fill: None, line: Some(Line::solid(color, look.line_width)), slice: None }
        })
        .collect()
}

/// A pie's slice from `start` to `end` (radians clockwise from the top) as a closed path:
/// the arc in cubic pieces of at most a quarter turn.
/// A slice from the center (`inner` 0), or a ring's piece between `inner` and `radius`
/// (its inner arc drawn back, the other way round: a whole ring keeps its hole).
fn slice(center: Point, (radius, inner): (f32, f32), start: f32, end: f32) -> Vec<Seg> {
    let at = |r: f32, a: f32| Point::new(center.x + r * a.sin(), center.y - r * a.cos());
    let mut segs = Vec::new();
    if inner > 0.0 {
        segs.push(Seg::Move(at(radius, start)));
        arc(&mut segs, center, radius, (start, end));
        segs.push(Seg::Line(at(inner, end)));
        arc(&mut segs, center, inner, (end, start));
    } else {
        segs.push(Seg::Move(center));
        segs.push(Seg::Line(at(radius, start)));
        arc(&mut segs, center, radius, (start, end));
    }
    segs.push(Seg::Close);
    segs
}

/// The arc of `radius` from angle `from` to `to` (clockwise when `to` is past `from`), in
/// cubic pieces of at most a quarter turn.
fn arc(segs: &mut Vec<Seg>, center: Point, radius: f32, (from, to): (f32, f32)) {
    let at = |a: f32| Point::new(center.x + radius * a.sin(), center.y - radius * a.cos());
    let pieces = ((to - from).abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = (to - from) / pieces as f32;
    // The handles' length for an arc of `step` (negative: drawn back).
    let k = 4.0 / 3.0 * (step / 4.0).tan() * radius;
    for i in 0..pieces {
        let (a0, a1) = (from + i as f32 * step, from + (i + 1) as f32 * step);
        let (p0, p1) = (at(a0), at(a1));
        // The tangents, clockwise.
        let t0 = Point::new(a0.cos(), a0.sin());
        let t1 = Point::new(a1.cos(), a1.sin());
        segs.push(Seg::Cubic(
            Point::new(p0.x + t0.x * k, p0.y + t0.y * k),
            Point::new(p1.x - t1.x * k, p1.y - t1.y * k),
            p1,
        ));
    }
}

/// The shapes rasterized (a scene the chart's size; a pie's slices one at a time), their
/// masks placed from the chart's corner.
fn rasterize(shapes: &[Owned], size: (f32, f32)) -> Vec<Piece> {
    let (across, down) = vector_tiles::grid(size, 1.0);
    let mut out = Vec::new();
    let mut run = |group: &[&Owned], slice: Option<usize>| {
        for ty in 0..down {
            for tx in 0..across {
                let shapes = group.iter().map(|s| Shape {
                    segs: &s.segs,
                    fill: s.fill,
                    line: s.line.as_ref(),
                });
                for (image, at) in vector_tiles::tile(size, shapes, 1.0, tx, ty) {
                    out.push(Piece { slice, image, at });
                }
            }
        }
    };
    let (slices, rest): (Vec<&Owned>, Vec<&Owned>) = shapes.iter().partition(|s| s.slice.is_some());
    if !rest.is_empty() {
        run(&rest, None);
    }
    for s in slices {
        run(&[s], s.slice);
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/charts/raster.rs"]
mod tests;
