//! A prepared chart laid out in a rectangle: its plot area, its axes' ticks, its legend and
//! each mark (a bar, a point, a slice, a line), all in the caller's pixels; and what's under
//! a point. Made when the data, the size or what's shown changes, never per frame: the
//! drawing reads it, the mouse searches it.

use kernel::{Point, Rect};

use crate::prepare::{Prepared, Shape};
use crate::scale::{self, Scale, Ticks};
use crate::spec::{ChartKind, Options};

/// Sizes a layout keeps (the caller's pixels, scaled).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// A label's line height.
    pub label_h: f32,
    /// The room around the plot and between its parts.
    pub pad: f32,
    /// A tick's length out of its axis.
    pub tick: f32,
    /// A point's radius (a line's dots, a scatter's).
    pub point: f32,
    /// The largest bubble's radius.
    pub bubble: f32,
    /// A legend's color swatch.
    pub swatch: f32,
}

impl Metrics {
    /// The usual sizes at `scale` (physical pixels per logical one).
    pub fn at(scale: f32) -> Self {
        Self {
            label_h: 16.0 * scale,
            pad: 8.0 * scale,
            tick: 4.0 * scale,
            point: 3.0 * scale,
            bubble: 22.0 * scale,
            swatch: 10.0 * scale,
        }
    }
}

/// What a layout is asked for, besides the data.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub area: Rect,
    pub options: Options,
    pub metrics: Metrics,
    /// Series hidden from the legend (by index; a missing one is shown).
    pub hidden: &'a [bool],
    /// The X range zoomed into (a measured X's values); `None`: all of it.
    pub zoom: Option<(f64, f64)>,
    /// Room kept for the axes' labels (off: the marks fill the area: a sparkline).
    pub axes: bool,
    /// A legend over the plot when there's more than one series (a pie's: its slices).
    pub legend: bool,
    /// The values' axis from and to these (a share: 0–100), whatever the values; `None`:
    /// from the values.
    pub y_range: Option<(f64, f64)>,
    /// Each series in a band of its own, one under the other, each on its own scale, along
    /// one X (a recording's CPU, its frames…): their names on the left, no values' axis.
    pub lanes: bool,
    /// How the values' axis's labels are written by whoever draws them (`50.0%`); `None`:
    /// as the ticks say. Their room is measured from what's written.
    pub y_label: Option<Written<'a>>,
}

/// How a value is written by whoever draws it.
#[derive(Clone, Copy)]
pub struct Written<'a>(pub &'a dyn Fn(f64) -> String);

impl std::fmt::Debug for Written<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Written")
    }
}

/// A tick on an axis: where (along it, pixels), its value and what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub at: f32,
    pub value: f64,
    pub label: String,
}

/// A mark's shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkShape {
    Bar(Rect),
    Point {
        center: Point,
        radius: f32,
    },
    /// From `start` to `end` (radians, clockwise from the top); `inner`: a ring's hole's
    /// radius (0: to the center).
    Slice {
        center: Point,
        radius: f32,
        inner: f32,
        start: f32,
        end: f32,
    },
}

/// A value drawn: of which series, which one in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub series: usize,
    pub index: usize,
    pub shape: MarkShape,
}

/// A series' line (and the area under it).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub series: usize,
    /// Its points, left to right; a gap (no value) splits it in runs.
    pub runs: Vec<Vec<Point>>,
    /// An area's lower edge per run (a stacked one's: the series under it); `None`: a line.
    pub floor: Option<Vec<Vec<Point>>>,
    pub stepped: bool,
}

/// A line's height at each category (`None`: no value there), for the dots under the mouse.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub series: usize,
    pub ys: Vec<Option<f32>>,
}

/// A legend's entry.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendItem {
    /// A series, or a pie's slice.
    pub index: usize,
    pub name: String,
    pub swatch: Rect,
    /// Where its name starts (its baseline's left).
    pub label: Point,
    /// The whole entry (a click toggles its series).
    pub area: Rect,
    pub hidden: bool,
}

/// What's under a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A bar, a point, a slice.
    Mark { series: usize, index: usize },
    /// A category's column (a line chart's: every series' value there).
    Slot(usize),
    /// A legend's entry.
    Legend(usize),
}

/// A chart laid out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    pub area: Rect,
    /// Where the marks go (inside the axes).
    pub plot: Rect,
    pub x_ticks: Vec<Tick>,
    pub y_ticks: Vec<Tick>,
    /// The values' zero line's height, when it's in view.
    pub zero: Option<f32>,
    pub marks: Vec<Mark>,
    pub lines: Vec<Line>,
    /// Each line's height at each category.
    pub levels: Vec<Level>,
    pub legend: Vec<LegendItem>,
    /// Each category's middle along X (a slot's for the mouse).
    pub slots: Vec<f32>,
    /// A slot's width.
    pub slot_width: f32,
    /// The measured X's scale (to zoom, to read the mouse's place).
    pub x_scale: Option<Scale>,
    /// In lanes: each shown series' band, its name drawn at its left (see [`View::lanes`]).
    pub bands: Vec<(usize, Rect)>,
}

/// The legend's room on top (what it takes from the area's height).
fn legend(
    names: &[(usize, String)],
    view: &View<'_>,
    measure: &mut dyn FnMut(&str) -> f32,
) -> (Vec<LegendItem>, f32) {
    let m = view.metrics;
    let area = view.area;
    let (mut x, mut y) = (area.min_x(), area.min_y());
    let mut items = Vec::with_capacity(names.len());
    for (index, name) in names {
        let w = m.swatch + m.pad * 0.6 + measure(name);
        if x > area.min_x() && x + w > area.max_x() {
            (x, y) = (area.min_x(), y + m.label_h + m.pad * 0.5);
        }
        let mid = y + m.label_h / 2.0;
        items.push(LegendItem {
            index: *index,
            name: name.clone(),
            swatch: Rect::new(x, mid - m.swatch / 2.0, m.swatch, m.swatch),
            label: Point::new(x + m.swatch + m.pad * 0.6, mid),
            area: Rect::new(x, y, w, m.label_h),
            hidden: view.hidden.get(*index).copied().unwrap_or(false),
        });
        x += w + m.pad * 2.0;
    }
    let used = if items.is_empty() { 0.0 } else { y + m.label_h + m.pad - area.min_y() };
    (items, used)
}

/// `p` laid out in `view` (`measure`: a label's width).
pub fn lay_out(p: &Prepared, view: &View<'_>, measure: &mut dyn FnMut(&str) -> f32) -> Geometry {
    let shown = |i: usize| !view.hidden.get(i).copied().unwrap_or(false);
    let names: Vec<(usize, String)> = match &p.shape {
        Shape::Slices { slices, rest } => slices
            .iter()
            .map(|s| s.name.clone())
            .chain(rest.map(|(n, _)| format!("+{n}")))
            .enumerate()
            .collect(),
        _ if p.series_count() > 1 => {
            (0..p.series_count()).map(|i| (i, p.series_name(i).to_owned())).collect()
        }
        _ => Vec::new(),
    };
    let names = if view.legend { names } else { Vec::new() };
    let (legend, legend_h) = legend(&names, view, measure);
    let mut g = Geometry { area: view.area, legend, ..Geometry::default() };
    let body = Rect::new(
        view.area.min_x(),
        view.area.min_y() + legend_h,
        view.area.size.width,
        (view.area.size.height - legend_h).max(0.0),
    );
    if let Shape::Slices { slices, rest } = &p.shape {
        let values = slices.iter().map(|s| s.value).chain(rest.map(|r| r.1));
        pie(&mut g, (body, view.options.hole), values, &shown);
        return g;
    }
    if view.lanes
        && let Shape::Categories { labels, xs, series } = &p.shape
    {
        lanes(&mut g, p, view, body, (labels, xs.as_deref(), series), measure);
        return g;
    }
    let y_ticks = match view.y_range {
        Some((lo, hi)) => scale::number_ticks(lo, hi, false),
        None => y_ticks(p, view.options, &shown),
    };
    let m = view.metrics;
    let plot = if view.axes {
        // As they'll be written (the owner's format wider than the ticks' own: `50.0%`).
        let y_label_w = match view.y_label {
            Some(Written(write)) => {
                y_ticks.values.iter().map(|v| measure(&write(*v))).fold(0.0f32, f32::max)
            }
            None => y_ticks.labels.iter().map(|l| measure(l)).fold(0.0f32, f32::max),
        };
        Rect::new(
            body.min_x() + y_label_w + m.tick + m.pad,
            body.min_y() + m.label_h / 2.0,
            (body.size.width - y_label_w - m.tick - m.pad * 2.0).max(1.0),
            (body.size.height - m.label_h * 1.5 - m.tick - m.pad).max(1.0),
        )
    } else {
        body
    };
    g.plot = plot;
    let y_of = |v: f64| plot.max_y() - (y_ticks.scale.unit(v) as f32) * plot.size.height;
    g.y_ticks = y_ticks
        .values
        .iter()
        .zip(&y_ticks.labels)
        .map(|(&v, l)| Tick { at: y_of(v), value: v, label: l.clone() })
        .collect();
    let zero = y_of(0.0);
    g.zero = (!y_ticks.scale.log && zero >= plot.min_y() - 0.5 && zero <= plot.max_y() + 0.5)
        .then_some(zero);
    let floor_y = g.zero.unwrap_or(plot.max_y());
    match &p.shape {
        Shape::Categories { labels, xs, series } => {
            categories(&mut g, p, view, (labels, xs.as_deref(), series), (&y_of, floor_y), measure);
        }
        Shape::Points(points) => {
            let xs = points.iter().flat_map(|s| s.points.iter().map(|q| q[0]));
            let ticks = x_ticks(xs, p.x_time, view.zoom);
            let x_of = place(&ticks.scale, plot);
            g.x_ticks = ticks_at(&ticks, &x_of);
            g.x_scale = Some(ticks.scale);
            let biggest =
                points.iter().flat_map(|s| s.points.iter().map(|q| q[2])).fold(0.0, f64::max);
            for (si, s) in points.iter().enumerate().filter(|(i, _)| shown(*i)) {
                for (i, q) in s.points.iter().enumerate() {
                    let center = Point::new(x_of(q[0]), y_of(q[1]));
                    if !plot.contains(center) {
                        continue;
                    }
                    let radius = if p.kind == ChartKind::Bubble && biggest > 0.0 {
                        (m.bubble * ((q[2].max(0.0) / biggest).sqrt() as f32)).max(m.point)
                    } else {
                        m.point
                    };
                    g.marks.push(Mark {
                        series: si,
                        index: i,
                        shape: MarkShape::Point { center, radius },
                    });
                }
            }
        }
        Shape::Bins { bins, .. } => {
            let lo = bins.first().map_or(0.0, |b| b.from);
            let hi = bins.last().map_or(1.0, |b| b.to);
            let ticks = x_ticks([lo, hi].into_iter(), p.y_time, None);
            let x_of = place(&Scale { lo, hi, log: false }, plot);
            g.x_ticks = ticks_at(&ticks, &x_of)
                .into_iter()
                .filter(|t| t.at >= plot.min_x() - 0.5 && t.at <= plot.max_x() + 0.5)
                .collect();
            let gap = m.point * 0.3;
            for (i, b) in bins.iter().enumerate() {
                let (x0, x1) = (x_of(b.from), x_of(b.to));
                let top = y_of(b.count);
                let rect = Rect::new(x0 + gap, top, (x1 - x0 - 2.0 * gap).max(1.0), floor_y - top);
                g.marks.push(Mark { series: 0, index: i, shape: MarkShape::Bar(rect) });
                g.slots.push((x0 + x1) / 2.0);
            }
            g.slot_width = plot.size.width / bins.len().max(1) as f32;
        }
        Shape::Slices { .. } => {}
    }
    g
}

/// The values' ticks: over every shown series (stacked: their sums; to 100%: 0–100).
fn y_ticks(p: &Prepared, options: Options, shown: &dyn Fn(usize) -> bool) -> Ticks {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut see = |v: f64| {
        lo = lo.min(v);
        hi = hi.max(v);
    };
    match &p.shape {
        Shape::Categories { labels, series, .. } => {
            if p.kind.stacked() {
                if options.percent {
                    return scale::number_ticks(0.0, 100.0, true);
                }
                for i in 0..labels.len() {
                    let (mut up, mut down) = (0.0, 0.0);
                    for s in series.iter().enumerate().filter(|(k, _)| shown(*k)).map(|(_, s)| s) {
                        match s.values[i] {
                            Some(v) if v >= 0.0 => up += v,
                            Some(v) => down += v,
                            None => {}
                        }
                    }
                    see(up);
                    see(down);
                }
            } else {
                for s in series.iter().enumerate().filter(|(k, _)| shown(*k)).map(|(_, s)| s) {
                    s.values.iter().flatten().for_each(|&v| see(v));
                }
            }
        }
        Shape::Points(points) => {
            for s in points.iter().enumerate().filter(|(k, _)| shown(*k)).map(|(_, s)| s) {
                s.points.iter().for_each(|q| see(q[1]));
            }
        }
        Shape::Bins { bins, .. } => bins.iter().for_each(|b| see(b.count)),
        Shape::Slices { .. } => {}
    }
    if options.log {
        return scale::log_ticks(lo, hi);
    }
    if p.y_time && p.kind.continuous_x() {
        return scale::time_ticks(lo, hi);
    }
    // Bars and areas stand on zero; lines and points only show what's there.
    let zero = !matches!(p.kind, ChartKind::Line | ChartKind::Scatter | ChartKind::Bubble);
    scale::number_ticks(lo, hi, zero)
}

/// A measured X's ticks over `values` (or the zoomed range).
fn x_ticks(values: impl Iterator<Item = f64>, time: bool, zoom: Option<(f64, f64)>) -> Ticks {
    let (lo, hi) = zoom.unwrap_or_else(|| {
        values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
    });
    let (lo, hi) = if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) };
    if time {
        return scale::time_ticks(lo, hi);
    }
    let mut ticks = scale::number_ticks(lo, hi, false);
    if zoom.is_some() || hi > lo {
        // The points fill the width: the scale is their range, its ticks those inside it.
        let keep: Vec<usize> = (0..ticks.values.len())
            .filter(|&i| ticks.values[i] >= lo && ticks.values[i] <= hi)
            .collect();
        ticks.values = keep.iter().map(|&i| ticks.values[i]).collect();
        ticks.labels = keep.iter().map(|&i| ticks.labels[i].clone()).collect();
        ticks.scale = Scale { lo, hi, log: false };
    }
    ticks
}

/// From a value to a place across `plot`.
fn place(s: &Scale, plot: Rect) -> impl Fn(f64) -> f32 + use<> {
    let (s, x0, w) = (*s, plot.min_x(), plot.size.width);
    move |v| x0 + s.unit(v) as f32 * w
}

fn ticks_at(t: &Ticks, x_of: &dyn Fn(f64) -> f32) -> Vec<Tick> {
    t.values
        .iter()
        .zip(&t.labels)
        .map(|(&v, l)| Tick { at: x_of(v), value: v, label: l.clone() })
        .collect()
}

/// Bars, lines and areas over categories.
fn categories(
    g: &mut Geometry,
    p: &Prepared,
    view: &View<'_>,
    (labels, xs, series): (&[String], Option<&[f64]>, &[crate::prepare::Values]),
    (y_of, floor_y): (&dyn Fn(f64) -> f32, f32),
    measure: &mut dyn FnMut(&str) -> f32,
) {
    let plot = g.plot;
    let m = view.metrics;
    let n = labels.len().max(1);
    let shown: Vec<usize> =
        (0..series.len()).filter(|&i| !view.hidden.get(i).copied().unwrap_or(false)).collect();
    // Where each category is along X.
    match xs {
        Some(xs) if p.kind.lined() => {
            let ticks = x_ticks(xs.iter().copied(), p.x_time, view.zoom);
            let x_of = place(&ticks.scale, plot);
            g.slots = xs.iter().map(|&x| x_of(x)).collect();
            g.x_ticks = ticks_at(&ticks, &x_of);
            g.x_scale = Some(ticks.scale);
            g.slot_width = plot.size.width / n as f32;
        }
        _ => {
            let w = plot.size.width / n as f32;
            g.slot_width = w;
            g.slots = (0..n).map(|i| plot.min_x() + (i as f32 + 0.5) * w).collect();
            // Every k-th label, so they don't run into each other.
            let widest = labels.iter().map(|l| measure(l)).fold(0.0f32, f32::max) + m.pad;
            let every = ((widest / w).ceil() as usize).max(1);
            g.x_ticks = labels
                .iter()
                .enumerate()
                .filter(|(i, _)| i % every == 0)
                .map(|(i, l)| Tick { at: g.slots[i], value: i as f64, label: l.clone() })
                .collect();
        }
    }
    let percent = view.options.percent && p.kind.stacked();
    let total = |i: usize| -> f64 {
        shown
            .iter()
            .filter_map(|&s| series[s].values[i])
            .map(f64::abs)
            .sum::<f64>()
            .max(f64::MIN_POSITIVE)
    };
    let value = |s: usize, i: usize| -> Option<f64> {
        let v = series[s].values[i]?;
        Some(if percent { v / total(i) * 100.0 } else { v })
    };
    match p.kind {
        ChartKind::Bar | ChartKind::StackedBar => {
            let group = g.slot_width * 0.8;
            let stacked = p.kind == ChartKind::StackedBar;
            let width = if stacked { group } else { group / shown.len().max(1) as f32 };
            let gap = if width > 6.0 { m.point * 0.3 } else { 0.0 };
            for i in 0..labels.len() {
                let left = g.slots[i] - group / 2.0;
                let (mut up, mut down) = (0.0, 0.0);
                for (k, &s) in shown.iter().enumerate() {
                    let Some(v) = value(s, i) else { continue };
                    let (from, to) = if stacked {
                        let base = if v >= 0.0 { &mut up } else { &mut down };
                        let from = *base;
                        *base += v;
                        (from, *base)
                    } else {
                        (0.0, v)
                    };
                    let (a, b) = (y_of(from), y_of(to));
                    let x = if stacked { left } else { left + k as f32 * width };
                    let rect =
                        Rect::new(x + gap / 2.0, a.min(b), (width - gap).max(1.0), (a - b).abs());
                    g.marks.push(Mark { series: s, index: i, shape: MarkShape::Bar(rect) });
                }
            }
        }
        _ => {
            // Lines and areas: a run of points per series, split where a value is missing.
            let stacked = p.kind == ChartKind::StackedArea;
            let mut under: Vec<f64> = vec![0.0; labels.len()];
            for &s in &shown {
                let mut ys: Vec<Option<f32>> = vec![None; labels.len()];
                let mut runs: Vec<Vec<Point>> = Vec::new();
                let mut floors: Vec<Vec<Point>> = Vec::new();
                let (mut run, mut floor) = (Vec::new(), Vec::new());
                for (i, below) in under.iter_mut().enumerate() {
                    let Some(v) = value(s, i) else {
                        if !run.is_empty() {
                            runs.push(std::mem::take(&mut run));
                            floors.push(std::mem::take(&mut floor));
                        }
                        continue;
                    };
                    let base = if stacked { *below } else { 0.0 };
                    let top = base + v;
                    if stacked {
                        *below = top;
                    }
                    let at = Point::new(g.slots[i], y_of(top));
                    ys[i] = Some(at.y);
                    run.push(at);
                    floor.push(Point::new(g.slots[i], if stacked { y_of(base) } else { floor_y }));
                    if view.options.points || p.kind == ChartKind::Line && labels.len() == 1 {
                        let shape = MarkShape::Point { center: at, radius: m.point };
                        g.marks.push(Mark { series: s, index: i, shape });
                    }
                }
                if !run.is_empty() {
                    runs.push(run);
                    floors.push(floor);
                }
                let runs = runs.into_iter().map(|r| thin(r, plot.size.width)).collect();
                let floor = (p.kind != ChartKind::Line)
                    .then(|| floors.into_iter().map(|f| thin(f, plot.size.width)).collect());
                g.lines.push(Line { series: s, runs, floor, stepped: view.options.stepped });
                g.levels.push(Level { series: s, ys });
            }
        }
    }
}

/// A run's points in steps (each value held until the next's place), or as they are: what
/// the screen draws and the export writes.
pub fn stepped(run: &[Point], steps: bool) -> Vec<Point> {
    if !steps || run.len() < 2 {
        return run.to_vec();
    }
    let mut out = Vec::with_capacity(run.len() * 2);
    for pair in run.windows(2) {
        out.push(pair[0]);
        out.push(Point::new(pair[1].x, pair[0].y));
    }
    out.extend(run.last());
    out
}

/// The gap between two lanes (times a label's height).
const LANE_GAP: f32 = 0.35;
/// How much of its band a lane's highest value reaches.
const LANE_FILL: f32 = 0.9;

/// Series in lanes (see [`View::lanes`]): the names' column on the left, each shown series
/// a band on its own scale (from zero to its highest value), areas along a shared X.
fn lanes(
    g: &mut Geometry,
    p: &Prepared,
    view: &View<'_>,
    body: Rect,
    (labels, xs, series): (&[String], Option<&[f64]>, &[crate::prepare::Values]),
    measure: &mut dyn FnMut(&str) -> f32,
) {
    let m = view.metrics;
    let shown: Vec<usize> =
        (0..series.len()).filter(|&i| !view.hidden.get(i).copied().unwrap_or(false)).collect();
    let names_w = shown.iter().map(|&s| measure(&series[s].name)).fold(0.0f32, f32::max);
    let plot = Rect::new(
        body.min_x() + names_w + m.pad * 2.0,
        body.min_y(),
        (body.size.width - names_w - m.pad * 3.0).max(1.0),
        (body.size.height - m.label_h - m.tick - m.pad).max(1.0),
    );
    g.plot = plot;
    let n = labels.len().max(1);
    match xs {
        Some(xs) => {
            let ticks = x_ticks(xs.iter().copied(), p.x_time, view.zoom);
            let x_of = place(&ticks.scale, plot);
            g.slots = xs.iter().map(|&x| x_of(x)).collect();
            g.x_ticks = ticks_at(&ticks, &x_of);
            g.x_scale = Some(ticks.scale);
        }
        None => {
            let w = plot.size.width / n as f32;
            g.slots = (0..n).map(|i| plot.min_x() + (i as f32 + 0.5) * w).collect();
        }
    }
    g.slot_width = plot.size.width / n as f32;
    let count = shown.len().max(1) as f32;
    let gap = m.label_h * LANE_GAP;
    let band_h = ((plot.size.height + gap) / count - gap).max(1.0);
    for (k, &s) in shown.iter().enumerate() {
        let band = Rect::new(
            plot.min_x(),
            plot.min_y() + k as f32 * (band_h + gap),
            plot.size.width,
            band_h,
        );
        g.bands.push((s, band));
        let values = &series[s].values;
        let top = values.iter().flatten().copied().fold(0.0f64, f64::max);
        let top = if top > 0.0 { scale::nice_step(top, 1).max(top) } else { 1.0 };
        let y_of =
            |v: f64| band.max_y() - (v / top).clamp(0.0, 1.0) as f32 * band.size.height * LANE_FILL;
        let mut ys = vec![None; labels.len()];
        let (mut runs, mut floors) = (Vec::new(), Vec::new());
        let (mut run, mut floor) = (Vec::new(), Vec::new());
        for (i, v) in values.iter().enumerate() {
            let Some(v) = *v else {
                if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                    floors.push(std::mem::take(&mut floor));
                }
                continue;
            };
            let Some(&x) = g.slots.get(i) else { continue };
            let at = Point::new(x, y_of(v));
            ys[i] = Some(at.y);
            run.push(at);
            floor.push(Point::new(x, band.max_y()));
        }
        if !run.is_empty() {
            runs.push(run);
            floors.push(floor);
        }
        let runs = runs.into_iter().map(|r| thin(r, plot.size.width)).collect();
        let floors = floors.into_iter().map(|f| thin(f, plot.size.width)).collect();
        g.lines.push(Line { series: s, runs, floor: Some(floors), stepped: view.options.stepped });
        g.levels.push(Level { series: s, ys });
    }
}

/// A run with more points than twice the pixels across: a point at each column's lowest and
/// highest (a spike stays), in order.
fn thin(run: Vec<Point>, width: f32) -> Vec<Point> {
    let columns = (width.max(1.0) * 2.0) as usize;
    if run.len() <= columns * 2 {
        return run;
    }
    let mut out: Vec<Point> = Vec::with_capacity(columns * 2);
    let mut column: Option<(i64, Point, Point)> = None;
    for q in run {
        let c = q.x.floor() as i64;
        match &mut column {
            Some((at, low, high)) if *at == c => {
                if q.y < low.y {
                    *low = q;
                }
                if q.y > high.y {
                    *high = q;
                }
            }
            _ => {
                if let Some((_, low, high)) = column.take() {
                    push_pair(&mut out, low, high);
                }
                column = Some((c, q, q));
            }
        }
    }
    if let Some((_, low, high)) = column {
        push_pair(&mut out, low, high);
    }
    out
}

fn push_pair(out: &mut Vec<Point>, a: Point, b: Point) {
    let (first, second) = if a.x <= b.x { (a, b) } else { (b, a) };
    out.push(first);
    if second != first {
        out.push(second);
    }
}

/// A pie's slices in the middle of `body`, clockwise from the top.
fn pie(
    g: &mut Geometry,
    (body, hole): (Rect, u8),
    values: impl Iterator<Item = f64>,
    shown: &dyn Fn(usize) -> bool,
) {
    let values: Vec<(usize, f64)> = values.enumerate().filter(|(i, _)| shown(*i)).collect();
    let total: f64 = values.iter().map(|v| v.1).sum();
    let radius = (body.size.width.min(body.size.height) / 2.0 - 4.0).max(1.0);
    let center = body.center();
    let inner = radius * f32::from(hole.min(crate::spec::MOST_HOLE)) / 100.0;
    g.plot = Rect::new(center.x - radius, center.y - radius, radius * 2.0, radius * 2.0);
    if total <= 0.0 {
        return;
    }
    let mut start = 0.0f32;
    for (i, v) in values {
        let end = start + (v / total) as f32 * std::f32::consts::TAU;
        g.marks.push(Mark {
            series: 0,
            index: i,
            shape: MarkShape::Slice { center, radius, inner, start, end },
        });
        start = end;
    }
}

impl Geometry {
    /// What's at `p`: a legend's entry, a mark (the nearest point within its reach, the
    /// topmost bar, the slice), else a category's column inside the plot.
    pub fn hit(&self, p: Point) -> Option<Hit> {
        if let Some(item) = self.legend.iter().find(|l| l.area.contains(p)) {
            return Some(Hit::Legend(item.index));
        }
        let mut nearest: Option<(f32, Hit)> = None;
        for mark in self.marks.iter().rev() {
            let hit = Hit::Mark { series: mark.series, index: mark.index };
            match mark.shape {
                MarkShape::Bar(r) if r.contains(p) => return Some(hit),
                MarkShape::Slice { center, radius, inner, start, end } => {
                    let (dx, dy) = (p.x - center.x, p.y - center.y);
                    let d2 = dx * dx + dy * dy;
                    // On the ring (a ring's hole is nothing's).
                    if d2 <= radius * radius && d2 >= inner * inner {
                        // Clockwise from the top.
                        let a = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU);
                        if a >= start && a < end {
                            return Some(hit);
                        }
                    }
                }
                MarkShape::Point { center, radius } => {
                    let d = ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt();
                    let reach = radius + 4.0;
                    if d <= reach && nearest.is_none_or(|(best, _)| d < best) {
                        nearest = Some((d, hit));
                    }
                }
                MarkShape::Bar(_) => {}
            }
        }
        if let Some((_, hit)) = nearest {
            return Some(hit);
        }
        if !self.plot.contains(p) || self.slots.is_empty() {
            return None;
        }
        // The category whose middle is nearest along X.
        let i = self.slots.partition_point(|&x| x < p.x);
        let pick = match (i.checked_sub(1), self.slots.get(i)) {
            (Some(a), Some(&bx)) if p.x - self.slots[a] <= bx - p.x => a,
            (Some(a), None) => a,
            _ => i,
        };
        Some(Hit::Slot(pick))
    }
}

#[cfg(test)]
#[path = "../tests/unit/geometry.rs"]
mod tests;
