//! A flowchart as a [`graph`](super::graph): its nodes in every shape Mermaid has, its
//! subgraphs as groups, its links' lines, heads and labels, its styles.

use diagram_text::mermaid::Style;
use diagram_text::mermaid::flowchart::{End, Flowchart, Head, Shape, Stroke};
use kernel::{Point, Rect};

use super::graph::{self, Graph, Outline};
use super::scene::{self, HeadKind, Line, Scene, Seg};
use super::{Look, Measure, label_texts, text_size};

pub fn layout(f: &Flowchart, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let mut group_of = vec![None; f.nodes.len()];
    for (g, group) in f.subgraphs.iter().enumerate() {
        for &n in &group.nodes {
            group_of[n] = Some(g);
        }
    }
    let nodes = f
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| graph::Node {
            size: node_size(n.shape, text_size(measure, &n.label, size, false)),
            outline: match n.shape {
                Shape::Circle | Shape::DoubleCircle => Outline::Ellipse,
                Shape::Diamond => Outline::Diamond,
                _ => Outline::Rect,
            },
            group: group_of[i],
        })
        .collect();
    let end = |e: End| match e {
        End::Node(n) => graph::End::Node(n),
        End::Group(g) => graph::End::Group(g),
    };
    let edges = f
        .edges
        .iter()
        .map(|e| {
            let color = e.style.stroke.unwrap_or(look.line);
            let width = e.style.stroke_width.unwrap_or(match e.stroke {
                Stroke::Thick => 3.0,
                _ => 1.5,
            });
            let line = match e.stroke {
                Stroke::Invisible => None,
                Stroke::Dotted => Some(Line { color, width, dash: Some((3.0, 3.0)) }),
                _ if e.style.dashed => Some(Line { color, width, dash: Some((5.0, 4.0)) }),
                _ => Some(Line::solid(color, width)),
            };
            graph::Edge {
                from: end(e.from),
                to: end(e.to),
                label: e.label.clone(),
                from_label: None,
                to_label: None,
                line,
                head: head_kind(e.head),
                tail: head_kind(e.tail),
                length: e.length,
                label_color: e.style.color,
            }
        })
        .collect();
    let groups = f
        .subgraphs
        .iter()
        .map(|g| graph::Group {
            title: g.title.clone(),
            parent: g.parent,
            direction: g.direction,
            style: g.style,
        })
        .collect();
    let g = Graph { direction: f.direction, nodes, edges, groups };
    let mut scene = Scene::default();
    graph::draw(&g, look, measure, &mut scene, |scene, n, r| {
        let node = &f.nodes[n];
        draw_node(scene, node.shape, r, &node.style, look);
        let color = node.style.color.unwrap_or(look.text);
        label_texts(scene, &node.label, r.center(), size, color, node.style.bold);
        if let Some(url) = &node.link {
            scene.links.push((r, url.clone()));
        }
    });
    scene
}

/// A node's size around its label's.
pub(crate) fn node_size(shape: Shape, (tw, th): (f32, f32)) -> (f32, f32) {
    let (w, h) = (tw + 30.0, (th + 18.0).max(38.0));
    match shape {
        Shape::Circle => {
            let d = tw.max(th) + 26.0;
            (d, d)
        }
        Shape::DoubleCircle => {
            let d = tw.max(th) + 36.0;
            (d, d)
        }
        Shape::Diamond => {
            let (a, b) = (tw + 16.0, th + 10.0);
            (a * 1.55 + 18.0, (b * 1.6 + 18.0).max(56.0))
        }
        Shape::Hexagon => (w + h * 0.6, h),
        Shape::Stadium => (tw + h, h),
        Shape::Subroutine => (w + 16.0, h),
        Shape::Cylinder => (w, h + 16.0),
        Shape::Parallelogram | Shape::ParallelogramAlt | Shape::Trapezoid | Shape::TrapezoidAlt => {
            (w + h * 0.6, h)
        }
        Shape::Asymmetric => (w + h * 0.3, h),
        Shape::Rect | Shape::Round => (w, h),
    }
}

fn head_kind(h: Head) -> Option<HeadKind> {
    match h {
        Head::None => None,
        Head::Arrow => Some(HeadKind::Arrow),
        Head::Circle => Some(HeadKind::Circle),
        Head::Cross => Some(HeadKind::Cross),
    }
}

/// A node's shape at `r`, in its style (the theme's where it has none).
pub fn draw_node(scene: &mut Scene, shape: Shape, r: Rect, style: &Style, look: &Look) {
    let fill = Some(style.fill.unwrap_or(look.fill));
    let stroke = style.stroke.unwrap_or(look.stroke);
    let width = style.stroke_width.unwrap_or(1.3);
    let line =
        Some(if style.dashed { Line::dashed(stroke, width) } else { Line::solid(stroke, width) });
    let (x0, y0, x1, y1) = (r.min_x(), r.min_y(), r.max_x(), r.max_y());
    let (w, h) = (r.size.width, r.size.height);
    let p = Point::new;
    match shape {
        Shape::Rect => scene.rect(r, 3.0, fill, line),
        Shape::Round => scene.rect(r, 12.0, fill, line),
        Shape::Stadium => scene.rect(r, h / 2.0, fill, line),
        Shape::Subroutine => {
            scene.rect(r, 2.0, fill, line);
            let l = Line::solid(stroke, width);
            scene.polyline(&[p(x0 + 8.0, y0), p(x0 + 8.0, y1)], l);
            scene.polyline(&[p(x1 - 8.0, y0), p(x1 - 8.0, y1)], l);
        }
        Shape::Cylinder => {
            let ry = 8.0f32.min(h / 4.0);
            let k = 0.552_284_8;
            let cx = r.center().x;
            let rx = w / 2.0;
            let body = vec![
                Seg::Move(p(x0, y0 + ry)),
                Seg::Line(p(x0, y1 - ry)),
                Seg::Cubic(p(x0, y1 - ry + ry * k), p(cx - rx * k, y1), p(cx, y1)),
                Seg::Cubic(p(cx + rx * k, y1), p(x1, y1 - ry + ry * k), p(x1, y1 - ry)),
                Seg::Line(p(x1, y0 + ry)),
                Seg::Cubic(p(x1, y0 + ry - ry * k), p(cx + rx * k, y0), p(cx, y0)),
                Seg::Cubic(p(cx - rx * k, y0), p(x0, y0 + ry - ry * k), p(x0, y0 + ry)),
                Seg::Close,
            ];
            scene.path(body, fill, line);
            // The rim's near half.
            let rim = vec![
                Seg::Move(p(x0, y0 + ry)),
                Seg::Cubic(
                    p(x0, y0 + ry + ry * k),
                    p(cx - rx * k, y0 + 2.0 * ry),
                    p(cx, y0 + 2.0 * ry),
                ),
                Seg::Cubic(p(cx + rx * k, y0 + 2.0 * ry), p(x1, y0 + ry + ry * k), p(x1, y0 + ry)),
            ];
            scene.path(rim, None, line);
        }
        Shape::Circle => scene.path(scene::ellipse(r), fill, line),
        Shape::DoubleCircle => {
            scene.path(scene::ellipse(r), fill, line);
            scene.path(scene::ellipse(super::grow(r, -5.0)), None, line);
        }
        Shape::Diamond => {
            let c = r.center();
            scene.path(
                scene::polygon(&[p(c.x, y0), p(x1, c.y), p(c.x, y1), p(x0, c.y)]),
                fill,
                line,
            );
        }
        Shape::Hexagon => {
            let s = h * 0.3;
            let c = r.center();
            scene.path(
                scene::polygon(&[
                    p(x0 + s, y0),
                    p(x1 - s, y0),
                    p(x1, c.y),
                    p(x1 - s, y1),
                    p(x0 + s, y1),
                    p(x0, c.y),
                ]),
                fill,
                line,
            );
        }
        Shape::Parallelogram => {
            let s = h * 0.3;
            scene.path(
                scene::polygon(&[p(x0 + s, y0), p(x1, y0), p(x1 - s, y1), p(x0, y1)]),
                fill,
                line,
            );
        }
        Shape::ParallelogramAlt => {
            let s = h * 0.3;
            scene.path(
                scene::polygon(&[p(x0, y0), p(x1 - s, y0), p(x1, y1), p(x0 + s, y1)]),
                fill,
                line,
            );
        }
        Shape::Trapezoid => {
            let s = h * 0.3;
            scene.path(
                scene::polygon(&[p(x0 + s, y0), p(x1 - s, y0), p(x1, y1), p(x0, y1)]),
                fill,
                line,
            );
        }
        Shape::TrapezoidAlt => {
            let s = h * 0.3;
            scene.path(
                scene::polygon(&[p(x0, y0), p(x1, y0), p(x1 - s, y1), p(x0 + s, y1)]),
                fill,
                line,
            );
        }
        Shape::Asymmetric => {
            let s = h * 0.3;
            let c = r.center();
            scene.path(
                scene::polygon(&[p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1), p(x0 + s, c.y)]),
                fill,
                line,
            );
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/flowchart.rs"]
mod tests;
