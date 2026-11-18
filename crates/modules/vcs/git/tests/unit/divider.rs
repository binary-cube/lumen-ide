use super::*;

const KEY: &str = "test.divider";

fn divider() -> Divider {
    Divider::new(Orientation::Vertical, KEY, 0.5, (100.0, 200.0))
}

fn area() -> Rect {
    Rect::new(0.0, 0.0, 1010.0, 400.0)
}

fn lay_out(d: &mut Divider) -> (Rect, Rect) {
    d.split(area(), 10.0, SplitterStyle::default(), 1.0)
}

#[test]
fn the_sides_share_the_space_without_the_strip() {
    let (before, after) = lay_out(&mut divider());
    assert_eq!(before.size.width, 500.0);
    assert_eq!(after.min_x(), 510.0);
    assert_eq!(after.size.width, 500.0);
}

#[test]
fn a_drag_moves_it_and_the_share_is_kept_for_the_project() {
    let (mut d, sizes) = (divider(), LayoutSizes::new());
    lay_out(&mut d);
    assert!(d.press(Point::new(505.0, 50.0), 1, Some(&sizes)));
    assert!(d.drag(Point::new(305.0, 50.0)));
    assert!(d.release(Some(&sizes)));
    assert_eq!(lay_out(&mut d).0.size.width, 300.0);
    let mut again = divider();
    again.take_sizes(&sizes);
    assert_eq!(lay_out(&mut again).0.size.width, 300.0);
}

#[test]
fn neither_side_gets_smaller_than_it_needs() {
    let mut d = divider();
    lay_out(&mut d);
    d.press(Point::new(505.0, 50.0), 1, None);
    d.drag(Point::new(5.0, 50.0));
    assert_eq!(lay_out(&mut d).0.size.width, 100.0);
    d.drag(Point::new(1005.0, 50.0));
    assert_eq!(lay_out(&mut d).1.size.width, 200.0);
}

#[test]
fn without_room_for_both_they_share_it_unless_the_side_after_it_goes_first() {
    let mut shared = divider();
    let (before, after) =
        shared.split(Rect::new(0.0, 0.0, 260.0, 400.0), 10.0, SplitterStyle::default(), 1.0);
    assert_eq!((before.size.width, after.size.width), (125.0, 125.0));
    let mut d = divider().after_first();
    let small = Rect::new(0.0, 0.0, 260.0, 400.0);
    let (before, after) = d.split(small, 10.0, SplitterStyle::default(), 1.0);
    assert_eq!((before.size.width, after.size.width), (50.0, 200.0));
    let tiny = Rect::new(0.0, 0.0, 100.0, 400.0);
    let (before, after) = d.split(tiny, 10.0, SplitterStyle::default(), 1.0);
    assert_eq!((before.size.width, after.size.width), (0.0, 90.0));
}

#[test]
fn two_clicks_bring_it_back_and_forget_the_share() {
    let (mut d, sizes) = (divider(), LayoutSizes::new());
    sizes.set(KEY, 0.2);
    d.take_sizes(&sizes);
    let (before, _) = lay_out(&mut d);
    assert!(d.press(Point::new(before.max_x() + 5.0, 50.0), 2, Some(&sizes)));
    assert_eq!(sizes.get(KEY), None);
    assert_eq!(lay_out(&mut d).0.size.width, 500.0);
}

#[test]
fn a_press_elsewhere_is_not_for_it() {
    let mut d = divider();
    lay_out(&mut d);
    assert!(!d.press(Point::new(20.0, 50.0), 1, None));
    assert!(!d.drag(Point::new(20.0, 50.0)));
}

/// Hidden by a layout that doesn't use it (the commit screen's message beside the lists):
/// not drawn, not taken, no resize cursor where the last layout had it; placed again, back.
#[test]
fn a_hidden_divider_is_neither_drawn_nor_taken() {
    let mut d = divider();
    lay_out(&mut d);
    let on = Point::new(505.0, 50.0);
    d.hide();
    let mut scene = Scene::default();
    d.paint(&mut scene);
    assert_eq!(scene.quad_count(), 0, "nothing drawn");
    assert!(!d.press(on, 1, None), "nothing to take");
    assert!(d.cursor(on).is_none() && !d.hover(on));
    lay_out(&mut d);
    assert!(d.press(on, 1, None), "placed again: back");
}
