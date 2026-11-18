use super::*;

#[test]
fn the_visible_rows_start_at_the_scrolled_one_and_stop_at_the_last() {
    let list = TreeList::new(28.0, 10);
    // 100 px tall: 4 rows fit (the 4th cut), one more for the bottom edge.
    assert_eq!(list.visible(0.0, 100.0), 0..5);
    assert_eq!(list.visible(30.0, 100.0), 1..6);
    assert_eq!(list.visible(250.0, 100.0), 8..10);
    // Scrolled past the end: nothing.
    assert_eq!(list.visible(1000.0, 100.0), 10..10);
}

#[test]
fn the_row_under_the_pointer_is_found_by_its_slot() {
    let list = TreeList::new(28.0, 3);
    assert_eq!(list.row_at(-0.5), None);
    assert_eq!(list.row_at(0.0), Some(0));
    assert_eq!(list.row_at(27.9), Some(0));
    assert_eq!(list.row_at(28.0), Some(1));
    assert_eq!(list.row_at(84.0), None, "below the last row");
    assert_eq!(list.slot_at(84.0), Some(3), "a slot past the rows is still counted");
}

#[test]
fn guides_are_one_line_per_level_on_whole_pixels() {
    let mut layer = Layer::default();
    let color = Rgba::WHITE;
    guides(&mut layer, 1..3, |level| 10.0 + level as f32 * 16.0 + 4.8, (5.0, 28.0), 1.0, color);
    let xs: Vec<f32> = layer.quads.iter().map(|q| q.bounds.min_x()).collect();
    assert_eq!(xs, vec![31.0, 47.0]);
    assert!(layer.quads.iter().all(|q| q.bounds.min_y() == 5.0 && q.bounds.size.height == 28.0));
}

#[test]
fn the_chevron_points_down_when_open() {
    assert_eq!(chevron_icon(true), "chevron-down");
    assert_eq!(chevron_icon(false), "chevron-right");
}
