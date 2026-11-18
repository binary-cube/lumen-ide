//! A class diagram as a [`graph`](super::graph): each class a card (its annotation, its
//! name, its attributes, its methods), namespaces as groups, notes as their own boxes tied
//! to their class, relations with UML's ends (hollow triangle, filled or hollow diamond,
//! open arrow) and their cardinalities.

use diagram_text::mermaid::class::{ClassDiagram, Mark, Member};
use kernel::{Point, Rect, Rgba};

use super::graph::{self, End, Graph, Outline};
use super::scene::{Anchor, HeadKind, Line, Scene, Seg, Text};
use super::{Look, Measure, label_texts, text_size};

const ROW: f32 = 1.45;
const PAD: f32 = 12.0;

pub fn layout(d: &ClassDiagram, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let row = size * ROW;
    let mut group_of = vec![None; d.classes.len()];
    for (g, (_, members)) in d.namespaces.iter().enumerate() {
        for &c in members {
            group_of[c] = Some(g);
        }
    }
    let mut nodes: Vec<graph::Node> = d
        .classes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let header = if c.annotation.is_some() { row * 2.0 + 6.0 } else { row + 10.0 };
            let mut w = text_size(measure, &c.label, size, true).0;
            if let Some(a) = &c.annotation {
                w = w.max(text_size(measure, &format!("«{a}»"), size * 0.85, false).0);
            }
            for m in c.attributes.iter().chain(&c.methods) {
                w = w.max(text_size(measure, &m.text, size * 0.92, false).0);
            }
            let rows = c.attributes.len().max(1) + c.methods.len().max(1);
            let h = header + rows as f32 * row * 0.92 + 18.0;
            graph::Node { size: (w + 2.0 * PAD, h), outline: Outline::Rect, group: group_of[i] }
        })
        .collect();
    let classes = nodes.len();
    let mut edges: Vec<graph::Edge> = d
        .relations
        .iter()
        .map(|r| graph::Edge {
            from: End::Node(r.from),
            to: End::Node(r.to),
            label: r.label.clone(),
            from_label: r.from_card.clone(),
            to_label: r.to_card.clone(),
            line: Some(if r.dashed {
                Line::dashed(look.line, 1.4)
            } else {
                Line::solid(look.line, 1.4)
            }),
            head: mark(r.to_mark),
            tail: mark(r.from_mark),
            length: 1,
            label_color: None,
        })
        .collect();
    // Notes: boxes of their own, a dashed line to their class.
    for note in &d.notes {
        let (w, h) = text_size(measure, &note.text, size * 0.92, false);
        let n = nodes.len();
        let group = note.class.and_then(|c| group_of[c]);
        nodes.push(graph::Node { size: (w + 20.0, h + 16.0), outline: Outline::Rect, group });
        if let Some(c) = note.class {
            edges.push(graph::Edge {
                from: End::Node(n),
                to: End::Node(c),
                label: None,
                from_label: None,
                to_label: None,
                line: Some(Line::dashed(look.note_stroke, 1.0)),
                head: None,
                tail: None,
                length: 1,
                label_color: None,
            });
        }
    }
    let groups = d
        .namespaces
        .iter()
        .map(|(name, _)| graph::Group { title: name.clone(), ..graph::Group::default() })
        .collect();
    let g = Graph { direction: d.direction, nodes, edges, groups };
    let mut scene = Scene::default();
    let row_h = row * 0.92;
    // The static members' widths (their underline).
    let widths: Vec<Vec<f32>> = d
        .classes
        .iter()
        .map(|c| {
            c.attributes
                .iter()
                .chain(&c.methods)
                .map(|m| {
                    if m.is_static {
                        text_size(measure, &m.text, size * 0.92, false).0
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    graph::draw(&g, look, measure, &mut scene, |scene, n, r| {
        if n >= classes {
            let note = &d.notes[n - classes];
            scene.rect(r, 3.0, Some(look.note), Some(Line::solid(look.note_stroke, 1.0)));
            label_texts(scene, &note.text, r.center(), size * 0.92, look.text, false);
            return;
        }
        let c = &d.classes[n];
        let header = if c.annotation.is_some() { row * 2.0 + 6.0 } else { row + 10.0 };
        let fill = c.style.fill.unwrap_or(look.fill);
        let stroke = c.style.stroke.unwrap_or(look.stroke);
        let text = c.style.color.unwrap_or(look.text);
        card_frame(scene, r, header, fill, stroke);
        let mut y = r.min_y() + 5.0 + row / 2.0;
        if let Some(a) = &c.annotation {
            scene.text(Text {
                at: Point::new(r.center().x, y),
                text: format!("«{a}»"),
                size: size * 0.85,
                bold: false,
                italic: true,
                mono: false,
                math: false,
                color: look.muted,
                anchor: Anchor::Middle,
            });
            y += row;
        }
        scene.text(Text {
            at: Point::new(r.center().x, y),
            text: c.label.clone(),
            size,
            bold: true,
            italic: c.annotation.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("abstract")),
            mono: false,
            math: false,
            color: text,
            anchor: Anchor::Middle,
        });
        let mut y = r.min_y() + header + 5.0;
        let rows = |scene: &mut Scene, members: &[Member], first: usize, y: &mut f32| {
            for (i, m) in members.iter().enumerate() {
                *y += row_h / 2.0;
                scene.text(Text {
                    at: Point::new(r.min_x() + PAD, *y),
                    text: m.text.clone(),
                    size: size * 0.92,
                    bold: false,
                    italic: m.is_abstract,
                    mono: false,
                    math: false,
                    color: text,
                    anchor: Anchor::Start,
                });
                if m.is_static {
                    let w = widths[n][first + i];
                    let uy = *y + size * 0.55;
                    scene.path(
                        vec![
                            Seg::Move(Point::new(r.min_x() + PAD, uy)),
                            Seg::Line(Point::new(r.min_x() + PAD + w, uy)),
                        ],
                        None,
                        Some(Line::solid(text, 1.0)),
                    );
                }
                *y += row_h / 2.0;
            }
            if members.is_empty() {
                *y += row_h;
            }
        };
        rows(scene, &c.attributes, 0, &mut y);
        let split = y + 3.0;
        scene.path(
            vec![Seg::Move(Point::new(r.min_x(), split)), Seg::Line(Point::new(r.max_x(), split))],
            None,
            Some(Line::solid(stroke.with_alpha(0.6), 1.0)),
        );
        y += 6.0;
        rows(scene, &c.methods, c.attributes.len(), &mut y);
        if let Some(url) = &c.link {
            scene.links.push((r, url.clone()));
        }
    });
    scene
}

/// A card's frame: its body, its header tinted, the line under the header.
pub fn card_frame(scene: &mut Scene, r: Rect, header: f32, fill: Rgba, stroke: Rgba) {
    scene.rect(r, 4.0, Some(fill), None);
    let head = Rect::new(r.min_x(), r.min_y(), r.size.width, header);
    scene.path(top_rounded(head, 4.0), Some(stroke.with_alpha(0.16)), None);
    scene.path(
        vec![
            Seg::Move(Point::new(r.min_x(), r.min_y() + header)),
            Seg::Line(Point::new(r.max_x(), r.min_y() + header)),
        ],
        None,
        Some(Line::solid(stroke.with_alpha(0.8), 1.0)),
    );
    scene.rect(r, 4.0, None, Some(Line::solid(stroke, 1.3)));
}

/// A rectangle with only its top corners rounded.
fn top_rounded(r: Rect, rad: f32) -> Vec<Seg> {
    let (x0, y0, x1, y1) = (r.min_x(), r.min_y(), r.max_x(), r.max_y());
    let k = rad * 0.552_284_8;
    let p = Point::new;
    vec![
        Seg::Move(p(x0, y1)),
        Seg::Line(p(x0, y0 + rad)),
        Seg::Cubic(p(x0, y0 + rad - k), p(x0 + rad - k, y0), p(x0 + rad, y0)),
        Seg::Line(p(x1 - rad, y0)),
        Seg::Cubic(p(x1 - rad + k, y0), p(x1, y0 + rad - k), p(x1, y0 + rad)),
        Seg::Line(p(x1, y1)),
        Seg::Close,
    ]
}

fn mark(m: Mark) -> Option<HeadKind> {
    match m {
        Mark::None => None,
        Mark::Inheritance => Some(HeadKind::Hollow),
        Mark::Composition => Some(HeadKind::Diamond),
        Mark::Aggregation => Some(HeadKind::HollowDiamond),
        Mark::Arrow => Some(HeadKind::Open),
        Mark::Lollipop => Some(HeadKind::Circle),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/class.rs"]
mod tests;
