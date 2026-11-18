use super::*;

fn rects(tree: &SplitTree) -> Vec<(usize, (f32, f32, f32, f32))> {
    let (groups, _) = tree.layout(Rect::new(0.0, 0.0, 101.0, 101.0), 1.0);
    let mut out: Vec<_> = groups
        .into_iter()
        .map(|(g, r)| (g, (r.min_x(), r.min_y(), r.size.width, r.size.height)))
        .collect();
    out.sort_by_key(|(g, _)| *g);
    out
}

#[test]
fn splits_nest_and_only_divide_their_group() {
    let mut tree = SplitTree::default();
    tree.split(0, Split::Right);
    // The right group is split top / bottom; the left one stays whole.
    tree.split(1, Split::Down);
    assert_eq!(
        rects(&tree),
        [(0, (0.0, 0.0, 50.0, 101.0)), (1, (51.0, 0.0, 50.0, 50.0)), (2, (51.0, 51.0, 50.0, 50.0))]
    );
    // Another right split of the first: a sibling in the same branch (a quarter each).
    tree.split(0, Split::Right);
    assert_eq!(rects(&tree).len(), 4);
    assert_eq!(rects(&tree)[3].1, (50.0, 51.0, 51.0, 50.0));
    // Removing a group gives its space to the neighbor and renumbers.
    tree.remove(1);
    assert_eq!(rects(&tree)[0].1, (0.0, 0.0, 50.0, 101.0));
    assert_eq!(rects(&tree).len(), 3);
}

#[test]
fn dividers_resize_neighbours() {
    let mut tree = SplitTree::default();
    tree.split(0, Split::Right);
    let (_, dividers) = tree.layout(Rect::new(0.0, 0.0, 201.0, 100.0), 1.0);
    tree.drag(&dividers[0], Point::new(150.0, 10.0));
    let (groups, _) = tree.layout(Rect::new(0.0, 0.0, 201.0, 100.0), 1.0);
    assert!((groups[0].1.size.width - 149.0).abs() <= 1.0, "{:?}", groups[0].1);
    // Not below the minimum.
    tree.drag(&dividers[0], Point::new(0.0, 10.0));
    let (groups, _) = tree.layout(Rect::new(0.0, 0.0, 201.0, 100.0), 1.0);
    assert!(groups[0].1.size.width >= 19.0);
}

#[test]
fn spec_round_trips() {
    let mut tree = SplitTree::default();
    tree.split(0, Split::Right);
    tree.split(1, Split::Down);
    let spec = tree.to_spec();
    assert_eq!(spec, "R(0.5000 L0,0.5000 D(0.5000 L1,0.5000 L2))");
    assert_eq!(SplitTree::from_spec(&spec, 3), Some(tree));
    assert!(SplitTree::from_spec(&spec, 2).is_none());
    assert!(SplitTree::from_spec("R(0.5 L0", 1).is_none());
    assert_eq!(SplitTree::from_spec("L0", 1), Some(SplitTree::default()));
}

/// Grabbing a separator without moving changes nothing (it used to jump by the separators'
/// width, wider ones more).
#[test]
fn grabbing_a_separator_keeps_it_in_place() {
    let mut tree = SplitTree::default();
    tree.split(0, Split::Right);
    tree.split(1, Split::Right);
    let area = Rect::new(10.0, 0.0, 900.0, 300.0);
    let line = 12.0;
    let (before, dividers) = tree.layout(area, line);
    for d in &dividers {
        let center = d.rect.min_x() + d.rect.size.width / 2.0;
        tree.drag(d, Point::new(center, 10.0));
    }
    let (after, _) = tree.layout(area, line);
    for (a, b) in before.iter().zip(&after) {
        assert!((a.1.size.width - b.1.size.width).abs() <= 1.0, "{:?} → {:?}", a.1, b.1);
    }
}

/// A separator's double click: its branch's groups get the same share.
#[test]
fn equalize_shares_a_branch_equally() {
    let mut tree = SplitTree::default();
    tree.split(0, Split::Right);
    tree.split(1, Split::Right);
    let rect = Rect::new(0.0, 0.0, 300.0, 100.0);
    let (_, dividers) = tree.layout(rect, 0.0);
    tree.drag(&dividers[0], Point::new(40.0, 0.0));
    let (_, dividers) = tree.layout(rect, 0.0);
    tree.equalize(&dividers[0]);
    let (groups, _) = tree.layout(rect, 0.0);
    let widths: Vec<f32> = groups.iter().map(|(_, r)| r.size.width).collect();
    assert_eq!(widths, [100.0, 100.0, 100.0]);
}

/// A split before a group (a tab dropped on its left / top edge): the new group comes first
/// in that direction and takes the index, the others move one up.
#[test]
fn split_before_puts_the_new_group_first() {
    let mut tree = SplitTree::default();
    tree.split(0, Split::Right);
    tree.split_before(1, Split::Down);
    assert_eq!(tree.to_spec(), "R(0.5000 L0,0.5000 D(0.5000 L1,0.5000 L2))");
    tree.split_before(0, Split::Right);
    assert_eq!(tree.to_spec(), "R(0.2500 L0,0.2500 L1,0.5000 D(0.5000 L2,0.5000 L3))");
}
