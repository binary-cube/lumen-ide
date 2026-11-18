use super::*;

fn e(from: usize, to: usize) -> EdgeSpec {
    EdgeSpec { from, to, min_len: 1, label: None }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.min_x() < b.max_x() && b.min_x() < a.max_x() && a.min_y() < b.max_y() && b.min_y() < a.max_y()
}

fn no_overlaps(l: &Layered) {
    for i in 0..l.nodes.len() {
        for j in i + 1..l.nodes.len() {
            assert!(
                !overlaps(l.nodes[i], l.nodes[j]),
                "{i} {j}: {:?} {:?}",
                l.nodes[i],
                l.nodes[j]
            );
        }
    }
}

#[test]
fn levels_follow_the_edges() {
    let sizes = vec![(80.0, 40.0); 4];
    let l = layout(&sizes, &[e(0, 1), e(0, 2), e(1, 3), e(2, 3)], Options::default());
    assert_eq!(l.ranks, vec![0, 1, 1, 2]);
    assert!(l.nodes[0].max_y() < l.nodes[1].min_y());
    assert!(l.nodes[1].max_y() < l.nodes[3].min_y());
    // The diamond is symmetric: the top and bottom centered over the middle two.
    let mid = (l.nodes[1].center().x + l.nodes[2].center().x) / 2.0;
    assert!((l.nodes[0].center().x - mid).abs() < 1.0);
    assert!((l.nodes[3].center().x - mid).abs() < 1.0);
    no_overlaps(&l);
    assert!(l.width > 0.0 && l.height > 0.0);
}

#[test]
fn cycles_long_edges_and_labels() {
    let sizes = vec![(60.0, 30.0); 4];
    let mut edges = vec![e(0, 1), e(1, 2), e(2, 0), e(0, 3)];
    edges[3].min_len = 3;
    edges[1].label = Some((50.0, 16.0));
    let l = layout(&sizes, &edges, Options::default());
    // The edge back up still starts at its source and ends at its target.
    let back = &l.edges[2];
    assert_eq!(*back.points.first().unwrap(), l.nodes[2].center());
    assert_eq!(*back.points.last().unwrap(), l.nodes[0].center());
    // The long edge goes through hidden nodes, one per level crossed.
    assert!(l.edges[3].points.len() > 2);
    let label = l.edges[1].label.expect("label placed");
    assert!(label.min_y() > l.nodes[1].max_y() && label.max_y() < l.nodes[2].min_y());
    no_overlaps(&l);
}

#[test]
fn across_and_self_loops() {
    let sizes = vec![(100.0, 30.0), (40.0, 40.0)];
    let options = Options { direction: Direction::Right, ..Options::default() };
    let l = layout(&sizes, &[e(0, 1), e(1, 1)], options);
    assert!(l.nodes[0].max_x() < l.nodes[1].min_x());
    assert_eq!(l.nodes[0].size.width, 100.0);
    assert_eq!(l.edges[1].points.len(), 4);
    let up = layout(&sizes, &[e(0, 1)], Options { direction: Direction::Up, ..Options::default() });
    assert!(up.nodes[0].min_y() > up.nodes[1].max_y());
}

#[test]
fn isotonic_fit() {
    assert_eq!(isotonic(&[1.0, 3.0, 2.0, 4.0], &[1.0; 4]), vec![1.0, 2.5, 2.5, 4.0]);
}

#[test]
fn a_wide_graph_stays_quick_and_apart() {
    let n = 300;
    let sizes = vec![(90.0, 36.0); n];
    let edges: Vec<EdgeSpec> = (1..n)
        .map(|i| e((i - 1) / 3, i))
        .chain((0..60).map(|i| e(i * 4 % n, (i * 7 + 13) % n)))
        .collect();
    let started = std::time::Instant::now();
    let l = layout(&sizes, &edges, Options::default());
    assert!(started.elapsed().as_secs_f32() < 5.0, "{:?}", started.elapsed());
    no_overlaps(&l);
}

/// A node's loop to itself and its label have room on its right: a neighbor on its level
/// stands past them (Category → Category beside Brand), in either direction; the node
/// keeps its own size.
#[test]
fn a_self_loop_and_its_label_have_room() {
    let label = Some((60.0, 16.0));
    let sizes = vec![(200.0, 120.0), (200.0, 120.0), (200.0, 120.0)];
    let edges = vec![e(0, 2), e(1, 2), EdgeSpec { from: 0, to: 0, min_len: 1, label }];
    for direction in [Direction::Down, Direction::Right] {
        let l = layout(&sizes, &edges, Options { direction, ..Options::default() });
        assert_eq!(l.nodes[0].size.width, 200.0, "{direction:?}: its own size");
        let edge = &l.edges[2];
        let reach = edge.points.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let label = edge.label.unwrap();
        let ends = reach.max(label.max_x());
        for (i, other) in l.nodes.iter().enumerate().skip(1) {
            let beside = other.min_y() < l.nodes[0].max_y() && l.nodes[0].min_y() < other.max_y();
            if beside && other.min_x() > l.nodes[0].min_x() {
                assert!(
                    other.min_x() >= ends,
                    "{direction:?}: node {i} at {other:?}, loop to {ends}"
                );
            }
        }
        no_overlaps(&l);
    }
}
