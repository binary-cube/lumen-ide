//! The diagrams that are boxes with words in them: requirements and their relations, C4
//! (people, systems, containers, components in their boundaries), architecture (services
//! and junctions in groups) — laid out as a [`graph`](super::graph); blocks on a grid of
//! columns (nested, spanning, with arrows between them) and a kanban board (columns of
//! cards).

use diagram_text::mermaid::Style;
use diagram_text::mermaid::block::BlockDiagram;
use diagram_text::mermaid::boxes::{BoxGraph, BoxNode};
use diagram_text::mermaid::flowchart::Shape;
use diagram_text::mermaid::tree::Kanban;
use kernel::{Point, Rect, Rgba};

use super::charts::{text, wrap};
use super::flowchart::{draw_node, node_size};
use super::graph::{self, Graph, Outline};
use super::scene::{self, Anchor, HeadKind, Line, Scene, Seg};
use super::{Look, Measure, label_texts, text_size};

/// Which of the three a [`BoxGraph`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Requirement,
    C4,
    Architecture,
}

/// A box's lines: what it is (small, over), its title (bold), what it says.
struct Card {
    kind: Option<String>,
    title: String,
    lines: Vec<String>,
}

fn card_of(n: &BoxNode, kind: Kind) -> Card {
    match kind {
        Kind::Requirement => Card {
            kind: Some(format!("«{}»", if n.kind.is_empty() { "requirement" } else { &n.kind })),
            title: n.title.clone(),
            lines: n.lines.clone(),
        },
        Kind::C4 => {
            let base = n.kind.trim_end_matches("_Ext");
            let what = match base {
                "Person" => "Person",
                "System" | "SystemDb" | "SystemQueue" => "Software System",
                "Container" | "ContainerDb" | "ContainerQueue" => "Container",
                "Component" | "ComponentDb" | "ComponentQueue" => "Component",
                _ => "",
            };
            let mut lines = n.lines.clone();
            // A technology joins what it is: `[Container: Rust]`.
            let kind = match lines.first() {
                Some(t) if t.starts_with('[') && !what.is_empty() => {
                    let tech = lines.remove(0);
                    Some(format!("[{what}: {}]", tech.trim_matches(['[', ']'])))
                }
                _ if !what.is_empty() => Some(format!("[{what}]")),
                _ => None,
            };
            Card { kind, title: n.title.clone(), lines }
        }
        Kind::Architecture => Card { kind: None, title: n.title.clone(), lines: Vec::new() },
    }
}

/// C4's colors, as its notation has them: an external box's grey, then a person's, a
/// system's, a container's, a component's blue (each lighter than the one before).
const C4_EXTERNAL: Rgba = Rgba::rgb(0x999999);
const C4_PERSON: Rgba = Rgba::rgb(0x08427b);
const C4_SYSTEM: Rgba = Rgba::rgb(0x1168bd);
const C4_CONTAINER: Rgba = Rgba::rgb(0x438dd5);
const C4_COMPONENT: Rgba = Rgba::rgb(0x85bbf0);
/// A label's dark ink on a light fill.
const DARK_INK: Rgba = Rgba::rgb(0x101014);

/// C4's colors by what a box is (its notation's, toned to the theme's background).
fn c4_fill(kind: &str, look: &Look) -> Rgba {
    let ext = kind.ends_with("_Ext");
    let base = kind.trim_end_matches("_Ext");
    let c = if ext {
        C4_EXTERNAL
    } else if base.starts_with("Person") {
        C4_PERSON
    } else if base.starts_with("System") {
        C4_SYSTEM
    } else if base.starts_with("Container") {
        C4_CONTAINER
    } else if base.starts_with("Component") {
        C4_COMPONENT
    } else {
        look.fill
    };
    c.mix(look.background, 0.12)
}

/// Black or white, whichever reads on `fill`.
fn ink_on(fill: Rgba) -> Rgba {
    let l = 0.299 * fill.r + 0.587 * fill.g + 0.114 * fill.b;
    if l > 0.6 { DARK_INK } else { Rgba::WHITE }
}

const ICON: f32 = 34.0;

/// A label's height at `size` (as [`text_size`] gives it).
fn height(s: &str, size: f32) -> f32 {
    s.lines().count().max(1) as f32 * size * 1.3
}

pub fn boxes(b: &BoxGraph, kind: Kind, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let small = size * 0.8;
    let width = 230.0;
    let cards: Vec<Card> = b
        .nodes
        .iter()
        .map(|n| {
            let mut c = card_of(n, kind);
            c.lines = c.lines.iter().map(|l| wrap(measure, l, small, width)).collect();
            c.title = wrap(measure, &c.title, size, width);
            c
        })
        .collect();
    let nodes = b
        .nodes
        .iter()
        .zip(&cards)
        .map(|(n, c)| {
            let junction = kind == Kind::Architecture && n.kind == "junction";
            let size = if junction {
                (12.0, 12.0)
            } else {
                let (tw, th) = text_size(measure, &c.title, size, true);
                let mut w = tw;
                let mut h = th;
                if let Some(k) = &c.kind {
                    let (kw, kh) = text_size(measure, k, small, false);
                    w = w.max(kw);
                    h += kh + 2.0;
                }
                for l in &c.lines {
                    let (lw, lh) = text_size(measure, l, small, false);
                    w = w.max(lw);
                    h += lh + 2.0;
                }
                if !c.lines.is_empty() && kind == Kind::Requirement {
                    h += 8.0;
                }
                if kind == Kind::Architecture {
                    (w.max(ICON) + 24.0, h + ICON + 18.0)
                } else if n.kind.starts_with("Person") {
                    (w + 32.0, h + 44.0)
                } else if n.kind.trim_end_matches("_Ext").ends_with("Db") {
                    (w + 32.0, h + 40.0)
                } else {
                    (w + 32.0, h + 24.0)
                }
            };
            graph::Node {
                size,
                outline: if junction { Outline::Ellipse } else { Outline::Rect },
                group: n.group,
            }
        })
        .collect();
    let edges = b
        .edges
        .iter()
        .map(|e| {
            let label = match (&e.label, &e.detail) {
                (Some(l), Some(d)) => Some(format!("{l}\n{d}")),
                (l, d) => l.clone().or_else(|| d.clone()),
            };
            let line =
                if e.dashed { Line::dashed(look.line, 1.3) } else { Line::solid(look.line, 1.4) };
            graph::Edge {
                from: graph::End::Node(e.from),
                to: graph::End::Node(e.to),
                label,
                from_label: None,
                to_label: None,
                line: Some(line),
                head: e.head.then_some(HeadKind::Arrow),
                tail: e.tail.then_some(HeadKind::Arrow),
                length: 1,
                label_color: None,
            }
        })
        .collect();
    let groups = b
        .groups
        .iter()
        .map(|g| graph::Group {
            title: if kind == Kind::C4 && !g.kind.is_empty() {
                format!("{}\n[{}]", g.title, g.kind.replace('_', " "))
            } else {
                g.title.clone()
            },
            parent: g.parent,
            direction: None,
            style: Style { dashed: kind == Kind::C4, ..Style::default() },
        })
        .collect();
    let g = Graph { direction: b.direction, nodes, edges, groups };
    let mut scene = Scene::default();
    graph::draw(&g, look, measure, &mut scene, |scene, i, r| {
        let n = &b.nodes[i];
        let c = &cards[i];
        match kind {
            Kind::Requirement => requirement(scene, c, r, look),
            Kind::C4 => c4(scene, n, c, r, look),
            Kind::Architecture => service(scene, n, c, r, look),
        }
    });
    scene
}

fn requirement(scene: &mut Scene, c: &Card, r: Rect, look: &Look) {
    let size = look.size;
    let small = size * 0.8;
    scene.rect(r, 3.0, Some(look.fill), Some(Line::solid(look.stroke, 1.3)));
    let mut y = r.min_y() + 12.0;
    if let Some(k) = &c.kind {
        let h = height(k, small);
        label_texts(scene, k, Point::new(r.center().x, y + h / 2.0), small, look.muted, false);
        y += h + 2.0;
    }
    let h = height(&c.title, size);
    label_texts(scene, &c.title, Point::new(r.center().x, y + h / 2.0), size, look.text, true);
    y += h + 4.0;
    if c.lines.is_empty() {
        return;
    }
    scene.polyline(
        &[Point::new(r.min_x(), y), Point::new(r.max_x(), y)],
        Line::solid(look.stroke.with_alpha(0.6), 1.0),
    );
    y += 4.0;
    for l in &c.lines {
        for line in l.lines() {
            let h = small * 1.3;
            scene.text(text(
                Point::new(r.min_x() + 12.0, y + h / 2.0),
                line,
                small,
                look.text,
                Anchor::Start,
                false,
            ));
            y += h;
        }
        y += 2.0;
    }
}

fn c4(scene: &mut Scene, n: &BoxNode, c: &Card, r: Rect, look: &Look) {
    let size = look.size;
    let small = size * 0.8;
    let fill = c4_fill(&n.kind, look);
    let ink = ink_on(fill);
    let line = Some(Line::solid(fill.mix(look.text, 0.25), 1.0));
    let base = n.kind.trim_end_matches("_Ext");
    if base.ends_with("Db") {
        draw_node(
            scene,
            Shape::Cylinder,
            r,
            &Style { fill: Some(fill), stroke: line.map(|l| l.color), ..Style::default() },
            look,
        );
    } else if base.ends_with("Queue") {
        scene.rect(r, r.size.height.min(r.size.width) / 4.0, Some(fill), line);
    } else {
        scene.rect(r, 8.0, Some(fill), line);
    }
    let mut y = r.min_y() + if base.ends_with("Db") { 24.0 } else { 12.0 };
    if base.starts_with("Person") {
        // A person's head and shoulders, over its words.
        let cx = r.center().x;
        let head = Rect::new(cx - 7.0, y - 4.0, 14.0, 14.0);
        scene.path(scene::ellipse(head), Some(ink.with_alpha(0.85)), None);
        let mut segs = vec![Seg::Move(Point::new(cx - 13.0, y + 22.0))];
        segs.push(Seg::Cubic(
            Point::new(cx - 13.0, y + 10.0),
            Point::new(cx + 13.0, y + 10.0),
            Point::new(cx + 13.0, y + 22.0),
        ));
        segs.push(Seg::Close);
        scene.path(segs, Some(ink.with_alpha(0.85)), None);
        y += 24.0;
    }
    let h = height(&c.title, size);
    label_texts(scene, &c.title, Point::new(r.center().x, y + h / 2.0), size, ink, true);
    y += h + 2.0;
    if let Some(k) = &c.kind {
        let h = height(k, small);
        label_texts(
            scene,
            k,
            Point::new(r.center().x, y + h / 2.0),
            small,
            ink.with_alpha(0.8),
            false,
        );
        y += h + 2.0;
    }
    for l in &c.lines {
        let h = height(l, small);
        label_texts(scene, l, Point::new(r.center().x, y + h / 2.0 + 4.0), small, ink, false);
        y += h + 2.0;
    }
}

/// An architecture service: its icon over its title (a junction is a dot).
fn service(scene: &mut Scene, n: &BoxNode, c: &Card, r: Rect, look: &Look) {
    if n.kind == "junction" {
        scene.path(scene::ellipse(r), Some(look.line), None);
        return;
    }
    let icon = Rect::new(r.center().x - ICON / 2.0, r.min_y() + 8.0, ICON, ICON);
    scene.rect(icon, 6.0, Some(look.fill), Some(Line::solid(look.stroke, 1.2)));
    let ink = Line::solid(look.accent, 1.6);
    let inner = super::grow(icon, -8.0);
    let (x0, y0, x1, y1) = (inner.min_x(), inner.min_y(), inner.max_x(), inner.max_y());
    let p = Point::new;
    match n.kind.as_str() {
        "database" => {
            draw_node(
                scene,
                Shape::Cylinder,
                inner,
                &Style { fill: Some(look.fill), stroke: Some(look.accent), ..Style::default() },
                look,
            );
        }
        "disk" => {
            scene.path(scene::ellipse(inner), None, Some(ink));
            scene.path(scene::ellipse(super::grow(inner, -6.0)), Some(look.accent), None);
        }
        "internet" => {
            scene.path(scene::ellipse(inner), None, Some(ink));
            scene.path(
                scene::ellipse(Rect::new(inner.center().x - 4.5, y0, 9.0, inner.size.height)),
                None,
                Some(ink),
            );
            scene.polyline(&[p(x0, inner.center().y), p(x1, inner.center().y)], ink);
        }
        "cloud" => {
            let segs = vec![
                Seg::Move(p(x0 + 3.0, y1)),
                Seg::Cubic(p(x0 - 3.0, y1), p(x0 - 2.0, y0 + 6.0), p(x0 + 5.0, y0 + 7.0)),
                Seg::Cubic(p(x0 + 6.0, y0 - 2.0), p(x1 - 3.0, y0 - 3.0), p(x1 - 3.0, y0 + 6.0)),
                Seg::Cubic(p(x1 + 3.0, y0 + 6.0), p(x1 + 3.0, y1), p(x1 - 3.0, y1)),
                Seg::Close,
            ];
            scene.path(segs, None, Some(ink));
        }
        _ => {
            // A server (and anything else): a rack of two units.
            let mid = inner.center().y;
            scene.rect(Rect::new(x0, y0, inner.size.width, mid - y0 - 1.5), 2.0, None, Some(ink));
            scene.rect(
                Rect::new(x0, mid + 1.5, inner.size.width, y1 - mid - 1.5),
                2.0,
                None,
                Some(ink),
            );
            scene.path(
                scene::ellipse(Rect::new(x1 - 6.0, y0 + 3.0, 3.0, 3.0)),
                Some(look.accent),
                None,
            );
            scene.path(
                scene::ellipse(Rect::new(x1 - 6.0, mid + 4.5, 3.0, 3.0)),
                Some(look.accent),
                None,
            );
        }
    }
    let h = c.title.lines().count().max(1) as f32 * look.size * 1.3;
    label_texts(
        scene,
        &c.title,
        Point::new(r.center().x, icon.max_y() + 6.0 + h / 2.0),
        look.size,
        look.text,
        true,
    );
}

// ---- block ----

const BLOCK_GAP: f32 = 10.0;
const BLOCK_PAD: f32 = 10.0;

pub fn block(d: &BlockDiagram, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    // Each block's own size: a leaf's around its label, a nested one's around its grid.
    let mut sizes = vec![(0.0f32, 0.0f32); d.blocks.len()];
    fn measure_block(
        d: &BlockDiagram,
        i: usize,
        look: &Look,
        m: &mut dyn Measure,
        sizes: &mut [(f32, f32)],
    ) -> (f32, f32) {
        let b = &d.blocks[i];
        let s = if b.space {
            (40.0, 38.0)
        } else if b.children.is_empty() {
            node_size(b.shape, text_size(m, &b.label, look.size, false))
        } else {
            for &c in &b.children {
                measure_block(d, c, look, m, sizes);
            }
            let (w, h) = grid_size(d, &b.children, b.columns, sizes);
            (w + 2.0 * BLOCK_PAD, h + 2.0 * BLOCK_PAD + title_room(b))
        };
        sizes[i] = s;
        s
    }
    for &t in &d.top {
        measure_block(d, t, look, measure, &mut sizes);
    }
    let (w, h) = grid_size(d, &d.top, d.columns, &sizes);
    let mut rects = vec![Rect::default(); d.blocks.len()];
    place(d, &d.top, d.columns, Rect::new(0.0, 0.0, w, h), &sizes, &mut rects);
    let mut scene = Scene::default();
    // Frames first (the outer ones under), then leaves, then arrows over.
    let mut order: Vec<(usize, usize)> = Vec::new();
    fn walk(d: &BlockDiagram, items: &[usize], depth: usize, out: &mut Vec<(usize, usize)>) {
        for &i in items {
            out.push((i, depth));
            walk(d, &d.blocks[i].children, depth + 1, out);
        }
    }
    walk(d, &d.top, 0, &mut order);
    for &(i, _) in &order {
        let b = &d.blocks[i];
        let r = rects[i];
        if b.space {
            continue;
        }
        if b.children.is_empty() {
            // Round shapes stay round in a stretched cell.
            let r = if matches!(b.shape, Shape::Circle | Shape::DoubleCircle) {
                let d = r.size.width.min(r.size.height);
                Rect::new(r.center().x - d / 2.0, r.center().y - d / 2.0, d, d)
            } else {
                r
            };
            draw_node(&mut scene, b.shape, r, &Style::default(), look);
            label_texts(&mut scene, &b.label, r.center(), size, look.text, false);
        } else {
            scene.rect(r, 6.0, Some(look.group), Some(Line::solid(look.group_stroke, 1.0)));
            if title_room(b) > 0.0 {
                let at = Point::new(r.center().x, r.min_y() + BLOCK_PAD + size * 0.6);
                label_texts(&mut scene, &b.label, at, size * 0.92, look.muted, true);
            }
        }
    }
    for (a, b, label) in &d.edges {
        let (ra, rb) = (rects[*a], rects[*b]);
        let (from, to) = (clip(ra, rb.center()), clip(rb, ra.center()));
        scene.polyline(&[from, to], Line::solid(look.line, 1.5));
        scene::head(&mut scene, from, to, HeadKind::Arrow, 9.0, look.line, look.background);
        if let Some(l) = label.as_ref().filter(|l| !l.is_empty()) {
            let mid = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
            let (lw, lh) = text_size(measure, l, size * 0.9, false);
            scene.rect(
                Rect::new(mid.x - lw / 2.0 - 4.0, mid.y - lh / 2.0 - 1.0, lw + 8.0, lh + 2.0),
                3.0,
                Some(look.background),
                None,
            );
            label_texts(&mut scene, l, mid, size * 0.9, look.text, false);
        }
    }
    scene.width = w;
    scene.height = h;
    scene
}

/// A nested block shows its label over its grid when it has one of its own.
fn title_room(b: &diagram_text::mermaid::block::Block) -> f32 {
    if b.label.trim().is_empty() || b.label == b.id { 0.0 } else { 22.0 }
}

/// A grid's rows: each block and the column it starts at.
type Rows = Vec<Vec<(usize, u32)>>;

/// The rows of a grid: `items` in `columns` (all on one row without), by their spans.
fn rows(d: &BlockDiagram, items: &[usize], columns: Option<u32>) -> (u32, Vec<Vec<(usize, u32)>>) {
    let columns = columns
        .unwrap_or_else(|| items.iter().map(|&i| d.blocks[i].span.max(1)).sum::<u32>())
        .max(1);
    let mut rows: Vec<Vec<(usize, u32)>> = vec![Vec::new()];
    let mut used = 0;
    for &i in items {
        let span = d.blocks[i].span.clamp(1, columns);
        if used + span > columns {
            rows.push(Vec::new());
            used = 0;
        }
        if let Some(r) = rows.last_mut() {
            r.push((i, used));
        }
        used += span;
    }
    (columns, rows)
}

/// A grid's cell width (all the same) and rows' heights.
fn cells(
    d: &BlockDiagram,
    items: &[usize],
    columns: Option<u32>,
    sizes: &[(f32, f32)],
) -> (u32, f32, Vec<f32>, Rows) {
    let (columns, rows) = rows(d, items, columns);
    let cell = items
        .iter()
        .map(|&i| {
            let span = d.blocks[i].span.clamp(1, columns) as f32;
            (sizes[i].0 - BLOCK_GAP * (span - 1.0)) / span
        })
        .fold(0.0, f32::max);
    let heights =
        rows.iter().map(|r| r.iter().map(|(i, _)| sizes[*i].1).fold(0.0, f32::max)).collect();
    (columns, cell, heights, rows)
}

fn grid_size(
    d: &BlockDiagram,
    items: &[usize],
    columns: Option<u32>,
    sizes: &[(f32, f32)],
) -> (f32, f32) {
    let (columns, cell, heights, _) = cells(d, items, columns, sizes);
    let w = cell * columns as f32 + BLOCK_GAP * (columns as f32 - 1.0);
    let h = heights.iter().sum::<f32>() + BLOCK_GAP * (heights.len() as f32 - 1.0).max(0.0);
    (w, h)
}

/// Places `items` in `area` (its width shared by the columns), nested blocks inside.
fn place(
    d: &BlockDiagram,
    items: &[usize],
    columns: Option<u32>,
    area: Rect,
    sizes: &[(f32, f32)],
    rects: &mut [Rect],
) {
    let (columns, _, heights, rows) = cells(d, items, columns, sizes);
    let cell = (area.size.width - BLOCK_GAP * (columns as f32 - 1.0)) / columns as f32;
    let mut y = area.min_y();
    for (row, h) in rows.iter().zip(&heights) {
        for &(i, at) in row {
            let span = d.blocks[i].span.clamp(1, columns) as f32;
            let r = Rect::new(
                area.min_x() + at as f32 * (cell + BLOCK_GAP),
                y,
                cell * span + BLOCK_GAP * (span - 1.0),
                *h,
            );
            rects[i] = r;
            let b = &d.blocks[i];
            if !b.children.is_empty() {
                let top = BLOCK_PAD + title_room(b);
                let inner = Rect::new(
                    r.min_x() + BLOCK_PAD,
                    r.min_y() + top,
                    r.size.width - 2.0 * BLOCK_PAD,
                    r.size.height - top - BLOCK_PAD,
                );
                place(d, &b.children, b.columns, inner, sizes, rects);
            }
        }
        y += h + BLOCK_GAP;
    }
}

/// Where the line from `r`'s center toward `to` leaves `r`.
fn clip(r: Rect, to: Point) -> Point {
    let c = r.center();
    let (dx, dy) = (to.x - c.x, to.y - c.y);
    if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
        return c;
    }
    let (hw, hh) = (r.size.width / 2.0, r.size.height / 2.0);
    let t = (hw / dx.abs().max(f32::EPSILON)).min(hh / dy.abs().max(f32::EPSILON));
    Point::new(c.x + dx * t, c.y + dy * t)
}

// ---- kanban ----

pub fn kanban(k: &Kanban, look: &Look, measure: &mut dyn Measure) -> Scene {
    let size = look.size;
    let small = size * 0.8;
    let mut scene = Scene::default();
    let col_w = 220.0f32;
    let gap = 14.0;
    let header = size * 2.2;
    let mut bottom: f32 = header;
    for (c, &root) in k.roots.iter().enumerate() {
        let x = c as f32 * (col_w + gap);
        let column = &k.nodes[root];
        // Its cards: everything under it, in order.
        let mut cards = Vec::new();
        let mut stack: Vec<usize> = column.children.iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            cards.push(n);
            stack.extend(k.nodes[n].children.iter().rev());
        }
        let mut y = header + 6.0;
        let mut placed = Vec::new();
        for &n in &cards {
            let node = &k.nodes[n];
            let label = wrap(measure, &node.text, size, col_w - 36.0);
            let (_, th) = text_size(measure, &label, size, false);
            let info: Vec<String> = ["ticket", "assigned"]
                .iter()
                .filter_map(|key| node.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
                .collect();
            let h = th + 20.0 + if info.is_empty() { 0.0 } else { small * 1.4 };
            placed.push((n, label, info, Rect::new(x + 8.0, y, col_w - 16.0, h)));
            y += h + 8.0;
        }
        let r = Rect::new(x, 0.0, col_w, y.max(header + 30.0));
        scene.rect(r, 8.0, Some(look.group), Some(Line::solid(look.group_stroke, 1.0)));
        label_texts(
            &mut scene,
            &column.text,
            Point::new(r.center().x, header / 2.0 + 2.0),
            size,
            look.text,
            true,
        );
        for (n, label, info, card) in placed {
            let node = &k.nodes[n];
            scene.rect(
                card,
                6.0,
                Some(look.fill),
                Some(Line::solid(look.stroke.with_alpha(0.6), 1.0)),
            );
            let priority = node
                .meta
                .iter()
                .find(|(k, _)| k == "priority")
                .map(|(_, v)| v.to_ascii_lowercase());
            let stripe = match priority.as_deref() {
                Some("very high") => Some(look.series[5]),
                Some("high") => Some(look.series[3]),
                Some("low") => Some(look.series[0]),
                Some("very low") => Some(look.series[6]),
                _ => None,
            };
            if let Some(color) = stripe {
                scene.rect(
                    Rect::new(card.min_x(), card.min_y(), 4.0, card.size.height),
                    2.0,
                    Some(color),
                    None,
                );
            }
            let mut ty = card.min_y() + 10.0;
            for line in label.lines() {
                scene.text(text(
                    Point::new(card.min_x() + 12.0, ty + size * 0.65),
                    line,
                    size,
                    look.text,
                    Anchor::Start,
                    false,
                ));
                ty += size * 1.3;
            }
            if !info.is_empty() {
                let at = Point::new(card.min_x() + 12.0, card.max_y() - small * 1.1);
                scene.text(text(at, &info.join(" · "), small, look.muted, Anchor::Start, false));
            }
        }
        bottom = bottom.max(r.max_y());
    }
    scene.width = (k.roots.len() as f32 * (col_w + gap) - gap).max(col_w);
    scene.height = bottom;
    scene
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/cards.rs"]
mod tests;
