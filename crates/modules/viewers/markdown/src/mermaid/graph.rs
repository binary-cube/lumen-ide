//! The diagrams that are boxes joined by lines (flowcharts, state machines, classes,
//! entities): each group laid out on its own first (in its own direction), then as one
//! box in its parent's layout — so its members stay together — the levels by the layered
//! layout ([`ui_components::diagram::layered`]); lines as smooth curves leaving and
//! entering along the flow, clipped at each box's outline, with their heads, their labels
//! and the labels at their ends (cardinalities). What each box looks like is the type's:
//! it draws them once they're placed.

use std::collections::HashMap;

use diagram_text::mermaid::{Direction, Style};
use kernel::{Point, Rect, Rgba};
use ui_components::diagram::layered::{self, EdgeSpec, Options};

use super::scene::{self, HeadKind, Line, Scene};
use super::{Look, Measure, label_texts, text_size};

/// What a line ends at: a box, or a whole group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum End {
    Node(usize),
    Group(usize),
}

/// A box's outline (where lines meet it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outline {
    Rect,
    Ellipse,
    Diamond,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub size: (f32, f32),
    pub outline: Outline,
    /// The group it's in.
    pub group: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: End,
    pub to: End,
    pub label: Option<String>,
    /// The labels at its ends (a relation's cardinalities).
    pub from_label: Option<String>,
    pub to_label: Option<String>,
    /// How it's drawn (`None`: not drawn, but laid out).
    pub line: Option<Line>,
    pub head: Option<HeadKind>,
    pub tail: Option<HeadKind>,
    pub length: usize,
    pub label_color: Option<Rgba>,
}

#[derive(Debug, Clone, Default)]
pub struct Group {
    pub title: String,
    pub parent: Option<usize>,
    pub direction: Option<Direction>,
    pub style: Style,
}

#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
}

/// A box of a level: a node, or a group laid out inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Member {
    Node(usize),
    Group(usize),
}

/// Room inside a group's frame.
const GROUP_PAD: f32 = 16.0;

/// A level laid out: its size, its members' boxes (from its corner), and the edges laid
/// out at this level (their index, points and label box, from the corner).
struct Placed {
    width: f32,
    height: f32,
    boxes: HashMap<Member, Rect>,
    edges: Vec<(usize, Vec<Point>, Option<Rect>)>,
}

struct Ctx<'a> {
    g: &'a Graph,
    titles: Vec<f32>,
    labels: Vec<Option<(f32, f32)>>,
    placed: HashMap<Option<usize>, Placed>,
}

/// Lays `graph` out and draws its groups and lines into `scene` (sized to it); the boxes
/// are left to `draw`, called for each node with its place, over the lines.
pub fn draw(
    graph: &Graph,
    look: &Look,
    measure: &mut dyn Measure,
    scene: &mut Scene,
    mut draw: impl FnMut(&mut Scene, usize, Rect),
) {
    let size = look.size;
    let titles = graph
        .groups
        .iter()
        .map(|g| {
            if g.title.trim().is_empty() {
                0.0
            } else {
                text_size(measure, &g.title, size, false).1 + 6.0
            }
        })
        .collect();
    let labels = graph
        .edges
        .iter()
        .map(|e| {
            e.label.as_ref().filter(|l| !l.trim().is_empty()).map(|l| {
                let (w, h) = text_size(measure, l, size * 0.92, false);
                (w + 10.0, h + 4.0)
            })
        })
        .collect();
    let mut ctx = Ctx { g: graph, titles, labels, placed: HashMap::new() };
    ctx.place(None, graph.direction);
    let mut nodes = vec![Rect::default(); graph.nodes.len()];
    let mut groups = vec![Rect::default(); graph.groups.len()];
    let mut edges: Vec<Option<(Vec<Point>, Option<Rect>)>> = vec![None; graph.edges.len()];
    ctx.absolute(None, Point::new(0.0, 0.0), &mut nodes, &mut groups, &mut edges);
    let root = &ctx.placed[&None];
    scene.width = root.width;
    scene.height = root.height;

    // Groups (outer first), then lines, then boxes over them, then the lines' labels.
    let mut order: Vec<usize> = (0..graph.groups.len()).collect();
    order.sort_by_key(|&g| depth(graph, g));
    for g in order {
        let group = &graph.groups[g];
        let r = groups[g];
        let fill = group.style.fill.unwrap_or(look.group);
        let stroke = group.style.stroke.unwrap_or(look.group_stroke);
        let width = group.style.stroke_width.unwrap_or(1.0);
        let line = if group.style.dashed {
            Line::dashed(stroke, width)
        } else {
            Line::solid(stroke, width)
        };
        scene.rect(r, 6.0, Some(fill), Some(line));
        if !group.title.trim().is_empty() {
            let color = group.style.color.unwrap_or(look.muted);
            let at = Point::new(r.center().x, r.min_y() + GROUP_PAD * 0.5 + ctx.titles[g] / 2.0);
            label_texts(scene, &group.title, at, size, color, false);
        }
    }
    let vertical = !graph.direction.horizontal();
    let mut texts = Vec::new();
    for (i, edge) in graph.edges.iter().enumerate() {
        let Some((points, label)) = &edges[i] else { continue };
        let from = ctx.end_shape(edge.from, &nodes, &groups);
        let to = ctx.end_shape(edge.to, &nodes, &groups);
        let ends = draw_edge(scene, edge, points, from, to, vertical, look);
        if let (Some(text), Some(r)) = (&edge.label, label) {
            texts.push((text.clone(), *r, edge.label_color.unwrap_or(look.muted), true));
        }
        // The ends' labels beside the line, just off the box.
        for (text, at) in [(&edge.from_label, ends.0), (&edge.to_label, ends.1)] {
            let (Some(text), Some((tip, dir))) = (text, at) else { continue };
            let (w, h) = text_size(measure, text, size * 0.85, false);
            let side = Point::new(-dir.1, dir.0);
            let along = dir.0.abs() * (w / 2.0 + 10.0) + dir.1.abs() * (h / 2.0 + 10.0);
            let beside = side.x.abs() * (w / 2.0 + 6.0) + side.y.abs() * (h / 2.0 + 6.0);
            let c = Point::new(
                tip.x + dir.0 * along + side.x * beside,
                tip.y + dir.1 * along + side.y * beside,
            );
            texts.push((
                text.clone(),
                Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h),
                look.muted,
                false,
            ));
        }
    }
    for (n, r) in nodes.iter().enumerate() {
        draw(scene, n, *r);
    }
    for (text, r, color, boxed) in texts {
        if boxed {
            scene.rect(r, 3.0, Some(look.background.with_alpha(0.92)), None);
        }
        let s = if boxed { size * 0.92 } else { size * 0.85 };
        label_texts(scene, &text, r.center(), s, color, false);
    }
}

fn depth(g: &Graph, mut i: usize) -> usize {
    let mut d = 0;
    while let Some(p) = g.groups[i].parent {
        d += 1;
        i = p;
    }
    d
}

pub fn to_layered(d: Direction) -> layered::Direction {
    match d {
        Direction::Down => layered::Direction::Down,
        Direction::Up => layered::Direction::Up,
        Direction::Right => layered::Direction::Right,
        Direction::Left => layered::Direction::Left,
    }
}

impl Ctx<'_> {
    fn members(&self, level: Option<usize>) -> Vec<Member> {
        let nodes =
            (0..self.g.nodes.len()).filter(|&n| self.g.nodes[n].group == level).map(Member::Node);
        let groups = (0..self.g.groups.len())
            .filter(|&g| self.g.groups[g].parent == level)
            .map(Member::Group);
        nodes.chain(groups).collect()
    }

    /// What stands for `end` at `level`: itself, or the group of the level holding it.
    fn representative(&self, end: End, level: Option<usize>) -> Option<Member> {
        let (mut member, mut parent) = match end {
            End::Node(n) => (Member::Node(n), self.g.nodes[n].group),
            End::Group(g) => (Member::Group(g), self.g.groups[g].parent),
        };
        loop {
            if parent == level {
                return Some(member);
            }
            let g = parent?;
            member = Member::Group(g);
            parent = self.g.groups[g].parent;
        }
    }

    fn place(&mut self, level: Option<usize>, direction: Direction) {
        let members = self.members(level);
        for m in &members {
            if let Member::Group(g) = *m {
                let d = self.g.groups[g].direction.unwrap_or(direction);
                self.place(Some(g), d);
            }
        }
        let sizes: Vec<(f32, f32)> = members
            .iter()
            .map(|m| match *m {
                Member::Node(n) => self.g.nodes[n].size,
                Member::Group(g) => {
                    let p = &self.placed[&Some(g)];
                    (p.width + 2.0 * GROUP_PAD, p.height + 2.0 * GROUP_PAD + self.titles[g])
                }
            })
            .collect();
        let index: HashMap<Member, usize> =
            members.iter().enumerate().map(|(i, m)| (*m, i)).collect();
        let mut specs = Vec::new();
        let mut which = Vec::new();
        for (i, e) in self.g.edges.iter().enumerate() {
            let (Some(a), Some(b)) =
                (self.representative(e.from, level), self.representative(e.to, level))
            else {
                continue;
            };
            if a == b && !(matches!(a, Member::Node(_)) && e.from == e.to) {
                continue;
            }
            specs.push(EdgeSpec {
                from: index[&a],
                to: index[&b],
                min_len: e.length,
                label: self.labels[i],
            });
            which.push(i);
        }
        let options = Options { direction: to_layered(direction), node_gap: 36.0, rank_gap: 48.0 };
        let laid = layered::layout(&sizes, &specs, options);
        let boxes = members.iter().enumerate().map(|(i, m)| (*m, laid.nodes[i])).collect();
        let edges =
            which.into_iter().zip(laid.edges).map(|(i, e)| (i, e.points, e.label)).collect();
        self.placed.insert(level, Placed { width: laid.width, height: laid.height, boxes, edges });
    }

    fn absolute(
        &self,
        level: Option<usize>,
        origin: Point,
        nodes: &mut [Rect],
        groups: &mut [Rect],
        edges: &mut [Option<(Vec<Point>, Option<Rect>)>],
    ) {
        let placed = &self.placed[&level];
        let at = |r: Rect| {
            Rect::new(r.min_x() + origin.x, r.min_y() + origin.y, r.size.width, r.size.height)
        };
        for (m, r) in &placed.boxes {
            let r = at(*r);
            match *m {
                Member::Node(n) => nodes[n] = r,
                Member::Group(g) => {
                    groups[g] = r;
                    let inner =
                        Point::new(r.min_x() + GROUP_PAD, r.min_y() + GROUP_PAD + self.titles[g]);
                    self.absolute(Some(g), inner, nodes, groups, edges);
                }
            }
        }
        for (i, points, label) in &placed.edges {
            let points =
                points.iter().map(|p| Point::new(p.x + origin.x, p.y + origin.y)).collect();
            edges[*i] = Some((points, label.map(at)));
        }
    }

    fn end_shape(&self, end: End, nodes: &[Rect], groups: &[Rect]) -> (Outline, Rect) {
        match end {
            End::Node(n) => (self.g.nodes[n].outline, nodes[n]),
            End::Group(g) => (Outline::Rect, groups[g]),
        }
    }
}

/// Where the line from `r`'s center toward `toward` leaves the outline.
fn boundary(outline: Outline, r: Rect, toward: Point) -> Point {
    let c = r.center();
    let (dx, dy) = (toward.x - c.x, toward.y - c.y);
    if dx.abs() < 0.001 && dy.abs() < 0.001 {
        return c;
    }
    let (hw, hh) = (r.size.width / 2.0, r.size.height / 2.0);
    let t = match outline {
        Outline::Ellipse => 1.0 / ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt(),
        Outline::Diamond => 1.0 / (dx.abs() / hw + dy.abs() / hh),
        Outline::Rect => {
            let tx = if dx.abs() > 0.001 { hw / dx.abs() } else { f32::INFINITY };
            let ty = if dy.abs() > 0.001 { hh / dy.abs() } else { f32::INFINITY };
            tx.min(ty)
        }
    };
    Point::new(c.x + dx * t, c.y + dy * t)
}

/// A line drawn (its heads too); its two ends on the boxes and the way each leaves its box
/// (for the ends' labels).
type Ends = (Option<(Point, (f32, f32))>, Option<(Point, (f32, f32))>);

fn draw_edge(
    scene: &mut Scene,
    edge: &Edge,
    points: &[Point],
    (from_shape, from_rect): (Outline, Rect),
    (to_shape, to_rect): (Outline, Rect),
    vertical: bool,
    look: &Look,
) -> Ends {
    let Some(line) = edge.line else { return (None, None) };
    if points.len() < 2 {
        return (None, None);
    }
    let mut points = points.to_vec();
    let last = points.len() - 1;
    points[0] = boundary(from_shape, from_rect, points[1]);
    points[last] = boundary(to_shape, to_rect, points[last - 1]);
    let size = 9.0 + line.width;
    // The heads point along the flow (the curves leave and enter along it): the line stops
    // short of their tips that way.
    let (start_tip, end_tip) = (points[0], points[last]);
    // From a tip along the line (toward its other points).
    let away = |tip: Point, from: Point| -> (f32, f32) {
        if vertical {
            (0.0, if from.y < tip.y { -1.0 } else { 1.0 })
        } else {
            (if from.x < tip.x { -1.0 } else { 1.0 }, 0.0)
        }
    };
    let end_dir = away(end_tip, points[last - 1]);
    let start_dir = away(start_tip, points[1]);
    let back = |tip: Point, dir: (f32, f32), other: Point, by: f32| -> Point {
        let room = if vertical { (tip.y - other.y).abs() } else { (tip.x - other.x).abs() } * 0.5;
        let by = by.min(room);
        Point::new(tip.x + dir.0 * by, tip.y + dir.1 * by)
    };
    let head_len = |kind: Option<HeadKind>| match kind {
        None => 0.0,
        Some(k) if k.on_line() => 0.0,
        Some(HeadKind::Hollow | HeadKind::Diamond | HeadKind::HollowDiamond) => size * 1.2,
        Some(_) => size * 0.7,
    };
    if edge.head.is_some() {
        points[last] = back(end_tip, end_dir, points[last - 1], head_len(edge.head));
    }
    if edge.tail.is_some() {
        points[0] = back(start_tip, start_dir, points[1], head_len(edge.tail));
    }
    scene.path(scene::flowing(&points, vertical), None, Some(line));
    if let Some(kind) = edge.head {
        let from = Point::new(end_tip.x + end_dir.0 * size, end_tip.y + end_dir.1 * size);
        scene::head(scene, from, end_tip, kind, size, line.color, look.background);
    }
    if let Some(kind) = edge.tail {
        let from = Point::new(start_tip.x + start_dir.0 * size, start_tip.y + start_dir.1 * size);
        scene::head(scene, from, start_tip, kind, size, line.color, look.background);
    }
    (Some((start_tip, start_dir)), Some((end_tip, end_dir)))
}
