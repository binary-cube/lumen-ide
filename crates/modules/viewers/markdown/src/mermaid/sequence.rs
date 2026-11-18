//! A sequence diagram laid out and drawn: a column per participant (as far apart as the
//! messages between them need), a row per message, note and block line; lifelines,
//! activations, the blocks' frames with their parts, boxes around participants, numbers.

use diagram_text::mermaid::sequence::{BlockKind, Item, Placement, Sequence, Tip};
use kernel::{Point, Rect, Rgba};

use super::scene::{self, Anchor, HeadKind, Line, Prim, Scene, Seg, Text};
use super::{Look, Measure, label_texts, text_size};

const BOX_H: f32 = 40.0;
const MIN_BOX_W: f32 = 90.0;
const ACTOR_H: f32 = 58.0;
const ACTIVATION_W: f32 = 10.0;

pub fn layout(seq: &Sequence, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let n = seq.participants.len().max(1);
    let widths: Vec<f32> = seq
        .participants
        .iter()
        .map(|p| (text_size(measure, &p.label, size, false).0 + 28.0).max(MIN_BOX_W))
        .collect();
    let head_h =
        if seq.participants.iter().any(|p| p.actor) { ACTOR_H + size * 1.4 } else { BOX_H };
    // The room each gap between neighbors needs (messages and notes over it).
    let mut gaps = vec![60.0f32; n.saturating_sub(1)];
    for p in 0..n.saturating_sub(1) {
        gaps[p] = gaps[p].max((widths[p] + widths[p + 1]) / 2.0 + 24.0);
    }
    for item in &seq.items {
        let (a, b, need) = match item {
            Item::Message { from, to, text, .. } if from != to => {
                let w = text_size(measure, text, size * 0.95, false).0 + 40.0;
                ((*from).min(*to), (*from).max(*to), w)
            }
            Item::Note { at: Placement::Over(a, b), text } if a != b => {
                let w = text_size(measure, text, size * 0.92, false).0 + 20.0;
                ((*a).min(*b), (*a).max(*b), w)
            }
            Item::Note { at: Placement::RightOf(a), text } if *a + 1 < n => {
                let w = text_size(measure, text, size * 0.92, false).0 + 40.0;
                (*a, *a + 1, w + widths[*a + 1] / 2.0)
            }
            Item::Note { at: Placement::LeftOf(a), text } if *a > 0 => {
                let w = text_size(measure, text, size * 0.92, false).0 + 40.0;
                (*a - 1, *a, w + widths[*a - 1] / 2.0)
            }
            _ => continue,
        };
        if a == b {
            continue;
        }
        let have: f32 = gaps[a..b].iter().sum();
        if have < need {
            let more = (need - have) / (b - a) as f32;
            for g in &mut gaps[a..b] {
                *g += more;
            }
        }
    }
    let mut xs = vec![widths.first().copied().unwrap_or(MIN_BOX_W) / 2.0; n];
    for p in 1..n {
        xs[p] = xs[p - 1] + gaps[p - 1];
    }

    let mut back = Scene::default();
    let mut front = Scene::default();
    let mut y = head_h + 24.0;
    // Activations: per participant, the rows where each open one began.
    let mut active: Vec<Vec<f32>> = vec![Vec::new(); n];
    let mut bars: Vec<(usize, usize, f32, f32)> = Vec::new();
    // Blocks open: kind, label, top, which participants they span, their parts' lines.
    struct Open {
        kind: BlockKind,
        label: String,
        color: Option<Rgba>,
        top: f32,
        span: Option<(usize, usize)>,
        parts: Vec<(f32, String)>,
    }
    let mut open: Vec<Open> = Vec::new();
    let spans = block_spans(seq);
    let mut block_index = 0;
    let mut number = 0;
    let offset = |active: &Vec<Vec<f32>>, p: usize, right: bool| -> f32 {
        let depth = active[p].len() as f32;
        if depth == 0.0 {
            0.0
        } else if right {
            ACTIVATION_W / 2.0 + (depth - 1.0) * 4.0
        } else {
            -ACTIVATION_W / 2.0
        }
    };
    let mut min_x = 0.0f32;
    let mut max_x = xs[n - 1] + widths.last().copied().unwrap_or(MIN_BOX_W) / 2.0;
    for item in &seq.items {
        match item {
            Item::Message { from, to, text, dotted, tip, both, activate, .. } => {
                let (from, to) = (*from, *to);
                let lines = text.lines().count().max(usize::from(!text.is_empty())) as f32;
                let text_h = lines * size * 1.25;
                let color = look.line;
                let line = if *dotted {
                    Line { color, width: 1.4, dash: Some((4.0, 3.0)) }
                } else {
                    Line::solid(color, 1.4)
                };
                if *activate == Some(true) {
                    // The target's bar starts with the message.
                }
                if from == to {
                    let x = xs[from] + offset(&active, from, true);
                    let top = y + text_h;
                    let w = 34.0;
                    let pts = [
                        Point::new(x, top),
                        Point::new(x + w, top),
                        Point::new(x + w, top + 18.0),
                        Point::new(x, top + 18.0),
                    ];
                    front.polyline(&pts, line);
                    head_at(
                        &mut front,
                        *tip,
                        Point::new(x + w, top + 18.0),
                        Point::new(x, top + 18.0),
                        color,
                        look,
                    );
                    if !text.is_empty() {
                        text_lines(
                            &mut front,
                            text,
                            Point::new(x + 6.0, y + text_h / 2.0),
                            size * 0.95,
                            look.text,
                            Anchor::Start,
                        );
                        let tw = text_size(measure, text, size * 0.95, false).0;
                        max_x = max_x.max(x + tw + 12.0);
                    }
                    max_x = max_x.max(x + w + 8.0);
                    y += text_h + 18.0 + 22.0;
                } else {
                    let right = to > from;
                    let x0 = xs[from] + offset(&active, from, right);
                    let target_offset = if *activate == Some(true) {
                        if right { -ACTIVATION_W / 2.0 } else { ACTIVATION_W / 2.0 }
                    } else {
                        offset(&active, to, !right)
                    };
                    let x1 = xs[to] + target_offset;
                    let ay = y + text_h + 6.0;
                    let size_head = 9.0;
                    let dir = if right { 1.0 } else { -1.0 };
                    let end = if *tip == Tip::None { x1 } else { x1 - dir * size_head * 0.7 };
                    let start =
                        if *both && *tip != Tip::None { x0 + dir * size_head * 0.7 } else { x0 };
                    front.polyline(&[Point::new(start, ay), Point::new(end, ay)], line);
                    head_at(&mut front, *tip, Point::new(x0, ay), Point::new(x1, ay), color, look);
                    if *both {
                        head_at(
                            &mut front,
                            *tip,
                            Point::new(x1, ay),
                            Point::new(x0, ay),
                            color,
                            look,
                        );
                    }
                    if seq.autonumber {
                        number += 1;
                        let r = 9.0;
                        let c = Point::new(x0, ay);
                        front.path(
                            scene::ellipse(Rect::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r)),
                            Some(look.accent),
                            None,
                        );
                        front.text(Text {
                            at: c,
                            text: number.to_string(),
                            size: size * 0.7,
                            bold: true,
                            italic: false,
                            mono: false,
                            math: false,
                            color: look.background,
                            anchor: Anchor::Middle,
                        });
                    }
                    if !text.is_empty() {
                        let mid = (x0 + x1) / 2.0;
                        text_lines(
                            &mut front,
                            text,
                            Point::new(mid, y + text_h / 2.0),
                            size * 0.95,
                            look.text,
                            Anchor::Middle,
                        );
                    }
                    y = ay + 22.0;
                }
                match activate {
                    Some(true) => active[to].push(y - 22.0),
                    Some(false) => {
                        if let Some(top) = active[from].pop() {
                            bars.push((from, active[from].len(), top, y - 22.0));
                        }
                    }
                    None => {}
                }
                if let Some(b) = open.last_mut() {
                    let _ = b;
                }
            }
            Item::Note { at, text } => {
                let (tw, th) = text_size(measure, text, size * 0.92, false);
                let (w, h) = (tw + 20.0, th + 14.0);
                let (x0, x1) = match *at {
                    Placement::RightOf(p) => (xs[p] + 14.0, xs[p] + 14.0 + w),
                    Placement::LeftOf(p) => (xs[p] - 14.0 - w, xs[p] - 14.0),
                    Placement::Over(a, b) => {
                        let (a, b) = (a.min(b), a.max(b));
                        let (l, r) =
                            (xs[a] - widths[a] / 2.0 + 10.0, xs[b] + widths[b] / 2.0 - 10.0);
                        if r - l >= w {
                            (l, r)
                        } else {
                            let c = (xs[a] + xs[b]) / 2.0;
                            (c - w / 2.0, c + w / 2.0)
                        }
                    }
                };
                let r = Rect::new(x0, y, x1 - x0, h);
                front.rect(r, 3.0, Some(look.note), Some(Line::solid(look.note_stroke, 1.0)));
                label_texts(&mut front, text, r.center(), size * 0.92, look.text, false);
                min_x = min_x.min(x0 - 4.0);
                max_x = max_x.max(x1 + 4.0);
                y += h + 14.0;
            }
            Item::Activate(p) => active[*p].push(y - 10.0),
            Item::Deactivate(p) => {
                if let Some(top) = active[*p].pop() {
                    bars.push((*p, active[*p].len(), top, y - 10.0));
                }
            }
            Item::Start { kind, label, color } => {
                let span = spans.get(block_index).copied().flatten();
                block_index += 1;
                open.push(Open {
                    kind: *kind,
                    label: label.clone(),
                    color: *color,
                    top: y,
                    span,
                    parts: Vec::new(),
                });
                y += if *kind == BlockKind::Rect { 10.0 } else { size * 1.5 + 14.0 };
            }
            Item::Part { label } => {
                y += 4.0;
                if let Some(b) = open.last_mut() {
                    b.parts.push((y, label.clone()));
                }
                y += size * 1.5 + 8.0;
            }
            Item::End => {
                let Some(b) = open.pop() else { continue };
                y += 6.0;
                let depth = open.len() as f32;
                let (a, z) = b.span.unwrap_or((0, n - 1));
                let x0 = xs[a] - widths[a] / 2.0 - 10.0 + depth * 8.0 - 8.0;
                let x1 = xs[z] + widths[z] / 2.0 + 10.0 - depth * 8.0 + 8.0;
                min_x = min_x.min(x0 - 4.0);
                max_x = max_x.max(x1 + 4.0);
                let r = Rect::new(x0, b.top, x1 - x0, y - b.top);
                if b.kind == BlockKind::Rect {
                    let fill = b
                        .color
                        .map_or(look.group, |c| if c.a >= 1.0 { c.with_alpha(0.35) } else { c });
                    back.rect(r, 4.0, Some(fill), None);
                } else {
                    let stroke = look.group_stroke;
                    back.rect(
                        r,
                        4.0,
                        Some(look.group.with_alpha(0.02)),
                        Some(Line::solid(stroke, 1.2)),
                    );
                    // The tab with the block's kind, then its label.
                    let name = match b.kind {
                        BlockKind::Loop => "loop",
                        BlockKind::Alt => "alt",
                        BlockKind::Opt => "opt",
                        BlockKind::Par => "par",
                        BlockKind::Critical => "critical",
                        BlockKind::Break => "break",
                        BlockKind::Rect => "",
                    };
                    let tw = text_size(measure, name, size * 0.85, true).0;
                    let tab_h = size * 1.5;
                    let p = Point::new;
                    let tab = vec![
                        Seg::Move(p(x0, b.top)),
                        Seg::Line(p(x0 + tw + 18.0, b.top)),
                        Seg::Line(p(x0 + tw + 18.0, b.top + tab_h - 6.0)),
                        Seg::Line(p(x0 + tw + 12.0, b.top + tab_h)),
                        Seg::Line(p(x0, b.top + tab_h)),
                        Seg::Close,
                    ];
                    front.path(tab, Some(stroke.with_alpha(0.35)), Some(Line::solid(stroke, 1.0)));
                    let at = Point::new(x0 + 8.0, b.top + tab_h / 2.0);
                    text_lines(&mut front, name, at, size * 0.85, look.text, Anchor::Start);
                    if !b.label.is_empty() {
                        let at = Point::new(x0 + tw + 26.0, b.top + tab_h / 2.0);
                        text_lines(
                            &mut front,
                            &format!("[{}]", b.label),
                            at,
                            size * 0.88,
                            look.muted,
                            Anchor::Start,
                        );
                    }
                    for (py, label) in &b.parts {
                        back.path(
                            vec![Seg::Move(p(x0, *py)), Seg::Line(p(x1, *py))],
                            None,
                            Some(Line::dashed(stroke, 1.0)),
                        );
                        if !label.is_empty() {
                            let at = Point::new((x0 + x1) / 2.0, py + size * 0.75 + 2.0);
                            text_lines(
                                &mut front,
                                &format!("[{label}]"),
                                at,
                                size * 0.88,
                                look.muted,
                                Anchor::Middle,
                            );
                        }
                    }
                }
            }
        }
    }
    // Activations still open run to the end.
    for (p, stack) in active.iter().enumerate() {
        for (depth, top) in stack.iter().enumerate() {
            bars.push((p, depth, *top, y));
        }
    }
    let bottom_top = y + 10.0;
    let total_h = bottom_top + head_h;

    let mut out = Scene::default();
    // Boxes around participants, behind everything.
    for group in &seq.boxes {
        let (Some(&a), Some(&z)) = (group.members.iter().min(), group.members.iter().max()) else {
            continue;
        };
        let x0 = xs[a] - widths[a] / 2.0 - 12.0;
        let x1 = xs[z] + widths[z] / 2.0 + 12.0;
        let title_h = if group.title.is_empty() { 0.0 } else { size * 1.6 };
        let r = Rect::new(x0, -title_h - 8.0, x1 - x0, total_h + title_h + 16.0);
        let fill = group.color.map_or(look.group, |c| c.with_alpha(0.22));
        out.rect(r, 6.0, Some(fill), Some(Line::solid(look.group_stroke, 1.0)));
        if !group.title.is_empty() {
            text_lines(
                &mut out,
                &group.title,
                Point::new(r.center().x, r.min_y() + title_h / 2.0 + 4.0),
                size,
                look.text,
                Anchor::Middle,
            );
        }
        min_x = min_x.min(x0);
        max_x = max_x.max(x1);
    }
    out.prims.append(&mut back.prims);
    // Lifelines.
    for (p, x) in xs.iter().enumerate().take(seq.participants.len()) {
        let _ = p;
        out.path(
            vec![Seg::Move(Point::new(*x, head_h)), Seg::Line(Point::new(*x, bottom_top))],
            None,
            Some(Line { color: look.group_stroke, width: 1.0, dash: Some((4.0, 4.0)) }),
        );
    }
    for (p, depth, top, bottom) in bars {
        let x = xs[p] - ACTIVATION_W / 2.0 + depth as f32 * 4.0;
        out.rect(
            Rect::new(x, top, ACTIVATION_W, (bottom - top).max(8.0)),
            1.0,
            Some(look.fill),
            Some(Line::solid(look.stroke, 1.0)),
        );
    }
    out.prims.append(&mut front.prims);
    for (p, participant) in seq.participants.iter().enumerate() {
        for top in [0.0, bottom_top] {
            participant_box(
                &mut out,
                participant.actor,
                &participant.label,
                (xs[p], top),
                (widths[p], head_h),
                look,
            );
        }
    }
    let has_box_titles = seq.boxes.iter().any(|b| !b.title.is_empty());
    let min_y = if seq.boxes.is_empty() {
        0.0
    } else if has_box_titles {
        -size * 1.6 - 8.0
    } else {
        -8.0
    };
    let max_y = if seq.boxes.is_empty() { total_h } else { total_h + 8.0 };
    out.width = max_x - min_x;
    out.height = max_y - min_y;
    out.shift(-min_x, -min_y);
    out
}

/// A participant's box (or an actor's figure) with its name.
fn participant_box(
    scene: &mut Scene,
    actor: bool,
    label: &str,
    (x, top): (f32, f32),
    (w, head_h): (f32, f32),
    look: &Look,
) {
    if actor {
        let stroke = Line::solid(look.stroke, 1.6);
        let c = Point::new(x, top + 10.0);
        scene.path(
            scene::ellipse(Rect::new(c.x - 8.0, c.y - 8.0, 16.0, 16.0)),
            Some(look.fill),
            Some(stroke),
        );
        let p = Point::new;
        scene.polyline(&[p(x, top + 18.0), p(x, top + 38.0)], stroke);
        scene.polyline(&[p(x - 14.0, top + 25.0), p(x + 14.0, top + 25.0)], stroke);
        scene.polyline(
            &[p(x - 12.0, top + 54.0), p(x, top + 38.0), p(x + 12.0, top + 54.0)],
            stroke,
        );
        label_texts(
            scene,
            label,
            Point::new(x, top + ACTOR_H + look.size * 0.6),
            look.size,
            look.text,
            false,
        );
    } else {
        let h = BOX_H;
        let y = top + (head_h - h) / 2.0;
        let r = Rect::new(x - w / 2.0, y, w, h);
        scene.rect(r, 4.0, Some(look.fill), Some(Line::solid(look.stroke, 1.3)));
        label_texts(scene, label, r.center(), look.size, look.text, false);
    }
}

/// A message's head at `to` (coming from `from`).
fn head_at(scene: &mut Scene, tip: Tip, from: Point, to: Point, color: Rgba, look: &Look) {
    let kind = match tip {
        Tip::None => return,
        Tip::Filled => HeadKind::Arrow,
        Tip::Cross => HeadKind::Cross,
        Tip::Open => HeadKind::Open,
    };
    scene::head(scene, from, to, kind, 9.0, color, look.background);
}

/// Lines of text from `at` (its middle), anchored.
fn text_lines(scene: &mut Scene, text: &str, at: Point, size: f32, color: Rgba, anchor: Anchor) {
    let lines: Vec<&str> = text.lines().collect();
    let line_h = size * 1.25;
    let top = at.y - line_h * lines.len() as f32 / 2.0 + line_h / 2.0;
    for (i, line) in lines.iter().enumerate() {
        scene.prims.push(Prim::Text(Text {
            at: Point::new(at.x, top + i as f32 * line_h),
            text: (*line).to_owned(),
            size,
            bold: false,
            italic: false,
            mono: false,
            math: false,
            color,
            anchor,
        }));
    }
}

/// Each block's span of participants (the messages and notes inside it), in order.
fn block_spans(seq: &Sequence) -> Vec<Option<(usize, usize)>> {
    let mut out: Vec<Option<(usize, usize)>> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for item in &seq.items {
        let touched: Option<(usize, usize)> = match item {
            Item::Start { .. } => {
                stack.push(out.len());
                out.push(None);
                None
            }
            Item::End => {
                stack.pop();
                None
            }
            Item::Message { from, to, .. } => Some(((*from).min(*to), (*from).max(*to))),
            Item::Note { at: Placement::Over(a, b), .. } => Some(((*a).min(*b), (*a).max(*b))),
            Item::Note { at: Placement::LeftOf(a) | Placement::RightOf(a), .. } => Some((*a, *a)),
            _ => None,
        };
        if let Some((a, b)) = touched {
            for &s in &stack {
                out[s] = Some(match out[s] {
                    Some((x, y)) => (x.min(a), y.max(b)),
                    None => (a, b),
                });
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/sequence.rs"]
mod tests;
