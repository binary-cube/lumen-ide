use super::*;

/// Reported in a frame, found in it (the latest report wins); a new frame forgets.
#[test]
fn anchors_live_for_one_frame() {
    begin();
    assert_eq!(find("tab"), None);
    report("tab", Rect::new(1.0, 2.0, 3.0, 4.0));
    report("tab", Rect::new(5.0, 6.0, 7.0, 8.0));
    report("button", Rect::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(find("tab"), Some(Rect::new(5.0, 6.0, 7.0, 8.0)));
    begin();
    assert_eq!(find("tab"), None);
}

/// Parts of one kind told apart by their own keys (each tab by its name).
#[test]
fn keyed_anchors_are_told_apart() {
    begin();
    report_keyed("tab", "one", Rect::new(0.0, 0.0, 10.0, 10.0));
    report_keyed("tab", "two", Rect::new(20.0, 0.0, 10.0, 10.0));
    assert_eq!(find_keyed("tab", "two"), Some(Rect::new(20.0, 0.0, 10.0, 10.0)));
    assert_eq!(find_keyed("tab", "three"), None);
    assert_eq!(find("tab"), None, "a kind alone isn't one of its keyed parts");
}
