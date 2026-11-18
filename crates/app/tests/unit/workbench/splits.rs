use super::*;

/// The separators follow the tree: one handle per separator, the dragged one moves the
/// ratios, the release ends it.
#[test]
fn the_handles_follow_the_tree_and_drag_its_separators() {
    let mut tree = SplitTree::default();
    tree.split(0, Split::Right);
    let mut handles = SplitHandles::default();
    let palette = Palette::default();
    let rects = handles.layout(&tree, Rect::new(0.0, 0.0, 410.0, 100.0), 10.0, 1.0, &palette);
    assert_eq!(rects.len(), 2);
    assert_eq!(handles.handles.len(), 1);
    assert_eq!(handles.drag(&mut tree, Point::new(100.0, 50.0)), None, "none pressed");
    let line = handles.dividers[0].rect;
    let middle = Point::new(line.min_x() + line.size.width / 2.0, line.min_y() + 50.0);
    assert_eq!(handles.begin_drag(middle), Some(0));
    assert_eq!(handles.drag(&mut tree, Point::new(105.0, 50.0)), Some(true));
    let rects = handles.layout(&tree, Rect::new(0.0, 0.0, 410.0, 100.0), 10.0, 1.0, &palette);
    assert!(rects[0].1.size.width < 150.0, "the first group shrank");
    assert!(handles.end_drag());
    assert!(!handles.end_drag());
}
