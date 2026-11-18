//! A git graph: a lane per branch (in their `order`), the commits along the time in the
//! order they're written, each branch's line from where it forks, merges and
//! cherry-picks as curves into their commit; commits by kind, ids and tags as labels.

use diagram_text::mermaid::GitGraph;
use diagram_text::mermaid::git::Kind;
use kernel::{Point, Rect};

use super::scene::{self, Anchor, Line, Scene, Seg, Text};
use super::{Look, Measure, text_size};

const STEP: f32 = 56.0;
const LANE: f32 = 52.0;
const R: f32 = 9.0;

pub fn layout(g: &GitGraph, look: &Look, measure: &mut dyn Measure) -> Scene {
    let mut scene = Scene::default();
    let size = look.size;
    let small = size * 0.82;
    let vertical = !g.direction.horizontal();
    // Lanes: by `order`, then as declared.
    let mut lanes: Vec<usize> = (0..g.branches.len()).collect();
    lanes.sort_by_key(|&b| (g.branches[b].order.unwrap_or(b as i32), b));
    let mut lane_of = vec![0usize; g.branches.len()];
    for (lane, &b) in lanes.iter().enumerate() {
        lane_of[b] = lane;
    }
    let names_w =
        g.branches.iter().map(|b| text_size(measure, &b.name, small, true).0).fold(0.0, f32::max)
            + 24.0;
    let start = names_w + 20.0;
    let at = |commit: usize| -> Point {
        let along = start + commit as f32 * STEP;
        let across = 20.0 + lane_of[g.commits[commit].branch] as f32 * LANE;
        if vertical {
            Point::new(across + names_w, along - names_w + 40.0)
        } else {
            Point::new(along, across)
        }
    };
    let color = |b: usize| look.series[lane_of[b] % look.series.len()];
    // Each branch's line: from its first commit's parent (where it forked) to its last.
    for (b, branch) in g.branches.iter().enumerate() {
        let own: Vec<usize> = (0..g.commits.len()).filter(|&c| g.commits[c].branch == b).collect();
        let lane_point = |along: f32| -> Point {
            let across = 20.0 + lane_of[b] as f32 * LANE;
            if vertical {
                Point::new(across + names_w, along - names_w + 40.0)
            } else {
                Point::new(along, across)
            }
        };
        let label_at = lane_point(start - 24.0 - if vertical { 0.0 } else { names_w / 2.0 - 8.0 });
        let w = text_size(measure, &branch.name, small, true).0 + 16.0;
        let pill = Rect::new(label_at.x - w / 2.0, label_at.y - 11.0, w, 22.0);
        scene.rect(pill, 11.0, Some(color(b).mix(look.background, 0.25)), None);
        scene.text(Text {
            at: pill.center(),
            text: branch.name.clone(),
            size: small,
            bold: true,
            italic: false,
            mono: false,
            math: false,
            color: look.background,
            anchor: Anchor::Middle,
        });
        let (Some(&first), Some(&last)) = (own.first(), own.last()) else { continue };
        let line = Line::solid(color(b), 2.5);
        let from = g.commits[first].parents.first().copied().filter(|&p| g.commits[p].branch != b);
        if let Some(p) = from {
            let (a, c) = (at(p), at(first));
            scene.path(curve(a, c, vertical), None, Some(line));
        }
        scene.polyline(&[at(first), at(last)], line);
    }
    // Merges and cherry-picks: from the other parent into the commit.
    for (i, c) in g.commits.iter().enumerate() {
        for &p in c.parents.iter().skip(1) {
            let line = if c.kind == Kind::CherryPick {
                Line::dashed(color(g.commits[p].branch), 2.0)
            } else {
                Line::solid(color(g.commits[p].branch), 2.5)
            };
            scene.path(curve(at(p), at(i), vertical), None, Some(line));
        }
    }
    let mut max = Point::new(0.0, 0.0);
    for (i, c) in g.commits.iter().enumerate() {
        let p = at(i);
        let tone = color(c.branch);
        let r = Rect::new(p.x - R, p.y - R, 2.0 * R, 2.0 * R);
        match c.kind {
            Kind::Normal => {
                scene.path(scene::ellipse(r), Some(tone), Some(Line::solid(look.background, 2.0)))
            }
            Kind::Highlight => scene.rect(r, 2.0, Some(tone), Some(Line::solid(look.text, 1.5))),
            Kind::Reverse => {
                scene.path(scene::ellipse(r), Some(tone), None);
                let k = R * 0.5;
                let l = Line::solid(look.background, 2.0);
                scene.path(
                    vec![
                        Seg::Move(Point::new(p.x - k, p.y - k)),
                        Seg::Line(Point::new(p.x + k, p.y + k)),
                    ],
                    None,
                    Some(l),
                );
                scene.path(
                    vec![
                        Seg::Move(Point::new(p.x - k, p.y + k)),
                        Seg::Line(Point::new(p.x + k, p.y - k)),
                    ],
                    None,
                    Some(l),
                );
            }
            Kind::Merge => {
                scene.path(scene::ellipse(r), Some(look.background), Some(Line::solid(tone, 3.0)));
                scene.path(scene::ellipse(super::grow(r, -4.5)), Some(tone), None);
            }
            Kind::CherryPick => {
                scene.path(scene::ellipse(r), Some(tone), None);
                scene.path(scene::ellipse(super::grow(r, -5.0)), Some(look.background), None);
            }
        }
        let below = if vertical {
            Point::new(p.x + R + 6.0, p.y)
        } else {
            Point::new(p.x, p.y + R + small)
        };
        if c.named {
            let anchor = if vertical { Anchor::Start } else { Anchor::Middle };
            scene.text(Text {
                at: below,
                text: c.id.clone(),
                size: small,
                bold: false,
                italic: false,
                mono: false,
                math: false,
                color: look.muted,
                anchor,
            });
            let w = text_size(measure, &c.id, small, false).0;
            max.x = max.x.max(below.x + if vertical { w } else { w / 2.0 });
        }
        if let Some(tag) = &c.tag {
            let w = text_size(measure, tag, small, false).0 + 12.0;
            let c0 = if vertical {
                Point::new(p.x - R - 8.0 - w / 2.0, p.y)
            } else {
                Point::new(p.x, p.y - R - 16.0)
            };
            let tr = Rect::new(c0.x - w / 2.0, c0.y - 10.0, w, 20.0);
            scene.rect(tr, 4.0, Some(look.note), Some(Line::solid(look.note_stroke, 1.0)));
            scene.text(Text {
                at: tr.center(),
                text: tag.clone(),
                size: small,
                bold: false,
                italic: false,
                mono: false,
                math: false,
                color: look.text,
                anchor: Anchor::Middle,
            });
        }
        max.x = max.x.max(p.x + R + 10.0);
        max.y = max.y.max(p.y + R + small * 1.8);
    }
    // Tags above the first lane: everything moved down for them.
    scene.shift(0.0, 24.0);
    scene.width = max.x.max(names_w + 40.0) + 8.0;
    scene.height = max.y + 28.0;
    scene
}

/// From `a` to `b` turning the corner smoothly (into the other lane).
fn curve(a: Point, b: Point, vertical: bool) -> Vec<Seg> {
    if vertical {
        let c1 = Point::new(a.x, b.y - (b.y - a.y) * 0.5);
        let c2 = Point::new(b.x, b.y - (b.y - a.y) * 0.5);
        vec![Seg::Move(a), Seg::Cubic(c1, c2, b)]
    } else {
        let c1 = Point::new(b.x - (b.x - a.x) * 0.5, a.y);
        let c2 = Point::new(b.x - (b.x - a.x) * 0.5, b.y);
        vec![Seg::Move(a), Seg::Cubic(c1, c2, b)]
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/git.rs"]
mod tests;
