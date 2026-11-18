//! The diagrams that are charts of values: a user journey (sections, tasks and how each
//! felt), a sankey (flows between columns of nodes), an xy chart (bars and lines on two
//! axes), a quadrant chart (points on a 2×2), a radar (curves over axes around a center),
//! a packet (fields over a bit grid) and a treemap (nested shares as rectangles).

use diagram_text::mermaid::journey::Journey;
use diagram_text::mermaid::packet::Packet;
use diagram_text::mermaid::quadrant::Quadrant;
use diagram_text::mermaid::radar::Radar;
use diagram_text::mermaid::sankey::Sankey;
use diagram_text::mermaid::tree::Treemap;
use diagram_text::mermaid::xychart::{Series, XyChart};
use kernel::{Point, Rect, Rgba};

use super::charts::{arc, text};
use super::scene::{self, Anchor, Line, Scene, Seg};
use super::{Look, Measure, label_texts, text_size};

fn series(look: &Look, i: usize) -> Rgba {
    look.series[i % look.series.len()]
}

/// A number as short as it reads (`12`, `2.5`).
fn number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// About `count` round steps over `lo..hi`: the step and the first tick.
fn ticks(lo: f64, hi: f64, count: usize) -> Vec<f64> {
    let span = (hi - lo).abs().max(f64::EPSILON);
    let raw = span / count.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step =
        [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(raw);
    let mut out = Vec::new();
    let mut t = (lo / step).ceil() * step;
    while t <= hi + step * 1e-9 && out.len() < 64 {
        out.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
    }
    out
}

/// A rectangle between two corners, in any order.
fn between(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
}

fn circle(scene: &mut Scene, c: Point, r: f32, fill: Option<Rgba>, line: Option<Line>) {
    scene.path(scene::ellipse(Rect::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r)), fill, line);
}

// ---- journey ----

pub fn journey(j: &Journey, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let small = size * 0.85;
    let mut scene = Scene::default();
    // The actors' legend, on the left.
    let actor_color = |a: &str| {
        let i = j.actors.iter().position(|x| x == a).unwrap_or(0);
        series(look, i + 2)
    };
    let row = size * 1.6;
    let mut legend_w: f32 = 0.0;
    for (i, a) in j.actors.iter().enumerate() {
        let y = 40.0 + row * i as f32;
        circle(&mut scene, Point::new(8.0, y), 6.0, Some(actor_color(a)), None);
        scene.text(text(Point::new(20.0, y), a, small, look.text, Anchor::Start, false));
        legend_w = legend_w.max(20.0 + text_size(measure, a, small, false).0);
    }
    let left = if j.actors.is_empty() { 0.0 } else { legend_w + 24.0 };
    let col = j
        .tasks
        .iter()
        .map(|t| text_size(measure, &t.name, size, false).0 + 24.0)
        .fold(120.0, f32::max)
        .min(220.0);
    let gap = 12.0;
    let x_of = |i: usize| left + i as f32 * (col + gap);
    let band_h = size * 2.0;
    let card_y = band_h + 10.0;
    let card_h = size * 3.2;
    // The sections, over their tasks.
    for (s, name) in j.sections.iter().enumerate() {
        let tasks: Vec<usize> = j
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.section == Some(s))
            .map(|(i, _)| i)
            .collect();
        let (Some(&a), Some(&b)) = (tasks.first(), tasks.last()) else { continue };
        let r = Rect::new(x_of(a), 0.0, x_of(b) + col - x_of(a), band_h);
        let color = series(look, s);
        scene.rect(r, 4.0, Some(color.mix(look.background, 0.35)), None);
        label_texts(&mut scene, name, r.center(), size, look.background, true);
    }
    let faces_top = card_y + card_h + 24.0;
    let step = 22.0;
    let faces_bottom = faces_top + 4.0 * step;
    for (i, t) in j.tasks.iter().enumerate() {
        let x = x_of(i);
        let color = t.section.map_or(look.accent, |s| series(look, s));
        let card = Rect::new(x, card_y, col, card_h);
        let label = super::charts::wrap(measure, &t.name, size, col - 16.0);
        scene.rect(
            card,
            6.0,
            Some(look.fill),
            Some(Line::solid(color.mix(look.background, 0.2), 1.2)),
        );
        label_texts(
            &mut scene,
            &label,
            Point::new(card.center().x, card.center().y - 4.0),
            size,
            look.text,
            false,
        );
        // Who's in it: dots under its text.
        let n = t.actors.len() as f32;
        for (k, a) in t.actors.iter().enumerate() {
            let cx = card.center().x - (n - 1.0) * 7.0 + k as f32 * 14.0;
            circle(&mut scene, Point::new(cx, card.max_y() - 9.0), 4.5, Some(actor_color(a)), None);
        }
        // How it felt: a face, higher the better.
        let score = t.score.clamp(1, 5);
        let cy = faces_top + f32::from(5 - score) * step;
        let cx = card.center().x;
        scene.polyline(
            &[Point::new(cx, card.max_y()), Point::new(cx, cy - 14.0)],
            Line { color: look.line.with_alpha(0.6), width: 1.0, dash: Some((3.0, 3.0)) },
        );
        face(&mut scene, Point::new(cx, cy), score, look);
    }
    let right = x_of(j.tasks.len().max(1)) - gap;
    // The time axis, under the faces.
    let axis_y = faces_bottom + 26.0;
    let line = Line::solid(look.line, 1.5);
    scene.polyline(&[Point::new(left, axis_y), Point::new(right, axis_y)], line);
    scene::head(
        &mut scene,
        Point::new(left, axis_y),
        Point::new(right + 8.0, axis_y),
        scene::HeadKind::Arrow,
        9.0,
        look.line,
        look.background,
    );
    scene.width = right + 12.0;
    scene.height = axis_y + 8.0;
    scene
}

/// A face for a score: smiling (4–5), flat (3), frowning (1–2).
fn face(scene: &mut Scene, c: Point, score: u8, look: &Look) {
    let color = match score {
        4.. => series(look, 1),
        3 => series(look, 3),
        _ => series(look, 5),
    };
    circle(scene, c, 13.0, Some(color.mix(look.background, 0.25)), Some(Line::solid(color, 1.2)));
    let ink = look.background;
    circle(scene, Point::new(c.x - 4.5, c.y - 3.5), 1.8, Some(ink), None);
    circle(scene, Point::new(c.x + 4.5, c.y - 3.5), 1.8, Some(ink), None);
    let pi = std::f32::consts::PI;
    let mouth = Line::solid(ink, 1.6);
    let (center, a0, a1) = match score {
        4.. => (Point::new(c.x, c.y), 0.2 * pi, 0.8 * pi),
        3 => {
            scene.polyline(
                &[Point::new(c.x - 5.0, c.y + 5.0), Point::new(c.x + 5.0, c.y + 5.0)],
                mouth,
            );
            return;
        }
        _ => (Point::new(c.x, c.y + 10.0), 1.2 * pi, 1.8 * pi),
    };
    let r = 6.5;
    let mut segs = vec![Seg::Move(Point::new(center.x + r * a0.cos(), center.y + r * a0.sin()))];
    arc(center, r, a0, a1, &mut segs);
    scene.path(segs, None, Some(mouth));
}

// ---- sankey ----

pub fn sankey(s: &Sankey, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size * 0.9;
    let mut scene = Scene::default();
    let n = s.nodes.len();
    if n == 0 {
        return scene;
    }
    // Each node's column: the longest way to it from a source (cycles cut short).
    let mut depth = vec![0usize; n];
    for _ in 0..n {
        let mut changed = false;
        for &(a, b, _) in &s.links {
            if a != b && depth[b] < depth[a] + 1 && depth[a] + 1 < n {
                depth[b] = depth[a] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Sinks to the last column.
    let last = depth.iter().copied().max().unwrap_or(0);
    for (i, d) in depth.iter_mut().enumerate() {
        if !s.links.iter().any(|&(a, _, _)| a == i) && s.links.iter().any(|&(_, b, _)| b == i) {
            *d = last;
        }
    }
    let value: Vec<f64> = (0..n)
        .map(|i| {
            let ins: f64 = s.links.iter().filter(|l| l.1 == i).map(|l| l.2).sum();
            let outs: f64 = s.links.iter().filter(|l| l.0 == i).map(|l| l.2).sum();
            ins.max(outs)
        })
        .collect();
    let height = 420.0f32;
    let pad = 12.0f32;
    let columns: Vec<Vec<usize>> =
        (0..=last).map(|c| (0..n).filter(|&i| depth[i] == c).collect()).collect();
    let k = columns
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| {
            let total: f64 = c.iter().map(|&i| value[i]).sum();
            (height - pad * (c.len() as f32 - 1.0)).max(40.0) / total.max(f64::EPSILON) as f32
        })
        .fold(f32::INFINITY, f32::min);
    let k = if k.is_finite() { k } else { 1.0 };
    let node_w = 14.0;
    let label_w = s
        .nodes
        .iter()
        .enumerate()
        .map(|(i, name)| text_size(measure, &format!("{name} {}", number(value[i])), size, false).0)
        .fold(0.0, f32::max);
    let gap = (label_w + 60.0).max(160.0);
    // Each column ordered by where its nodes' flows come from (fewer crossings).
    let mut columns = columns;
    let mut rank: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
    for col in columns.iter_mut() {
        let center = |i: usize| {
            let (sum, weight) = s
                .links
                .iter()
                .filter(|l| l.1 == i)
                .fold((0.0, 0.0), |(a, w), l| (a + rank[l.0] * l.2 as f32, w + l.2 as f32));
            if weight > 0.0 { sum / weight } else { rank[i] }
        };
        let keys: Vec<(usize, f32)> = col.iter().map(|&i| (i, center(i))).collect();
        let mut keys = keys;
        keys.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        *col = keys.iter().map(|k| k.0).collect();
        let len = col.len() as f32;
        for (pos, &i) in col.iter().enumerate() {
            rank[i] = (pos as f32 + 0.5) / len;
        }
    }
    let mut rects = vec![Rect::default(); n];
    for (c, col) in columns.iter().enumerate() {
        let used: f32 = col.iter().map(|&i| value[i] as f32 * k).sum::<f32>()
            + pad * (col.len() as f32 - 1.0).max(0.0);
        let mut y = (height - used) / 2.0;
        for &i in col {
            let h = (value[i] as f32 * k).max(1.0);
            rects[i] = Rect::new(c as f32 * gap, y, node_w, h);
            y += h + pad;
        }
    }
    // The ribbons: each link leaves its source below the one before and enters its
    // target the same way, sorted so they don't cross needlessly.
    let mut out_at: Vec<f32> = rects.iter().map(|r| r.min_y()).collect();
    let mut in_at = out_at.clone();
    let mut order: Vec<usize> = (0..s.links.len()).collect();
    order.sort_by(|&a, &b| {
        let (la, lb) = (s.links[a], s.links[b]);
        (la.0, rects[la.1].min_y())
            .partial_cmp(&(lb.0, rects[lb.1].min_y()))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut ribbons = vec![(0.0f32, 0.0f32); s.links.len()];
    for &l in &order {
        let (a, _, v) = s.links[l];
        ribbons[l].0 = out_at[a];
        out_at[a] += v as f32 * k;
    }
    order.sort_by(|&a, &b| {
        let (la, lb) = (s.links[a], s.links[b]);
        (la.1, rects[la.0].min_y())
            .partial_cmp(&(lb.1, rects[lb.0].min_y()))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for &l in &order {
        let (_, b, v) = s.links[l];
        ribbons[l].1 = in_at[b];
        in_at[b] += v as f32 * k;
    }
    for (l, &(a, b, v)) in s.links.iter().enumerate() {
        let t = v as f32 * k;
        let (x0, x1) = (rects[a].max_x(), rects[b].min_x());
        let (y0, y1) = ribbons[l];
        let mid = (x0 + x1) / 2.0;
        let segs = vec![
            Seg::Move(Point::new(x0, y0)),
            Seg::Cubic(Point::new(mid, y0), Point::new(mid, y1), Point::new(x1, y1)),
            Seg::Line(Point::new(x1, y1 + t)),
            Seg::Cubic(Point::new(mid, y1 + t), Point::new(mid, y0 + t), Point::new(x0, y0 + t)),
            Seg::Close,
        ];
        scene.path(segs, Some(series(look, a).with_alpha(0.35)), None);
    }
    let mut right: f32 = 0.0;
    for (i, r) in rects.iter().enumerate() {
        scene.rect(*r, 1.0, Some(series(look, i)), None);
        let label = format!("{} {}", s.nodes[i], number(value[i]));
        let (at, anchor) = if depth[i] == last && last > 0 {
            (Point::new(r.min_x() - 6.0, r.center().y), Anchor::End)
        } else {
            (Point::new(r.max_x() + 6.0, r.center().y), Anchor::Start)
        };
        scene.text(text(at, &label, size, look.text, anchor, false));
        right = right.max(r.max_x());
    }
    if last == 0 {
        right += label_w + 8.0;
    }
    scene.width = right;
    scene.height = height;
    scene
}

// ---- xy chart ----

pub fn xychart(c: &XyChart, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size * 0.88;
    let mut scene = Scene::default();
    let count = c.series.iter().map(|s| match s {
        Series::Bar(v) | Series::Line(v) => v.len(),
    });
    let count = count.max().unwrap_or(0).max(c.categories.len()).max(1);
    // The categories: named, or numbers over the x range.
    let categories: Vec<String> = if !c.categories.is_empty() {
        c.categories.clone()
    } else if let Some((a, b)) = c.x_range {
        (0..count).map(|i| number(a + (b - a) * i as f64 / (count.max(2) - 1) as f64)).collect()
    } else {
        (1..=count).map(|i| i.to_string()).collect()
    };
    let values = c.series.iter().flat_map(|s| match s {
        Series::Bar(v) | Series::Line(v) => v.iter().copied(),
    });
    let (min, max) =
        values.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
    let (lo, hi) = c.y_range.unwrap_or_else(|| {
        if !min.is_finite() {
            (0.0, 1.0)
        } else {
            let lo = min.min(0.0);
            let hi = if max > lo { max + (max - lo) * 0.08 } else { lo + 1.0 };
            (lo, hi)
        }
    });
    let ticks = ticks(lo, hi, 6);
    let tick_w =
        ticks.iter().map(|t| text_size(measure, &number(*t), size, false).0).fold(0.0, f32::max);
    let cat_w = categories.iter().map(|t| text_size(measure, t, size, false).0).fold(0.0, f32::max);
    let line_h = size * 1.3;
    let top = if c.y_title.is_some() { line_h + 10.0 } else { 6.0 };
    let (plot_w, plot_h) = if c.horizontal {
        (560.0, (count as f32 * 34.0).clamp(200.0, 520.0))
    } else {
        ((count as f32 * 56.0).clamp(320.0, 760.0), 300.0)
    };
    let left = if c.horizontal { cat_w } else { tick_w } + 12.0;
    let plot = Rect::new(left, top, plot_w, plot_h);
    let band = if c.horizontal { plot_h } else { plot_w } / count as f32;
    let span = (hi - lo).max(f64::EPSILON);
    // A place on the chart: along the categories (pixels from their axis' start), at a value.
    let at = |along: f32, v: f64| {
        let share = ((v - lo) / span) as f32;
        if c.horizontal {
            Point::new(plot.min_x() + share * plot_w, plot.min_y() + along)
        } else {
            Point::new(plot.min_x() + along, plot.max_y() - share * plot_h)
        }
    };
    // The grid and the value axis' ticks.
    let grid = Line::solid(look.line.with_alpha(0.25), 1.0);
    for t in &ticks {
        let (a, b) = (at(0.0, *t), at(count as f32 * band, *t));
        scene.polyline(&[a, b], grid);
        let label = number(*t);
        if c.horizontal {
            scene.text(text(
                Point::new(a.x, plot.max_y() + line_h * 0.8),
                &label,
                size,
                look.muted,
                Anchor::Middle,
                false,
            ));
        } else {
            scene.text(text(
                Point::new(a.x - 6.0, a.y),
                &label,
                size,
                look.muted,
                Anchor::End,
                false,
            ));
        }
    }
    let axis = Line::solid(look.line, 1.3);
    scene.polyline(
        &[
            Point::new(plot.min_x(), plot.min_y()),
            Point::new(plot.min_x(), plot.max_y()),
            Point::new(plot.max_x(), plot.max_y()),
        ],
        axis,
    );
    // The categories' labels.
    for (i, name) in categories.iter().enumerate() {
        let mid = band * (i as f32 + 0.5);
        if c.horizontal {
            scene.text(text(
                Point::new(plot.min_x() - 6.0, plot.min_y() + mid),
                name,
                size,
                look.text,
                Anchor::End,
                false,
            ));
        } else {
            scene.text(text(
                Point::new(plot.min_x() + mid, plot.max_y() + line_h * 0.8),
                name,
                size,
                look.text,
                Anchor::Middle,
                false,
            ));
        }
    }
    // Bars side by side in each band, lines over them.
    let bars: Vec<&Vec<f64>> = c
        .series
        .iter()
        .filter_map(|s| if let Series::Bar(v) = s { Some(v) } else { None })
        .collect();
    let base = lo.max(0.0).min(hi);
    let group = band * 0.72;
    let bar = group / bars.len().max(1) as f32;
    for (b, values) in bars.iter().enumerate() {
        let color = series(look, b);
        for (i, v) in values.iter().enumerate() {
            let start = band * i as f32 + (band - group) / 2.0 + bar * b as f32;
            let r = between(at(start + 1.0, base), at(start + bar - 1.0, v.clamp(lo, hi)));
            scene.rect(r, 2.0, Some(color.mix(look.background, 0.15)), None);
        }
    }
    let mut line_i = bars.len();
    for s in &c.series {
        let Series::Line(values) = s else { continue };
        let color = series(look, line_i);
        line_i += 1;
        let points: Vec<Point> = values
            .iter()
            .enumerate()
            .map(|(i, v)| at(band * (i as f32 + 0.5), v.clamp(lo, hi)))
            .collect();
        scene.polyline(&points, Line::solid(color, 2.2));
        for p in &points {
            circle(&mut scene, *p, 3.5, Some(color), None);
        }
    }
    let mut bottom = plot.max_y() + line_h * 1.6;
    // The axes' titles: the categories' under (or over) their axis, the values' over theirs.
    let (cat_title, value_title) = (c.x_title.as_deref(), c.y_title.as_deref());
    let (under, over) =
        if c.horizontal { (value_title, cat_title) } else { (cat_title, value_title) };
    if let Some(t) = under {
        scene.text(text(
            Point::new(plot.center().x, bottom + line_h * 0.3),
            t,
            size,
            look.text,
            Anchor::Middle,
            true,
        ));
        bottom += line_h * 1.4;
    }
    if let Some(t) = over {
        scene.text(text(
            Point::new(4.0, top - line_h * 0.7),
            t,
            size,
            look.text,
            Anchor::Start,
            true,
        ));
    }
    scene.width = plot.max_x() + 12.0;
    scene.height = bottom;
    scene
}

// ---- quadrant chart ----

pub fn quadrant(q: &Quadrant, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size * 0.9;
    let mut scene = Scene::default();
    let side = 420.0f32;
    let left =
        [&q.y.0, &q.y.1].iter().map(|t| text_size(measure, t, size, false).0).fold(0.0, f32::max)
            + 14.0;
    let plot = Rect::new(left, 4.0, side, side);
    let half = side / 2.0;
    // Quadrants 1 (top right), 2 (top left), 3 (bottom left), 4 (bottom right).
    let corners = [
        (plot.min_x() + half, plot.min_y()),
        (plot.min_x(), plot.min_y()),
        (plot.min_x(), plot.min_y() + half),
        (plot.min_x() + half, plot.min_y() + half),
    ];
    let tints = [0.22, 0.14, 0.08, 0.14];
    for (i, (x, y)) in corners.iter().enumerate() {
        let r = Rect::new(*x, *y, half, half);
        scene.rect(r, 0.0, Some(look.background.mix(look.accent, tints[i])), None);
        if !q.quadrants[i].is_empty() {
            let label = super::charts::wrap(measure, &q.quadrants[i], size * 1.05, half - 24.0);
            let lines = label.lines().count().max(1) as f32;
            let at = Point::new(r.center().x, r.min_y() + 14.0 + lines * size * 0.65);
            label_texts(&mut scene, &label, at, size * 1.05, look.text, true);
        }
    }
    let line = Line::solid(look.line.with_alpha(0.6), 1.0);
    scene.rect(plot, 0.0, None, Some(line));
    scene.polyline(
        &[Point::new(plot.center().x, plot.min_y()), Point::new(plot.center().x, plot.max_y())],
        line,
    );
    scene.polyline(
        &[Point::new(plot.min_x(), plot.center().y), Point::new(plot.max_x(), plot.center().y)],
        line,
    );
    // The axes' ends: x under each half, y beside each half.
    let below = plot.max_y() + size * 1.1;
    scene.text(text(
        Point::new(plot.min_x() + half / 2.0, below),
        &q.x.0,
        size,
        look.muted,
        Anchor::Middle,
        false,
    ));
    scene.text(text(
        Point::new(plot.max_x() - half / 2.0, below),
        &q.x.1,
        size,
        look.muted,
        Anchor::Middle,
        false,
    ));
    scene.text(text(
        Point::new(plot.min_x() - 8.0, plot.max_y() - half / 2.0),
        &q.y.0,
        size,
        look.muted,
        Anchor::End,
        false,
    ));
    scene.text(text(
        Point::new(plot.min_x() - 8.0, plot.min_y() + half / 2.0),
        &q.y.1,
        size,
        look.muted,
        Anchor::End,
        false,
    ));
    for (name, x, y) in &q.points {
        let p = Point::new(
            plot.min_x() + x.clamp(0.0, 1.0) as f32 * side,
            plot.max_y() - y.clamp(0.0, 1.0) as f32 * side,
        );
        circle(&mut scene, p, 5.5, Some(look.accent), Some(Line::solid(look.background, 1.2)));
        scene.text(text(
            Point::new(p.x, p.y + 15.0),
            name,
            size * 0.92,
            look.text,
            Anchor::Middle,
            false,
        ));
    }
    scene.width = plot.max_x() + 8.0;
    scene.height = below + size;
    scene
}

// ---- radar ----

pub fn radar(r: &Radar, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size * 0.9;
    let mut scene = Scene::default();
    let n = r.axes.len().max(1);
    let radius = 150.0f32;
    let label_w =
        r.axes.iter().map(|(_, l)| text_size(measure, l, size, false).0).fold(0.0, f32::max);
    let c = Point::new(label_w + radius + 18.0, radius + size * 1.6 + 6.0);
    let angle =
        |i: usize| -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
    let at = |i: usize, share: f32| {
        let a = angle(i);
        Point::new(c.x + radius * share * a.cos(), c.y + radius * share * a.sin())
    };
    let max = r.max.unwrap_or_else(|| {
        r.curves.iter().flat_map(|(_, v)| v.iter().copied()).fold(0.0, f64::max)
    });
    let span = (max - r.min).max(f64::EPSILON);
    let grid = Line::solid(look.line.with_alpha(0.3), 1.0);
    for level in 1..=5 {
        let share = level as f32 / 5.0;
        let points: Vec<Point> = (0..n).map(|i| at(i, share)).collect();
        if n >= 3 {
            scene.path(scene::polygon(&points), None, Some(grid));
        } else {
            circle(&mut scene, c, radius * share, None, Some(grid));
        }
    }
    for (i, (_, label)) in r.axes.iter().enumerate() {
        scene.polyline(&[c, at(i, 1.0)], grid);
        let p = at(i, 1.0);
        let a = angle(i);
        let (dx, dy) = (a.cos(), a.sin());
        let anchor = if dx > 0.2 {
            Anchor::Start
        } else if dx < -0.2 {
            Anchor::End
        } else {
            Anchor::Middle
        };
        let at = Point::new(p.x + dx * 10.0, p.y + dy * (10.0 + size * 0.5));
        scene.text(text(at, label, size, look.text, anchor, false));
    }
    for (k, (_, values)) in r.curves.iter().enumerate() {
        let color = series(look, k);
        let points: Vec<Point> = (0..n)
            .map(|i| {
                at(
                    i,
                    ((values.get(i).copied().unwrap_or(r.min) - r.min) / span).clamp(0.0, 1.0)
                        as f32,
                )
            })
            .collect();
        scene.path(
            scene::polygon(&points),
            Some(color.with_alpha(0.22)),
            Some(Line::solid(color, 2.0)),
        );
        for p in &points {
            circle(&mut scene, *p, 3.0, Some(color), None);
        }
    }
    // The curves' legend, beside.
    let lx = c.x + radius + label_w + 28.0;
    let row = size * 1.7;
    let mut right = c.x + radius + label_w + 12.0;
    for (k, (name, _)) in r.curves.iter().enumerate() {
        let y = c.y - radius + row * k as f32;
        scene.rect(Rect::new(lx, y - 7.0, 14.0, 14.0), 3.0, Some(series(look, k)), None);
        scene.text(text(Point::new(lx + 22.0, y), name, size, look.text, Anchor::Start, false));
        right = right.max(lx + 22.0 + text_size(measure, name, size, false).0);
    }
    scene.width = right + 4.0;
    scene.height = c.y + radius + size * 1.6 + 6.0;
    scene
}

// ---- packet ----

/// Bits on a row.
const BITS: u32 = 32;

pub fn packet(p: &Packet, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size * 0.88;
    let small = size * 0.78;
    let mut scene = Scene::default();
    let bit = 30.0f32;
    let row_h = 36.0f32;
    let numbers = small * 1.3;
    let row_y = |row: u32| row as f32 * (row_h + numbers) + numbers;
    let mut rows = 0;
    for (i, (start, end, name)) in p.fields.iter().enumerate() {
        let (start, end) = (*start.min(end), *end.max(start));
        // A field over several rows: one box on each.
        let mut a = start;
        while a <= end {
            let row = a / BITS;
            let b = end.min(row * BITS + BITS - 1);
            let x0 = (a % BITS) as f32 * bit;
            let r = Rect::new(x0, row_y(row), (b - a + 1) as f32 * bit, row_h);
            let fill = look.fill.mix(series(look, i), 0.08);
            scene.rect(r, 2.0, Some(fill), Some(Line::solid(look.stroke, 1.1)));
            // Too wide for its box: smaller, then on lines.
            let room = r.size.width - 6.0;
            let fits = |m: &mut dyn Measure, s: f32| text_size(m, name, s, false).0 <= room;
            let (label, at) = if fits(measure, size) {
                (name.clone(), size)
            } else if fits(measure, small) {
                (name.clone(), small)
            } else {
                (super::charts::wrap(measure, name, small, room), small)
            };
            label_texts(&mut scene, &label, r.center(), at, look.text, false);
            let ny = r.min_y() - numbers * 0.45;
            scene.text(text(
                Point::new(r.min_x() + 2.0, ny),
                &a.to_string(),
                small,
                look.muted,
                Anchor::Start,
                false,
            ));
            if b != a {
                scene.text(text(
                    Point::new(r.max_x() - 2.0, ny),
                    &b.to_string(),
                    small,
                    look.muted,
                    Anchor::End,
                    false,
                ));
            }
            rows = rows.max(row + 1);
            a = b + 1;
        }
    }
    scene.width = BITS as f32 * bit;
    scene.height = row_y(rows.max(1)) - numbers + 4.0;
    scene
}

// ---- treemap ----

pub fn treemap(t: &Treemap, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size * 0.88;
    let mut scene = Scene::default();
    let mut totals = vec![0.0f64; t.nodes.len()];
    fn total(t: &Treemap, n: usize, out: &mut [f64]) -> f64 {
        let own = t.nodes[n].value.unwrap_or(0.0).max(0.0);
        let sum: f64 = t.nodes[n].children.clone().into_iter().map(|c| total(t, c, out)).sum();
        out[n] = if t.nodes[n].children.is_empty() { own } else { sum.max(own) };
        out[n]
    }
    for &r in &t.roots {
        total(t, r, &mut totals);
    }
    let area = Rect::new(0.0, 0.0, 760.0, 460.0);
    let mut stack: Vec<(Vec<usize>, Rect, usize, usize)> = vec![(t.roots.clone(), area, 0, 0)];
    while let Some((items, rect, depth, color)) = stack.pop() {
        let placed = squarify(&items, &totals, rect);
        for (k, (n, r)) in placed.into_iter().enumerate() {
            let color = if depth == 0 { k } else { color };
            let node = &t.nodes[n];
            let r = Rect::new(
                r.min_x() + 2.0,
                r.min_y() + 2.0,
                (r.size.width - 4.0).max(0.0),
                (r.size.height - 4.0).max(0.0),
            );
            if r.size.width < 2.0 || r.size.height < 2.0 {
                continue;
            }
            let hue = series(look, color);
            let header = size * 1.6;
            if node.children.is_empty() {
                scene.rect(r, 3.0, Some(hue.mix(look.background, 0.2 + 0.12 * depth as f32)), None);
                let label = format!("{}\n{}", node.text, number(totals[n]));
                let (w, h) = text_size(measure, &label, size, false);
                if w <= r.size.width - 6.0 && h <= r.size.height - 4.0 {
                    label_texts(&mut scene, &label, r.center(), size, look.background, false);
                } else if text_size(measure, &node.text, size, false).0 <= r.size.width - 6.0
                    && size * 1.3 <= r.size.height
                {
                    label_texts(&mut scene, &node.text, r.center(), size, look.background, false);
                }
            } else {
                scene.rect(
                    r,
                    3.0,
                    Some(hue.with_alpha(0.12)),
                    Some(Line::solid(hue.mix(look.background, 0.3), 1.0)),
                );
                if r.size.height > header * 1.5 {
                    let title = format!("{}  {}", node.text, number(totals[n]));
                    scene.text(text(
                        Point::new(r.min_x() + 6.0, r.min_y() + header / 2.0),
                        &title,
                        size,
                        look.text,
                        Anchor::Start,
                        true,
                    ));
                    let inner = Rect::new(
                        r.min_x() + 4.0,
                        r.min_y() + header,
                        r.size.width - 8.0,
                        r.size.height - header - 4.0,
                    );
                    stack.push((node.children.clone(), inner, depth + 1, color));
                }
            }
        }
    }
    scene.width = area.size.width;
    scene.height = area.size.height;
    scene
}

/// The squarified treemap: `items` (by `value`) into `rect`, in rows whose boxes are as
/// square as they can be.
fn squarify(items: &[usize], value: &[f64], rect: Rect) -> Vec<(usize, Rect)> {
    let mut items: Vec<usize> = items.iter().copied().filter(|&i| value[i] > 0.0).collect();
    items.sort_by(|&a, &b| value[b].partial_cmp(&value[a]).unwrap_or(std::cmp::Ordering::Equal));
    let sum: f64 = items.iter().map(|&i| value[i]).sum();
    if sum <= 0.0 {
        return Vec::new();
    }
    let scale = f64::from(rect.size.width) * f64::from(rect.size.height) / sum;
    let mut out = Vec::new();
    let mut free = rect;
    let mut i = 0;
    while i < items.len() {
        let short = f64::from(free.size.width.min(free.size.height));
        let worst = |row: &[usize]| {
            let areas: Vec<f64> = row.iter().map(|&k| value[k] * scale).collect();
            let s: f64 = areas.iter().sum();
            let (mx, mn) =
                areas.iter().fold((0.0f64, f64::INFINITY), |(a, b), &v| (a.max(v), b.min(v)));
            (short * short * mx / (s * s)).max(s * s / (short * short * mn))
        };
        let mut end = i + 1;
        while end < items.len() && worst(&items[i..=end]) <= worst(&items[i..end]) {
            end += 1;
        }
        let row = &items[i..end];
        let area: f64 = row.iter().map(|&k| value[k] * scale).sum();
        let wide = free.size.width >= free.size.height;
        let thick = (area / short) as f32;
        let mut along = 0.0f32;
        for &k in row {
            let len = (value[k] * scale / area * short) as f32;
            let r = if wide {
                Rect::new(free.min_x(), free.min_y() + along, thick, len)
            } else {
                Rect::new(free.min_x() + along, free.min_y(), len, thick)
            };
            out.push((k, r));
            along += len;
        }
        free = if wide {
            Rect::new(
                free.min_x() + thick,
                free.min_y(),
                (free.size.width - thick).max(0.0),
                free.size.height,
            )
        } else {
            Rect::new(
                free.min_x(),
                free.min_y() + thick,
                free.size.width,
                (free.size.height - thick).max(0.0),
            )
        };
        i = end;
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/plots.rs"]
mod tests;
