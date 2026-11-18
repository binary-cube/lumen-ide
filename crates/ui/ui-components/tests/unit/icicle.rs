//! The icicle: bars as wide as their values within their parent (their own part what their
//! children leave), children worth more scaled into it and marked, the narrow ones joined;
//! zooming into a subtree and back, a choice from outside leaving the zoom, the keys, the
//! tooltip of a merged bar, drawn within budget.

use super::*;
use render_api::Scene;

fn node(parent: Option<usize>, value: f64, self_value: f64, label: &str) -> IcicleNode {
    IcicleNode {
        parent,
        value,
        self_value,
        label: label.to_owned(),
        detail: String::new(),
        tone: Ink::heat(self_value / 100.0),
        mark: None,
        sign: None,
        tip: label.to_owned(),
    }
}

/// root 100 → a 60 (→ a1 30), b 30; root's own 10.
fn sample() -> Vec<IcicleNode> {
    vec![
        node(None, 100.0, 10.0, "root"),
        node(Some(0), 60.0, 30.0, "a"),
        node(Some(0), 30.0, 30.0, "b"),
        node(Some(1), 30.0, 30.0, "a1"),
    ]
}

fn tree(nodes: &[IcicleNode]) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut children = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        match n.parent {
            Some(p) => children[p].push(i),
            None => roots.push(i),
        }
    }
    (children, roots)
}

fn cell(cells: &[IcicleCell], node: usize) -> &IcicleCell {
    cells.iter().find(|c| c.node == Some(node)).unwrap()
}

#[test]
fn bars_are_as_wide_as_their_values_within_their_parent() {
    let nodes = sample();
    let (children, roots) = tree(&nodes);
    let cells = layout(&nodes, &children, &roots, None, 1000.0, 2.0);
    let root = cell(&cells, 0);
    assert_eq!((root.x, root.w, root.depth), (0.0, 1000.0, 0));
    let (a, b, a1) = (cell(&cells, 1), cell(&cells, 2), cell(&cells, 3));
    assert_eq!((a.x, a.w, a.depth), (0.0, 600.0, 1));
    assert_eq!((b.x, b.w), (600.0, 300.0));
    // The root's own part: what its children leave uncovered on its right.
    assert_eq!(root.w - a.w - b.w, 100.0);
    assert_eq!((a1.x, a1.w, a1.depth), (0.0, 300.0, 2));
    assert!(cells.iter().all(|c| !c.approx));
}

#[test]
fn children_worth_more_than_their_parent_are_scaled_into_it() {
    // Parallel workers' sums: 70 + 50 under a parent of 100.
    let nodes = vec![
        node(None, 100.0, 0.0, "gather"),
        node(Some(0), 70.0, 70.0, "w1"),
        node(Some(0), 50.0, 50.0, "w2"),
    ];
    let (children, roots) = tree(&nodes);
    let cells = layout(&nodes, &children, &roots, None, 1200.0, 2.0);
    let (w1, w2) = (cell(&cells, 1), cell(&cells, 2));
    assert_eq!((w1.w, w2.x, w2.w), (700.0, 700.0, 500.0));
    assert!(w1.approx && w2.approx);
    assert!(w2.x + w2.w <= 1200.0 + 1e-3, "within the parent");
}

#[test]
fn narrow_neighbours_are_joined_and_not_followed_down() {
    let mut nodes = vec![node(None, 1000.0, 0.0, "root"), node(Some(0), 900.0, 900.0, "big")];
    for i in 0..10 {
        nodes.push(node(Some(0), 1.0, 1.0, &format!("tiny{i}")));
    }
    // A child of a tiny one: never laid out.
    nodes.push(node(Some(2), 1.0, 1.0, "under-tiny"));
    let (children, roots) = tree(&nodes);
    let cells = layout(&nodes, &children, &roots, None, 500.0, 2.0);
    let merged: Vec<&IcicleCell> = cells.iter().filter(|c| c.node.is_none()).collect();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].merged.len(), 10);
    assert!((merged[0].w - 5.0).abs() < 1e-3 && (merged[0].x - 450.0).abs() < 1e-3);
    assert_eq!(merged[0].value, 10.0);
    assert!(!cells.iter().any(|c| c.node == Some(nodes.len() - 1)));
    // Wider, they're bars of their own.
    let cells = layout(&nodes, &children, &roots, None, 50_000.0, 2.0);
    assert!(cells.iter().all(|c| c.node.is_some()));
}

#[test]
fn zooming_into_a_subtree_and_back() {
    let mut icicle = Icicle::new();
    icicle.set_nodes(sample());
    assert!(icicle.zoom_to(Some(1)));
    assert_eq!(icicle.path(), vec![0, 1]);
    let cells = icicle.cells(1000.0).to_vec();
    assert_eq!(cell(&cells, 1).w, 1000.0, "the whole width");
    assert_eq!(cell(&cells, 3).w, 500.0);
    assert!(!cells.iter().any(|c| c.node == Some(2)), "outside the zoom");
    assert!(icicle.zoom_to(Some(3)));
    assert!(icicle.back());
    assert_eq!(icicle.focus(), Some(1));
    assert!(icicle.back());
    assert_eq!(icicle.focus(), None);
    assert!(!icicle.back());
    // Chosen from another view outside the zoom: back to the whole tree.
    icicle.zoom_to(Some(1));
    icicle.select(Some(2));
    assert_eq!((icicle.focus(), icicle.selected()), (None, Some(2)));
    // Inside it, the zoom stays.
    icicle.zoom_to(Some(1));
    icicle.select(Some(3));
    assert_eq!(icicle.focus(), Some(1));
}

#[test]
fn keys_walk_the_tree() {
    let mut icicle = Icicle::new();
    icicle.set_nodes(sample());
    assert!(icicle.key(ViewerKey::Down));
    assert_eq!(icicle.selected(), Some(0));
    assert!(icicle.key(ViewerKey::Down));
    assert_eq!(icicle.selected(), Some(1), "the widest child");
    assert!(icicle.key(ViewerKey::Right));
    assert_eq!(icicle.selected(), Some(2));
    assert!(icicle.key(ViewerKey::Left));
    assert!(icicle.key(ViewerKey::Up));
    assert_eq!(icicle.selected(), Some(0));
    icicle.select(Some(1));
    assert!(icicle.key(ViewerKey::Enter));
    assert_eq!(icicle.focus(), Some(1));
    assert!(icicle.key(ViewerKey::Escape));
    assert_eq!(icicle.focus(), None);
}

fn paint(icicle: &mut Icicle, width: f32) -> Scene {
    let mut text = text_layout::TextSystem::new();
    let palette = Palette::dark();
    let icons = Icons::builtin();
    let mut scene = Scene::default();
    icicle.paint(IciclePaint {
        bounds: Rect::new(0.0, 0.0, width, 400.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
    });
    scene
}

#[test]
fn a_click_chooses_and_zooms_a_crumb_goes_back() {
    let mut icicle = Icicle::new();
    icicle.set_format(|v| format!("{v} ms"));
    icicle.set_labels(IcicleLabels {
        all: "All".into(),
        merged: "{count} steps".into(),
        ..IcicleLabels::default()
    });
    icicle.set_nodes(sample());
    paint(&mut icicle, 1020.0);
    // Row 1 starts under the band and the axis; `a` at its left.
    let y = BAND_H + AXIS_H + 2.0 + ROW_H * 1.5;
    assert_eq!(icicle.press(Point::new(100.0, y)), Some(IcicleEvent::Selected(1)));
    assert_eq!((icicle.focus(), icicle.selected()), (Some(1), Some(1)));
    // The tooltip: the node's own.
    paint(&mut icicle, 1020.0);
    let tip = icicle.tooltip_at(Point::new(100.0, BAND_H + AXIS_H + 2.0 + ROW_H * 0.5)).unwrap();
    assert_eq!(tip.label, "a");
    // The first crumb ("All", after ←): the whole tree again.
    let all = icicle.crumbs.iter().find(|(_, h)| *h == Hit::Crumb(None)).unwrap().0;
    assert_eq!(
        icicle.press(Point::new(all.min_x() + 4.0, all.min_y() + 4.0)),
        Some(IcicleEvent::Zoomed)
    );
    assert_eq!(icicle.focus(), None);
}

#[test]
fn a_merged_bar_says_how_many_it_joins() {
    let mut nodes = vec![node(None, 1000.0, 0.0, "root"), node(Some(0), 990.0, 990.0, "big")];
    for i in 0..10 {
        nodes.push(node(Some(0), 1.0, 1.0, &format!("tiny{i}")));
    }
    let mut icicle = Icicle::new();
    icicle.set_format(|v| format!("{v} ms"));
    icicle.set_labels(IcicleLabels { merged: "{count} steps".into(), ..IcicleLabels::default() });
    icicle.set_nodes(nodes);
    paint(&mut icicle, 1020.0);
    let merged = icicle.cells_cached().iter().find(|c| c.node.is_none()).unwrap().clone();
    let r = icicle.cell_rect(&merged);
    let tip =
        icicle.tooltip_at(Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + 4.0)).unwrap();
    assert_eq!(tip.label, "10 steps · 10 ms");
    // A click on it zooms into their parent (wider there).
    let at = Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + 4.0);
    assert_eq!(icicle.press(at), Some(IcicleEvent::Zoomed));
    assert_eq!(icicle.focus(), Some(0));
}

#[test]
fn ten_thousand_nodes_are_laid_out_once_per_width() {
    let mut nodes = vec![node(None, 10_000.0, 0.0, "root")];
    for i in 1..10_000 {
        nodes.push(node(Some((i - 1) / 4), (10_000 - i) as f64 / 10.0, 1.0, "n"));
    }
    let mut icicle = Icicle::new();
    icicle.set_nodes(nodes);
    let started = std::time::Instant::now();
    let n = icicle.cells(1600.0).len();
    let took = started.elapsed();
    assert!(n > 0);
    let budget = if cfg!(debug_assertions) { 300 } else { 10 };
    assert!(took.as_millis() <= budget, "{took:?}");
    // The same width again: the memo, nothing laid out.
    let again = std::time::Instant::now();
    assert_eq!(icicle.cells(1600.0).len(), n);
    assert!(again.elapsed() < took.max(std::time::Duration::from_micros(50)));
    let scene = paint(&mut icicle, 1600.0);
    assert!(scene.quad_count() > 0);
}
