//! The diagrams that aren't boxes and lines: a pie (its slices, their shares, a legend),
//! a gantt chart (a time axis, sections, task bars and milestones), a timeline (periods
//! in a row, their events under them, sections above).

use diagram_text::mermaid::gantt::{self, DAY, Gantt};
use diagram_text::mermaid::{Pie, Timeline};
use kernel::{Point, Rect, Rgba};

use super::scene::{self, Anchor, Line, Scene, Seg, Text};
use super::{Look, Measure, label_texts, text_size};

pub(crate) fn text(at: Point, s: &str, size: f32, color: Rgba, anchor: Anchor, bold: bool) -> Text {
    Text {
        at,
        text: s.to_owned(),
        size,
        bold,
        italic: false,
        mono: false,
        math: false,
        color,
        anchor,
    }
}

/// An arc of the circle around `c` from angle `a0` to `a1` (radians, clockwise from
/// the right), as cubics.
pub(crate) fn arc(c: Point, r: f32, a0: f32, a1: f32, segs: &mut Vec<Seg>) {
    let steps = ((a1 - a0).abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = (a1 - a0) / steps as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |a: f32| Point::new(c.x + r * a.cos(), c.y + r * a.sin());
    for i in 0..steps {
        let (s, e) = (a0 + step * i as f32, a0 + step * (i + 1) as f32);
        let (p0, p1) = (at(s), at(e));
        let c1 = Point::new(p0.x - k * r * s.sin(), p0.y + k * r * s.cos());
        let c2 = Point::new(p1.x + k * r * e.sin(), p1.y - k * r * e.cos());
        segs.push(Seg::Cubic(c1, c2, p1));
    }
}

pub fn pie(p: &Pie, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let mut scene = Scene::default();
    let r = 120.0;
    let c = Point::new(r + 4.0, r + 4.0);
    let total: f64 = p.slices.iter().map(|(_, v)| v).sum();
    let mut a = -std::f32::consts::FRAC_PI_2;
    let color = |i: usize| look.series[i % look.series.len()];
    for (i, (_, v)) in p.slices.iter().enumerate() {
        if total <= 0.0 {
            break;
        }
        let share = (*v / total) as f32;
        let sweep = share * std::f32::consts::TAU;
        let mut segs =
            vec![Seg::Move(c), Seg::Line(Point::new(c.x + r * a.cos(), c.y + r * a.sin()))];
        arc(c, r, a, a + sweep, &mut segs);
        segs.push(Seg::Close);
        let fill = color(i).mix(look.background, 0.15);
        scene.path(segs, Some(fill), Some(Line::solid(look.background, 2.0)));
        if share >= 0.04 {
            let mid = a + sweep / 2.0;
            let at = Point::new(c.x + r * 0.62 * mid.cos(), c.y + r * 0.62 * mid.sin());
            let label = format!("{:.0}%", share * 100.0);
            scene.text(text(at, &label, size * 0.9, look.background, Anchor::Middle, true));
        }
        a += sweep;
    }
    // The legend, beside.
    let lx = c.x + r + 32.0;
    let row = size * 1.7;
    let ly = c.y - row * p.slices.len() as f32 / 2.0 + row / 2.0;
    let mut right: f32 = lx;
    for (i, (name, v)) in p.slices.iter().enumerate() {
        let y = ly + row * i as f32;
        scene.rect(
            Rect::new(lx, y - 7.0, 14.0, 14.0),
            3.0,
            Some(color(i).mix(look.background, 0.15)),
            None,
        );
        let label =
            if p.show_data { format!("{name} [{}]", trim_number(*v)) } else { name.clone() };
        let w = text_size(measure, &label, size, false).0;
        scene.text(text(Point::new(lx + 22.0, y), &label, size, look.text, Anchor::Start, false));
        right = right.max(lx + 22.0 + w);
    }
    scene.width = right + 4.0;
    scene.height = (c.y + r + 4.0).max(ly + row * p.slices.len() as f32);
    scene
}

fn trim_number(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") }
}

pub fn gantt(g: &Gantt, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let mut scene = Scene::default();
    let small = size * 0.88;
    let bar_h = 22.0;
    let row = bar_h + 8.0;
    let top = 4.0;
    let left =
        g.sections.iter().map(|s| text_size(measure, s, small, true).0).fold(0.0, f32::max) + 24.0;
    let (t0, t1) = g
        .tasks
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t.start), b.max(t.end)));
    if !t0.is_finite() {
        scene.width = left;
        scene.height = top;
        return scene;
    }
    let span = (t1 - t0).max(DAY);
    let width = 760.0f32;
    let x = |t: f64| left + ((t - t0) / span) as f32 * width;
    let body_h = g.tasks.len() as f32 * row;
    let body = Rect::new(left, top, width, body_h);
    // Sections' bands (each task row in its section's color).
    for (i, t) in g.tasks.iter().enumerate() {
        let y = top + i as f32 * row;
        let s = t.section.unwrap_or(0);
        let band = look.series[s % look.series.len()].with_alpha(0.07);
        scene.rect(Rect::new(0.0, y, left + width, row), 0.0, Some(band), None);
    }
    for (s, name) in g.sections.iter().enumerate() {
        let rows: Vec<usize> = g
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.section == Some(s))
            .map(|(i, _)| i)
            .collect();
        let (Some(&a), Some(&b)) = (rows.first(), rows.last()) else { continue };
        let mid = top + (a + b + 1) as f32 * row / 2.0;
        scene.text(text(Point::new(8.0, mid), name, small, look.text, Anchor::Start, true));
    }
    // The axis: a tick every round interval, about eight of them.
    let steps = [
        3600.0,
        6.0 * 3600.0,
        DAY,
        2.0 * DAY,
        7.0 * DAY,
        14.0 * DAY,
        30.0 * DAY,
        91.0 * DAY,
        365.0 * DAY,
    ];
    let step = steps.iter().copied().find(|s| span / s <= 10.0).unwrap_or(365.0 * DAY);
    let format = g
        .axis_format
        .clone()
        .unwrap_or_else(|| if step < DAY { "%H:%M".into() } else { "%Y-%m-%d".into() });
    let mut t = (t0 / step).ceil() * step;
    let grid = Line::solid(look.group_stroke.with_alpha(0.6), 1.0);
    while t <= t1 + 1.0 {
        let tx = x(t);
        scene.path(
            vec![Seg::Move(Point::new(tx, top)), Seg::Line(Point::new(tx, top + body_h))],
            None,
            Some(grid),
        );
        let label = gantt::format_time(t, &format);
        scene.text(text(
            Point::new(tx, top + body_h + size),
            &label,
            small * 0.92,
            look.muted,
            Anchor::Middle,
            false,
        ));
        t += step;
    }
    for (i, task) in g.tasks.iter().enumerate() {
        let y = top + i as f32 * row + (row - bar_h) / 2.0;
        let s = task.section.unwrap_or(0);
        let base = look.series[s % look.series.len()];
        let crit = look.series[5];
        let (fill, stroke) = if task.done {
            (look.muted.with_alpha(0.35), look.muted)
        } else if task.crit {
            (crit.mix(look.background, 0.25), crit)
        } else if task.active {
            (base.mix(look.background, 0.6), base)
        } else {
            (base.mix(look.background, 0.2), base.mix(look.text, 0.2))
        };
        let (x0, x1) = (x(task.start), x(task.end));
        if task.milestone {
            let c = Point::new(x0, y + bar_h / 2.0);
            let h = bar_h / 2.0;
            let p = Point::new;
            scene.path(
                scene::polygon(&[
                    p(c.x, c.y - h),
                    p(c.x + h, c.y),
                    p(c.x, c.y + h),
                    p(c.x - h, c.y),
                ]),
                Some(fill),
                Some(Line::solid(stroke, 1.3)),
            );
            scene.text(text(
                Point::new(c.x + h + 6.0, c.y),
                &task.name,
                small,
                look.text,
                Anchor::Start,
                false,
            ));
            continue;
        }
        let r = Rect::new(x0, y, (x1 - x0).max(2.0), bar_h);
        scene.rect(
            r,
            4.0,
            Some(fill),
            Some(Line::solid(stroke, if task.active || task.crit { 1.6 } else { 1.0 })),
        );
        let w = text_size(measure, &task.name, small, false).0;
        let color = if task.done { look.muted } else { look.text };
        if w + 12.0 <= r.size.width {
            scene.text(text(r.center(), &task.name, small, color, Anchor::Middle, false));
        } else {
            scene.text(text(
                Point::new(r.max_x() + 6.0, r.center().y),
                &task.name,
                small,
                color,
                Anchor::Start,
                false,
            ));
        }
    }
    scene.rect(body, 0.0, None, Some(Line::solid(look.group_stroke, 1.0)));
    let names_right = g
        .tasks
        .iter()
        .map(|t| x(t.end) + 12.0 + text_size(measure, &t.name, small, false).0)
        .fold(left + width, f32::max);
    scene.width = names_right + 4.0;
    scene.height = top + body_h + size * 2.0;
    scene
}

pub fn timeline(t: &Timeline, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let small = size * 0.9;
    let mut scene = Scene::default();
    let col = 160.0f32;
    let gap = 16.0;
    let top = 0.0;
    let has_sections = !t.sections.is_empty();
    let section_h = if has_sections { size * 2.2 } else { 0.0 };
    let period_top = top + section_h + 8.0;
    let period_h = size * 2.4;
    let color = |i: usize| look.series[i % look.series.len()];
    // Sections spanning their periods.
    for (s, name) in t.sections.iter().enumerate() {
        let cols: Vec<usize> = t
            .periods
            .iter()
            .enumerate()
            .filter(|(_, p)| p.section == Some(s))
            .map(|(i, _)| i)
            .collect();
        let (Some(&a), Some(&b)) = (cols.first(), cols.last()) else { continue };
        let r = Rect::new(
            a as f32 * (col + gap),
            top,
            (b - a + 1) as f32 * (col + gap) - gap,
            section_h - 4.0,
        );
        scene.rect(r, 6.0, Some(color(s).mix(look.background, 0.55)), None);
        label_texts(&mut scene, name, r.center(), size, look.text, true);
    }
    let mut bottom = period_top + period_h;
    let axis_y = period_top + period_h + 18.0;
    for (i, p) in t.periods.iter().enumerate() {
        let x0 = i as f32 * (col + gap);
        let tone = color(p.section.unwrap_or(i));
        let r = Rect::new(x0, period_top, col, period_h);
        scene.rect(r, 6.0, Some(tone.mix(look.background, 0.25)), None);
        label_texts(&mut scene, &p.name, r.center(), size, look.background, true);
        let cx = r.center().x;
        let mut y = axis_y + 14.0;
        for e in &p.events {
            let wrapped = wrap(measure, e, small, col - 16.0);
            let (_, h) = text_size(measure, &wrapped, small, false);
            let er = Rect::new(x0 + 4.0, y, col - 8.0, h + 14.0);
            scene.rect(
                er,
                5.0,
                Some(tone.mix(look.background, 0.82)),
                Some(Line::solid(tone.mix(look.background, 0.4), 1.0)),
            );
            label_texts(&mut scene, &wrapped, er.center(), small, look.text, false);
            y = er.max_y() + 8.0;
        }
        if !p.events.is_empty() {
            scene.path(
                vec![
                    Seg::Move(Point::new(cx, r.max_y())),
                    Seg::Line(Point::new(cx, axis_y + 14.0)),
                ],
                None,
                Some(Line::dashed(tone, 1.2)),
            );
        }
        bottom = bottom.max(y);
    }
    let width = t.periods.len().max(1) as f32 * (col + gap) - gap;
    // The time's line, under the periods.
    scene.path(
        vec![Seg::Move(Point::new(0.0, axis_y)), Seg::Line(Point::new(width, axis_y))],
        None,
        Some(Line::solid(look.line, 2.0)),
    );
    scene::head(
        &mut scene,
        Point::new(width - 10.0, axis_y),
        Point::new(width + 6.0, axis_y),
        scene::HeadKind::Arrow,
        10.0,
        look.line,
        look.background,
    );
    scene.width = width + 8.0;
    scene.height = bottom;
    scene
}

/// `s` with new lines where it's wider than `width`.
pub fn wrap(measure: &mut dyn Measure, s: &str, size: f32, width: f32) -> String {
    let mut out = String::new();
    for line in s.lines() {
        let mut current = String::new();
        for word in line.split_whitespace() {
            let candidate =
                if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
            if !current.is_empty() && measure.width(&candidate, size, false) > width {
                out.push_str(&current);
                out.push('\n');
                current = word.to_owned();
            } else {
                current = candidate;
            }
        }
        out.push_str(&current);
        out.push('\n');
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/charts.rs"]
mod tests;
