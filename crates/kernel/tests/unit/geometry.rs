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

/// The union holds both rectangles and nothing past them.
#[test]
fn the_union_holds_both() {
    let u = Rect::new(0.0, 10.0, 20.0, 5.0).union(&Rect::new(30.0, 0.0, 10.0, 10.0));
    assert_eq!(u, Rect::new(0.0, 0.0, 40.0, 15.0));
}

/// A place moved into what holds it and back, and from one unit to another.
#[test]
fn places_move_between_frames_and_units() {
    let origin = Point::new(10.0, 20.0);
    let p = Point::new(3.0, 4.0);
    assert_eq!(p.offset(origin), Point::new(13.0, 24.0));
    assert_eq!(p.offset(origin).relative_to(origin), p);
    assert_eq!(p.scaled(2.0), Point::new(6.0, 8.0));
    let r = Rect::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(r.offset(origin), Rect::new(11.0, 22.0, 3.0, 4.0));
    assert_eq!(r.scaled(2.0), Rect::new(2.0, 4.0, 6.0, 8.0));
}
