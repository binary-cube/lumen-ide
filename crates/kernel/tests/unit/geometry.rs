use super::*;

#[test]
fn intersection_of_overlapping_rects() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 10.0, 10.0);
    assert_eq!(a.intersect(&b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
}

#[test]
fn touching_rects_do_not_intersect() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(10.0, 0.0, 10.0, 10.0);
    assert_eq!(a.intersect(&b), None);
}

#[test]
fn contains_is_half_open() {
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert!(r.contains(Point::new(0.0, 0.0)));
    assert!(!r.contains(Point::new(10.0, 5.0)));
}
