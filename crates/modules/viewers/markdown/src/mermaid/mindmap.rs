//! A mindmap: the root in the middle, its branches split to its right and its left, each
//! side a tidy tree ([`ui_components::diagram::tidy`]), every branch in a color of its
//! own, the lines thinner as they go out, each node in its shape.

use diagram_text::mermaid::Mindmap;
use diagram_text::mermaid::mindmap::Shape;
use kernel::{Point, Rect, Rgba};
use ui_components::diagram::{Flow, Gaps, tidy};

use super::scene::{self, Line, Scene, Seg};
use super::{Look, Measure, label_texts, text_size};

pub fn layout(m: &Mindmap, look: &Look, measure: &mut dyn Measure) -> Scene {
    let mut scene = Scene::default();
    if m.nodes.is_empty() {
        return scene;
    }
    let size = look.size;
    // Long texts wrapped, as Mermaid does.
    let texts: Vec<String> =
        m.nodes.iter().map(|n| super::charts::wrap(measure, &n.text, size, 200.0)).collect();
    let sizes: Vec<(f32, f32)> = m
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = if n.depth == 0 { size * 1.15 } else { size };
            let (w, h) = text_size(measure, &texts[i], s, n.depth == 0);
            match n.shape {
                Shape::Circle => {
                    let d = w.max(h) + 28.0;
                    (d, d)
                }
                Shape::Cloud | Shape::Bang => (w + 44.0, h + 30.0),
                Shape::Hexagon => (w + 40.0, h + 18.0),
                _ => (w + 26.0, h + 14.0),
            }
        })
        .collect();
    let root = 0;
    let first = &m.nodes[root].children;
    // Radial (Mermaid's look) unless it takes much more room than the two sides do (a wide
    // map: long branches on a few levels).
    let (sides, on_left) = two_sides(m, &sizes);
    let around = radial(m, &sizes);
    let area = |rects: &[Rect]| {
        let (w, h) = extent(rects);
        w * h
    };
    let radial_look = area(&around) <= area(&sides) * 1.6;
    let mut rects = if radial_look { around } else { sides };
    // From (0, 0).
    let (min_x, min_y) = rects
        .iter()
        .filter(|r| r.size.width > 0.0)
        .fold((f32::INFINITY, f32::INFINITY), |(x, y), r| (x.min(r.min_x()), y.min(r.min_y())));
    for r in &mut rects {
        r.origin.x -= min_x;
        r.origin.y -= min_y;
    }
    let (w, h) = extent(&rects);
    scene.width = w;
    scene.height = h;
    // Each node's branch color.
    let mut branch = vec![0usize; m.nodes.len()];
    for (b, &top) in first.iter().enumerate() {
        mark(m, top, b, &mut branch);
    }
    let tone = |i: usize| -> Rgba { look.series[branch[i] % look.series.len()] };
    // Lines first.
    for (i, n) in m.nodes.iter().enumerate() {
        let Some(p) = n.parent else { continue };
        let (pr, cr) = (rects[p], rects[i]);
        let width = (4.0 - n.depth as f32 * 0.9).max(1.3);
        let line = Line::solid(tone(i).mix(look.background, 0.2), width);
        if radial_look {
            // Out from the parent's middle, bending toward the child's.
            let (a, b) = (pr.center(), cr.center());
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let c1 = Point::new(a.x + dx * 0.5, a.y + dy * 0.15);
            let c2 = Point::new(a.x + dx * 0.85, a.y + dy * 0.75);
            scene.path(vec![Seg::Move(a), Seg::Cubic(c1, c2, b)], None, Some(line));
            continue;
        }
        let leftward = on_left[i];
        let a = if leftward {
            Point::new(
                pr.min_x() + if p == root { pr.size.width / 2.0 } else { 0.0 },
                pr.center().y,
            )
        } else {
            Point::new(
                pr.max_x() - if p == root { pr.size.width / 2.0 } else { 0.0 },
                pr.center().y,
            )
        };
        let b = if leftward {
            Point::new(cr.max_x(), cr.center().y)
        } else {
            Point::new(cr.min_x(), cr.center().y)
        };
        scene.path(scene::flowing(&[a, b], false), None, Some(line));
    }
    for (i, n) in m.nodes.iter().enumerate() {
        let r = rects[i];
        if r.size.width <= 0.0 {
            continue;
        }
        let (fill, text) = if i == root {
            (look.accent, look.background)
        } else if n.depth == 1 {
            (tone(i).mix(look.background, 0.2), look.background)
        } else {
            (tone(i).mix(look.background, 0.78), look.text)
        };
        let line = Some(Line::solid(tone(i).mix(look.background, 0.1), 1.2));
        match n.shape {
            Shape::Circle => scene.path(scene::ellipse(r), Some(fill), None),
            Shape::Square => scene.rect(r, 2.0, Some(fill), line),
            Shape::Rounded => scene.rect(r, 10.0, Some(fill), line),
            Shape::Hexagon => {
                let (x0, x1, y0, y1, c) = (r.min_x(), r.max_x(), r.min_y(), r.max_y(), r.center());
                let s = r.size.height * 0.35;
                let p = Point::new;
                scene.path(
                    scene::polygon(&[
                        p(x0 + s, y0),
                        p(x1 - s, y0),
                        p(x1, c.y),
                        p(x1 - s, y1),
                        p(x0 + s, y1),
                        p(x0, c.y),
                    ]),
                    Some(fill),
                    line,
                );
            }
            Shape::Cloud => cloud(&mut scene, r, fill),
            Shape::Bang => bang(&mut scene, r, fill),
            Shape::Plain => {
                if i == root {
                    scene.rect(r, r.size.height / 2.0, Some(fill), None);
                } else {
                    scene.rect(r, 6.0, Some(fill), None);
                }
            }
        }
        let s = if n.depth == 0 { size * 1.15 } else { size };
        label_texts(&mut scene, &texts[i], r.center(), s, text, n.depth <= 1);
    }
    scene
}

/// The width and height `rects` span.
fn extent(rects: &[Rect]) -> (f32, f32) {
    let shown = rects.iter().filter(|r| r.size.width > 0.0);
    let (x0, y0, x1, y1) = shown.fold(
        (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
        |(a, b, c, d), r| (a.min(r.min_x()), b.min(r.min_y()), c.max(r.max_x()), d.max(r.max_y())),
    );
    if x0.is_finite() { (x1 - x0, y1 - y0) } else { (0.0, 0.0) }
}

/// The root in the middle, its branches' first half to its right and the rest to its
/// left, each side a tidy tree; which nodes went left.
fn two_sides(m: &Mindmap, sizes: &[(f32, f32)]) -> (Vec<Rect>, Vec<bool>) {
    let root = 0;
    let first = &m.nodes[root].children;
    let half = first.len().div_ceil(2);
    let gaps = Gaps { sibling: 12.0, subtree: 18.0, level: 46.0 };
    let side = |branches: &[usize]| -> Vec<Rect> {
        let mut children: Vec<Vec<usize>> = m.nodes.iter().map(|n| n.children.clone()).collect();
        children[root] = branches.to_vec();
        tidy(&children, &[root], sizes, Flow::Right, gaps)
    };
    let right = side(&first[..half]);
    let left = side(&first[half..]);
    // The left side mirrored, its root put on the right side's.
    let r0 = right[root];
    let l0 = left[root];
    let mut rects = right;
    let mut on_left = vec![false; m.nodes.len()];
    for (i, r) in left.iter().enumerate() {
        if i == root || r.size.width <= 0.0 || !in_branch(m, i, &first[half..]) {
            continue;
        }
        let x = r0.min_x() + r0.size.width - (r.max_x() - l0.min_x());
        let y = r.min_y() - l0.min_y() + r0.min_y();
        rects[i] = Rect::new(x - r0.size.width, y, r.size.width, r.size.height);
        on_left[i] = true;
    }
    (rects, on_left)
}

/// The root in the middle, every level a ring around it, each branch a sector as wide
/// as its leaves are many; a ring as far out as its nodes need to sit side by side.
fn radial(m: &Mindmap, sizes: &[(f32, f32)]) -> Vec<Rect> {
    let n = m.nodes.len();
    // Each subtree's weight: its leaves.
    let mut weight = vec![1.0f32; n];
    for i in (0..n).rev() {
        let kids = &m.nodes[i].children;
        if !kids.is_empty() {
            weight[i] = kids.iter().map(|&c| weight[c]).sum();
        }
    }
    // Each node's sector (start, sweep) and its angle (the sector's middle).
    let mut sector = vec![(0.0f32, 0.0f32); n];
    sector[0] = (-std::f32::consts::FRAC_PI_2, std::f32::consts::TAU);
    for i in 0..n {
        let (start, sweep) = sector[i];
        let kids = &m.nodes[i].children;
        let total: f32 = kids.iter().map(|&c| weight[c]).sum::<f32>().max(1.0);
        let mut at = start;
        for &c in kids {
            let part = sweep * weight[c] / total;
            sector[c] = (at, part);
            at += part;
        }
    }
    let depth = m.nodes.iter().map(|n| n.depth).max().unwrap_or(0);
    // Each ring's radius: past the one inside by the boxes' reach, and far enough for each
    // box to fit its sector along the ring.
    let mut radius = vec![0.0f32; depth + 1];
    let reach = |i: usize, angle: f32| {
        let (w, h) = sizes[i];
        (w * angle.cos().abs() + h * angle.sin().abs()) / 2.0
    };
    for d in 1..=depth {
        let ring: Vec<usize> = (0..n).filter(|&i| m.nodes[i].depth == d).collect();
        let inner = (0..n)
            .filter(|&i| m.nodes[i].depth == d - 1)
            .map(|i| reach(i, sector[i].0 + sector[i].1 / 2.0))
            .fold(0.0f32, f32::max);
        let out =
            ring.iter().map(|&i| reach(i, sector[i].0 + sector[i].1 / 2.0)).fold(0.0f32, f32::max);
        let mut r = radius[d - 1] + inner + out + 40.0;
        for &i in &ring {
            let (start, sweep) = sector[i];
            let angle = start + sweep / 2.0;
            let (w, h) = sizes[i];
            // Across the ring the box takes this much.
            let across = w * angle.sin().abs() + h * angle.cos().abs() + 16.0;
            if sweep > 0.0 {
                r = r.max(across / sweep.min(std::f32::consts::PI));
            }
        }
        radius[d] = r;
    }
    (0..n)
        .map(|i| {
            let d = m.nodes[i].depth;
            let (start, sweep) = sector[i];
            let angle = start + sweep / 2.0;
            let r = if d == 0 { 0.0 } else { radius[d] };
            let (w, h) = sizes[i];
            Rect::new(r * angle.cos() - w / 2.0, r * angle.sin() - h / 2.0, w, h)
        })
        .collect()
}

/// `node` is in one of `branches`' subtrees.
fn in_branch(m: &Mindmap, mut node: usize, branches: &[usize]) -> bool {
    loop {
        if branches.contains(&node) {
            return true;
        }
        match m.nodes[node].parent {
            Some(p) => node = p,
            None => return false,
        }
    }
}

fn mark(m: &Mindmap, node: usize, b: usize, out: &mut [usize]) {
    out[node] = b;
    for &c in &m.nodes[node].children {
        mark(m, c, b, out);
    }
}

/// A cloud: bumps around the box.
fn cloud(scene: &mut Scene, r: Rect, fill: Rgba) {
    let c = r.center();
    let (rx, ry) = (r.size.width / 2.0, r.size.height / 2.0);
    let n = 9;
    let mut segs = Vec::new();
    let at = |a: f32, k: f32| Point::new(c.x + rx * k * a.cos(), c.y + ry * k * a.sin());
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let p = at(a, 0.86);
        if i == 0 {
            segs.push(Seg::Move(p));
            continue;
        }
        let mid = a - std::f32::consts::TAU / n as f32 / 2.0;
        let bump = at(mid, 1.2);
        segs.push(Seg::Cubic(bump, bump, p));
    }
    segs.push(Seg::Close);
    scene.path(segs, Some(fill), None);
}

/// A bang: a jagged star around the box.
fn bang(scene: &mut Scene, r: Rect, fill: Rgba) {
    let c = r.center();
    let (rx, ry) = (r.size.width / 2.0, r.size.height / 2.0);
    let n = 14;
    let points: Vec<Point> = (0..n * 2)
        .map(|i| {
            let a = i as f32 / (n * 2) as f32 * std::f32::consts::TAU;
            let k = if i % 2 == 0 { 1.12 } else { 0.84 };
            Point::new(c.x + rx * k * a.cos(), c.y + ry * k * a.sin())
        })
        .collect();
    scene.path(scene::polygon(&points), Some(fill), None);
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/mindmap.rs"]
mod tests;
