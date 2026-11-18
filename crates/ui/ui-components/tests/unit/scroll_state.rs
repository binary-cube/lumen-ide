//! Never past either end, whole pixels, the selected row kept in view.

use super::*;

#[test]
fn it_never_scrolls_past_its_ends() {
    let mut s = ScrollState::default();
    s.set_viewport(100.0);
    s.measure(0.0, 300.0);
    assert!(s.overflows());
    assert_eq!(s.max(), 200.0);
    assert!(s.scroll_by(150.5));
    assert_eq!(s.origin(10.0), 10.0 - 151.0, "whole pixels");
    assert!(s.scroll_by(1_000.0));
    assert_eq!(s.offset(), 200.0);
    assert!(!s.scroll_by(5.0), "at the end");
    assert!(s.scroll_to(-20.0));
    assert_eq!(s.offset(), 0.0);
    // Shorter content measured: the offset follows.
    s.scroll_to(200.0);
    s.measure(0.0, 150.0);
    assert_eq!(s.offset(), 50.0);
    s.measure(0.0, 80.0);
    assert!(!s.overflows());
    assert_eq!(s.offset(), 0.0);
}

#[test]
fn a_row_is_kept_in_view() {
    let mut s = ScrollState::default();
    s.set_viewport(100.0);
    s.measure(0.0, 500.0);
    let view = Rect::new(0.0, 0.0, 50.0, 100.0);
    s.keep_visible(Rect::new(0.0, 120.0, 50.0, 20.0), view);
    assert_eq!(s.offset(), 40.0);
    s.keep_visible(Rect::new(0.0, -30.0, 50.0, 20.0), view);
    assert_eq!(s.offset(), 10.0);
}
