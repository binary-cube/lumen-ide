//! An entity-relationship diagram as a [`graph`](super::graph): each entity a card (its
//! name, then its attributes in columns: type, name, keys, comment), relationships with
//! crow's feet at both ends (their cardinality), dashed when not identifying, labeled.

use diagram_text::mermaid::er::{Cardinality, ErDiagram};
use kernel::Point;

use super::class::card_frame;
use super::graph::{self, End, Graph, Outline};
use super::scene::{Anchor, HeadKind, Line, Scene, Text};
use super::{Look, Measure, text_size};

const PAD: f32 = 10.0;
const GAP: f32 = 14.0;

pub fn layout(d: &ErDiagram, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let small = size * 0.9;
    let row = small * 1.5;
    let header = size * 1.4 + 12.0;
    // Each entity's columns' widths.
    let columns: Vec<[f32; 4]> = d
        .entities
        .iter()
        .map(|e| {
            let mut w = [0.0f32; 4];
            for a in &e.attributes {
                w[0] = w[0].max(text_size(measure, &a.kind, small, false).0);
                w[1] = w[1].max(text_size(measure, &a.name, small, false).0);
                w[2] = w[2].max(text_size(measure, &a.keys.join(", "), small, false).0);
                if let Some(c) = &a.comment {
                    w[3] = w[3].max(text_size(measure, c, small, false).0);
                }
            }
            w
        })
        .collect();
    let nodes = d
        .entities
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let cols: f32 = columns[i].iter().filter(|w| **w > 0.0).map(|w| w + GAP).sum();
            let name = text_size(measure, &e.label, size, true).0;
            let w = (cols - GAP).max(name) + 2.0 * PAD;
            let h = header
                + if e.attributes.is_empty() { 0.0 } else { e.attributes.len() as f32 * row + 8.0 };
            graph::Node { size: (w.max(90.0), h), outline: Outline::Rect, group: None }
        })
        .collect();
    let edges = d
        .relations
        .iter()
        .map(|r| graph::Edge {
            from: End::Node(r.from),
            to: End::Node(r.to),
            label: (!r.label.is_empty()).then(|| r.label.clone()),
            from_label: None,
            to_label: None,
            line: Some(if r.identifying {
                Line::solid(look.line, 1.4)
            } else {
                Line::dashed(look.line, 1.4)
            }),
            head: Some(foot(r.to_card)),
            tail: Some(foot(r.from_card)),
            length: 1,
            label_color: None,
        })
        .collect();
    let g = Graph { direction: d.direction, nodes, edges, groups: Vec::new() };
    let mut scene = Scene::default();
    graph::draw(&g, look, measure, &mut scene, |scene, n, r| {
        let e = &d.entities[n];
        let h = if e.attributes.is_empty() { r.size.height } else { header };
        card_frame(scene, r, h, look.fill, look.stroke);
        let text = |at: Point, s: &str, size: f32, bold: bool, color, anchor| Text {
            at,
            text: s.to_owned(),
            size,
            bold,
            italic: false,
            mono: false,
            math: false,
            color,
            anchor,
        };
        scene.text(text(
            Point::new(r.center().x, r.min_y() + h / 2.0),
            &e.label,
            size,
            true,
            look.text,
            Anchor::Middle,
        ));
        let cols = columns[n];
        for (i, a) in e.attributes.iter().enumerate() {
            let y = r.min_y() + header + 4.0 + row * (i as f32 + 0.5);
            let mut x = r.min_x() + PAD;
            let cells = [(&a.kind, look.muted), (&a.name, look.text)];
            for (c, (value, color)) in cells.into_iter().enumerate() {
                scene.text(text(Point::new(x, y), value, small, false, color, Anchor::Start));
                x += cols[c] + GAP;
            }
            if cols[2] > 0.0 {
                scene.text(text(
                    Point::new(x, y),
                    &a.keys.join(", "),
                    small,
                    true,
                    look.accent,
                    Anchor::Start,
                ));
                x += cols[2] + GAP;
            }
            if let Some(comment) = &a.comment {
                scene.text(text(
                    Point::new(x, y),
                    comment,
                    small,
                    false,
                    look.muted,
                    Anchor::Start,
                ));
            }
        }
    });
    scene
}

fn foot(c: Cardinality) -> HeadKind {
    match c {
        Cardinality::ZeroOrOne => HeadKind::ZeroOrOne,
        Cardinality::ExactlyOne => HeadKind::ExactlyOne,
        Cardinality::ZeroOrMore => HeadKind::ZeroOrMore,
        Cardinality::OneOrMore => HeadKind::OneOrMore,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/er.rs"]
mod tests;
