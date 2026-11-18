//! A state diagram as a [`graph`](super::graph): states as rounded boxes (their
//! descriptions under a line), the start as a dot and the end as a ringed dot, forks and
//! joins as bars, choices as small diamonds, composite states as frames (their concurrent
//! regions as dashed frames inside), notes beside their state, transitions with labels.

use std::collections::HashMap;

use diagram_text::mermaid::state::{Kind, StateDiagram};
use kernel::{Point, Rect};

use super::graph::{self, End, Graph, Outline};
use super::scene::{self, HeadKind, Line, Scene, Seg};
use super::{Look, Measure, label_texts, text_size};

pub fn layout(d: &StateDiagram, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let across = d.direction.horizontal();
    // Composite states are groups; one with several regions, a group per region in it.
    let mut groups: Vec<graph::Group> = Vec::new();
    let mut group_of_state: HashMap<usize, usize> = HashMap::new();
    let mut region_groups: HashMap<(usize, usize), usize> = HashMap::new();
    // Parents before children: states are listed as they're met, a composite before what
    // it holds.
    for (i, s) in d.states.iter().enumerate() {
        if !s.composite {
            continue;
        }
        let parent =
            s.parent.and_then(|p| container(d, &group_of_state, &region_groups, p, s.region));
        let g = groups.len();
        groups.push(graph::Group {
            title: s.label.clone(),
            parent,
            direction: None,
            style: s.style,
        });
        group_of_state.insert(i, g);
        if s.regions > 1 {
            for r in 0..s.regions {
                let rg = groups.len();
                let style = diagram_text::mermaid::Style {
                    dashed: true,
                    fill: Some(kernel::Rgba::TRANSPARENT),
                    ..Default::default()
                };
                groups.push(graph::Group {
                    title: String::new(),
                    parent: Some(g),
                    direction: None,
                    style,
                });
                region_groups.insert((i, r), rg);
            }
        }
    }
    let mut node_of_state: HashMap<usize, usize> = HashMap::new();
    let mut nodes: Vec<graph::Node> = Vec::new();
    let mut kinds: Vec<usize> = Vec::new();
    for (i, s) in d.states.iter().enumerate() {
        if s.composite {
            continue;
        }
        let group =
            s.parent.and_then(|p| container(d, &group_of_state, &region_groups, p, s.region));
        let (w, h, outline) = match s.kind {
            Kind::Start => (16.0, 16.0, Outline::Ellipse),
            Kind::End => (20.0, 20.0, Outline::Ellipse),
            Kind::Fork | Kind::Join => {
                if across {
                    (8.0, 64.0, Outline::Rect)
                } else {
                    (64.0, 8.0, Outline::Rect)
                }
            }
            Kind::Choice => (28.0, 28.0, Outline::Diamond),
            Kind::Normal => {
                let (mut w, mut h) = text_size(measure, &s.label, size, false);
                for line in &s.descriptions {
                    let (dw, dh) = text_size(measure, line, size * 0.9, false);
                    w = w.max(dw);
                    h += dh;
                }
                if !s.descriptions.is_empty() {
                    h += 10.0;
                }
                (w + 30.0, (h + 18.0).max(38.0), Outline::Rect)
            }
        };
        node_of_state.insert(i, nodes.len());
        nodes.push(graph::Node { size: (w, h), outline, group });
        kinds.push(i);
    }
    let end_of = |s: usize| -> End {
        match group_of_state.get(&s) {
            Some(&g) => End::Group(g),
            None => End::Node(node_of_state[&s]),
        }
    };
    let mut edges: Vec<graph::Edge> = d
        .transitions
        .iter()
        .map(|t| graph::Edge {
            from: end_of(t.from),
            to: end_of(t.to),
            label: t.label.clone(),
            from_label: None,
            to_label: None,
            line: Some(Line::solid(look.line, 1.4)),
            head: Some(HeadKind::Arrow),
            tail: None,
            length: 1,
            label_color: None,
        })
        .collect();
    let states = nodes.len();
    for note in &d.notes {
        let (w, h) = text_size(measure, &note.text, size * 0.92, false);
        let state = &d.states[note.state];
        let group = if state.composite {
            group_of_state.get(&note.state).and_then(|&g| groups[g].parent)
        } else {
            nodes.get(node_of_state[&note.state]).and_then(|n| n.group)
        };
        let n = nodes.len();
        nodes.push(graph::Node { size: (w + 20.0, h + 16.0), outline: Outline::Rect, group });
        let (from, to) = if note.left {
            (End::Node(n), end_of(note.state))
        } else {
            (end_of(note.state), End::Node(n))
        };
        edges.push(graph::Edge {
            from,
            to,
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
    let g = Graph { direction: d.direction, nodes, edges, groups };
    let mut scene = Scene::default();
    graph::draw(&g, look, measure, &mut scene, |scene, n, r| {
        if n >= states {
            let note = &d.notes[n - states];
            scene.rect(r, 3.0, Some(look.note), Some(Line::solid(look.note_stroke, 1.0)));
            label_texts(scene, &note.text, r.center(), size * 0.92, look.text, false);
            return;
        }
        let s = &d.states[kinds[n]];
        let fill = s.style.fill.unwrap_or(look.fill);
        let stroke = s.style.stroke.unwrap_or(look.stroke);
        let text = s.style.color.unwrap_or(look.text);
        match s.kind {
            Kind::Start => scene.path(scene::ellipse(r), Some(look.text), None),
            Kind::End => {
                scene.path(
                    scene::ellipse(r),
                    Some(look.background),
                    Some(Line::solid(look.text, 1.5)),
                );
                scene.path(scene::ellipse(super::grow(r, -4.5)), Some(look.text), None);
            }
            Kind::Fork | Kind::Join => scene.rect(r, 2.0, Some(look.text), None),
            Kind::Choice => {
                let c = r.center();
                let p = Point::new;
                scene.path(
                    scene::polygon(&[
                        p(c.x, r.min_y()),
                        p(r.max_x(), c.y),
                        p(c.x, r.max_y()),
                        p(r.min_x(), c.y),
                    ]),
                    Some(fill),
                    Some(Line::solid(stroke, 1.3)),
                );
            }
            Kind::Normal => {
                scene.rect(r, 10.0, Some(fill), Some(Line::solid(stroke, 1.3)));
                if s.descriptions.is_empty() {
                    label_texts(scene, &s.label, r.center(), size, text, false);
                } else {
                    let line_h = size * 1.3;
                    let title_y = r.min_y() + 9.0 + line_h / 2.0;
                    label_texts(
                        scene,
                        &s.label,
                        Point::new(r.center().x, title_y),
                        size,
                        text,
                        true,
                    );
                    let split =
                        r.min_y() + 9.0 + line_h * s.label.lines().count().max(1) as f32 + 3.0;
                    scene.path(
                        vec![
                            Seg::Move(Point::new(r.min_x(), split)),
                            Seg::Line(Point::new(r.max_x(), split)),
                        ],
                        None,
                        Some(Line::solid(stroke.with_alpha(0.7), 1.0)),
                    );
                    let body = Rect::new(r.min_x(), split, r.size.width, r.max_y() - split);
                    label_texts(
                        scene,
                        &s.descriptions.join("\n"),
                        body.center(),
                        size * 0.9,
                        look.muted,
                        false,
                    );
                }
            }
        }
    });
    scene
}

/// The group a state of composite `parent` (in its `region`) goes in.
fn container(
    d: &StateDiagram,
    groups: &HashMap<usize, usize>,
    regions: &HashMap<(usize, usize), usize>,
    parent: usize,
    region: usize,
) -> Option<usize> {
    if d.states[parent].regions > 1 {
        regions.get(&(parent, region)).copied()
    } else {
        groups.get(&parent).copied()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/state.rs"]
mod tests;
