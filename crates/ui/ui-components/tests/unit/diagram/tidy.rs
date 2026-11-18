//! A tree laid out tidily: children after their parent, a parent centered over them, the
//! levels lined up, no two cards overlapping whatever the shape (random trees, a fixed
//! seed), both ways; the edges from each child into its parent; 10 000 nodes in budget.

use super::*;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

/// A tree from each node's parent (`None`: a root), in order.
fn tree(parents: &[Option<usize>]) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut children = vec![Vec::new(); parents.len()];
    let mut roots = Vec::new();
    for (i, p) in parents.iter().enumerate() {
        match p {
            Some(p) => children[*p].push(i),
            None => roots.push(i),
        }
    }
    (children, roots)
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.min_x() < b.max_x() - 0.01
        && b.min_x() < a.max_x() - 0.01
        && a.min_y() < b.max_y() - 0.01
        && b.min_y() < a.max_y() - 0.01
}

fn center_x(r: Rect) -> f32 {
    r.min_x() + r.size.width / 2.0
}

#[test]
fn children_go_under_their_parent_centered() {
    // 0 → (1, 2), 2 → 3.
    let (children, roots) = tree(&[None, Some(0), Some(0), Some(2)]);
    let sizes = vec![(200.0, 90.0), (160.0, 90.0), (200.0, 60.0), (120.0, 90.0)];
    let r = tidy(&children, &roots, &sizes, Flow::Down, Gaps::default());
    assert_eq!((r[0].min_x().min(r[1].min_x()), r[0].min_y()), (0.0, 0.0), "the origin");
    for (c, p) in [(1, 0), (2, 0), (3, 2)] {
        assert!(r[c].min_y() >= r[p].max_y() + Gaps::default().level - 0.01, "{c} under {p}");
    }
    // Centered over the first and last child.
    let mid = (center_x(r[1]) + center_x(r[2])) / 2.0;
    assert!((center_x(r[0]) - mid).abs() < 0.01);
    // Siblings apart by their gap, the level's shorter card in the middle of its band.
    assert!((r[2].min_x() - r[1].max_x() - Gaps::default().sibling).abs() < 0.01);
    assert!((r[2].min_y() - (r[1].min_y() + 15.0)).abs() < 0.01);
    // A single child right under its parent.
    assert!((center_x(r[3]) - center_x(r[2])).abs() < 0.01);
}

#[test]
fn flowing_right_swaps_the_axes() {
    let (children, roots) = tree(&[None, Some(0), Some(0)]);
    let sizes = vec![(200.0, 90.0); 3];
    let r = tidy(&children, &roots, &sizes, Flow::Right, Gaps::default());
    assert!(r[1].min_x() >= r[0].max_x() + Gaps::default().level - 0.01);
    assert!(r[2].min_y() >= r[1].max_y() + Gaps::default().sibling - 0.01);
    let mid = (r[1].min_y() + r[2].max_y()) / 2.0;
    assert!((r[0].min_y() + 45.0 - mid).abs() < 0.01);
}

#[test]
fn subtrees_pack_by_their_contours() {
    // Two deep subtrees under one root: their leaves would collide on level 3 if packed by
    // their tops only.
    //        0
    //     1      4
    //     2      5
    //   3 a b  c d 6
    let parents = [
        None,
        Some(0),
        Some(1),
        Some(2),
        Some(0),
        Some(4),
        Some(5),
        Some(2),
        Some(2),
        Some(5),
        Some(5),
    ];
    let (children, roots) = tree(&parents);
    let sizes = vec![(100.0, 40.0); parents.len()];
    let r = tidy(&children, &roots, &sizes, Flow::Down, Gaps::default());
    for i in 0..r.len() {
        for j in i + 1..r.len() {
            assert!(!overlap(r[i], r[j]), "{i} {j}: {:?} {:?}", r[i], r[j]);
        }
    }
    // The second subtree's leaves keep their subtree's gap from the first's.
    let first = [3, 7, 8].iter().map(|&i| r[i].max_x()).fold(f32::MIN, f32::max);
    let second = [6, 9, 10].iter().map(|&i| r[i].min_x()).fold(f32::MAX, f32::min);
    assert!(second - first >= Gaps::default().subtree - 0.01, "{first} {second}");
}

#[test]
fn a_forest_lays_its_roots_side_by_side() {
    let (children, roots) = tree(&[None, Some(0), None, Some(2)]);
    let sizes = vec![(100.0, 40.0); 4];
    let r = tidy(&children, &roots, &sizes, Flow::Down, Gaps::default());
    assert_eq!(r[0].min_y(), r[2].min_y());
    assert!(r[2].min_x() >= r[0].max_x());
    assert!(!overlap(r[1], r[3]));
}

#[test]
fn links_go_from_each_child_into_its_parent() {
    let (children, roots) = tree(&[None, Some(0), Some(0), Some(0)]);
    let sizes = vec![(200.0, 90.0); 4];
    let r = tidy(&children, &roots, &sizes, Flow::Down, Gaps::default());
    let edges = links(&children, &r, Flow::Down);
    assert_eq!(edges.len(), 3);
    let mut entries = Vec::new();
    for e in &edges {
        let (child, parent) = e.kind;
        assert_eq!(parent, 0);
        assert_eq!(e.dir, Dir::Up);
        let first = e.points[0];
        let last = *e.points.last().unwrap();
        assert!((first.y - r[child].min_y()).abs() < 0.01, "from the child's top");
        assert!((last.y - r[0].max_y()).abs() < 0.01, "into the parent's bottom");
        // Axis-aligned.
        for w in e.points.windows(2) {
            assert!((w[0].x - w[1].x).abs() < 0.01 || (w[0].y - w[1].y).abs() < 0.01);
        }
        entries.push(last.x);
    }
    // Each child at a lane of its own, in order, within the parent's side.
    assert!(entries.windows(2).all(|w| w[0] < w[1]));
    assert!(entries.iter().all(|&x| x > r[0].min_x() && x < r[0].max_x()));
    // Flowing right: into the parent's right side.
    let r = tidy(&children, &roots, &sizes, Flow::Right, Gaps::default());
    let edges = links(&children, &r, Flow::Right);
    assert!(edges.iter().all(|e| e.dir == Dir::Left
        && (e.points.last().unwrap().x - r[0].max_x()).abs() < 0.01));
}

#[test]
fn ten_thousand_nodes_are_laid_out_in_budget() {
    // A wide and deep plan: partitions under an Append, each a short chain, and a long chain.
    let mut parents = vec![None];
    for i in 0..4_000 {
        parents.push(Some(0));
        let at = parents.len() - 1;
        parents.push(Some(at));
        if i % 2 == 0 {
            parents.push(Some(at + 1));
        }
    }
    while parents.len() < 10_000 {
        let last = parents.len() - 1;
        parents.push(Some(last));
    }
    let (children, roots) = tree(&parents);
    let sizes = vec![(220.0, 110.0); parents.len()];
    let started = std::time::Instant::now();
    let r = tidy(&children, &roots, &sizes, Flow::Down, Gaps::default());
    let _ = links(&children, &r, Flow::Down);
    let took = started.elapsed();
    let budget = if cfg!(debug_assertions) { 400 } else { 10 };
    assert!(took.as_millis() <= budget, "{took:?}");
    assert!(r.iter().all(|r| r.size.width > 0.0));
}

fn arb_tree() -> impl Strategy<Value = (Vec<Option<usize>>, Vec<(f32, f32)>)> {
    (1usize..60).prop_flat_map(|n| {
        let parents = (0..n)
            .map(|i| {
                if i == 0 {
                    Just(None).boxed()
                } else {
                    prop_oneof![1 => Just(None), 9 => (0..i).prop_map(Some)].boxed()
                }
            })
            .collect::<Vec<_>>();
        let sizes = proptest::collection::vec((40.0f32..260.0, 20.0f32..140.0), n);
        (parents, sizes)
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 300,
        rng_seed: RngSeed::Fixed(395),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn no_two_cards_overlap((parents, sizes) in arb_tree(), right in any::<bool>()) {
        let (children, roots) = tree(&parents);
        let flow = if right { Flow::Right } else { Flow::Down };
        let r = tidy(&children, &roots, &sizes, flow, Gaps::default());
        for i in 0..r.len() {
            prop_assert_eq!((r[i].size.width, r[i].size.height), sizes[i]);
            for j in i + 1..r.len() {
                prop_assert!(!overlap(r[i], r[j]), "{} {}: {:?} {:?}", i, j, r[i], r[j]);
            }
            // After its parent, on the next level.
            if let Some(p) = parents[i] {
                match flow {
                    Flow::Down => prop_assert!(r[i].min_y() >= r[p].max_y()),
                    Flow::Right => prop_assert!(r[i].min_x() >= r[p].max_x()),
                }
            }
        }
    }
}
