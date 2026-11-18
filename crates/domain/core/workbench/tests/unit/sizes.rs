use super::*;

#[test]
fn sizes_are_shared_by_clones_and_change_their_generation() {
    let sizes = LayoutSizes::new();
    let other = sizes.clone();
    assert_eq!(sizes.get(DOCK_BOTTOM), None);
    let before = sizes.generation();
    other.set(DOCK_BOTTOM, 360.0);
    assert_eq!(sizes.get(DOCK_BOTTOM), Some(360.0));
    assert!(sizes.generation() > before);
    // The same value again: nothing changed.
    let now = sizes.generation();
    sizes.set(DOCK_BOTTOM, 360.0);
    assert_eq!(sizes.generation(), now);
    // Not a number: ignored.
    sizes.set(DOCK_LEFT, f32::NAN);
    assert_eq!(sizes.get(DOCK_LEFT), None);
    sizes.remove(DOCK_BOTTOM);
    assert_eq!(other.get(DOCK_BOTTOM), None);
}

#[test]
fn entries_are_sorted_and_replaced_wholesale() {
    let sizes = LayoutSizes::new();
    sizes.set(DOCK_RIGHT, 300.0);
    sizes.set(DOCK_BOTTOM, 400.0);
    assert_eq!(sizes.entries(), [(DOCK_BOTTOM.to_owned(), 400.0), (DOCK_RIGHT.to_owned(), 300.0)]);
    sizes.replace(&[("x".to_owned(), 1.0), ("y".to_owned(), f32::INFINITY)]);
    assert_eq!(sizes.entries(), [("x".to_owned(), 1.0)]);
}
