use super::*;

/// A handle dragged moves its sides, snapped to the grid, never past the other side.
#[test]
fn handles_resize_their_sides() {
    let r = Rect::new(100.0, 100.0, 80.0, 40.0);
    assert_eq!(
        resized(r, Handle::SE, (17.0, 12.0), Some(10.0)),
        Rect::new(100.0, 100.0, 100.0, 50.0)
    );
    assert_eq!(
        resized(r, Handle::W, (-13.0, 50.0), None),
        Rect::new(87.0, 100.0, 93.0, 40.0),
        "only its side"
    );
    assert_eq!(
        resized(r, Handle::N, (0.0, 500.0), None).size.height,
        1.0,
        "never past the other side"
    );
    let hs = handles(r, 8.0);
    assert_eq!(hs[0], (Handle::NW, Rect::new(96.0, 96.0, 8.0, 8.0)));
    assert_eq!(hs[4].0, Handle::SE);
}

/// A move snaps to another shape's side or middle (a guide there), else to the grid.
#[test]
fn moves_snap_to_shapes_then_to_the_grid() {
    let moving = Rect::new(0.0, 0.0, 50.0, 50.0);
    let others = [Rect::new(203.0, 300.0, 40.0, 40.0)];
    // Its left 3 from the other's left, its middle 2 from the other's middle: the closer.
    let (dx, dy, guides) = snap_move(moving, &others, (200.0, 17.0), Some(10.0), 5.0);
    assert_eq!((dx, dy), (198.0, 20.0), "nothing near across: the grid down");
    assert_eq!(guides, [Guide::X(223.0)]);
    // Only its left near: there.
    let (dx, _, guides) = snap_move(moving, &others, (204.0, 0.0), None, 2.0);
    assert_eq!((dx, guides), (203.0, vec![Guide::X(203.0)]));
    // Its middle on the other's middle.
    let (_, dy, guides) = snap_move(moving, &others, (0.0, 293.0), None, 5.0);
    assert_eq!(dy, 295.0);
    assert_eq!(guides, [Guide::Y(320.0)]);
    let (dx, _, guides) = snap_move(moving, &[], (14.0, 0.0), Some(10.0), 5.0);
    assert_eq!((dx, guides.len()), (10.0, 0));
}

/// What's at a point: an edge near it first, else the topmost shape; what a band holds whole.
#[test]
fn what_s_under_the_mouse_and_in_a_band() {
    let d = crate::drawing::tests::flow();
    assert_eq!(hit(&d, Point::new(400.0, 200.0), 3.0).as_deref(), Some("check"));
    assert_eq!(
        hit(&d, Point::new(400.0, 130.0), 3.0).as_deref(),
        Some("e1"),
        "on the edge between"
    );
    assert_eq!(hit(&d, Point::new(5.0, 5.0), 3.0), None);
    assert_eq!(
        hit(&d, Point::new(210.0, 400.0), 3.0).as_deref(),
        Some("g-b"),
        "a group's child over it"
    );
    let inside = within(&d, Rect::new(330.0, 30.0, 140.0, 220.0));
    assert!(
        inside.contains(&"start".to_owned())
            && inside.contains(&"check".to_owned())
            && inside.contains(&"e1".to_owned())
    );
    assert!(!inside.contains(&"save".to_owned()));
}

/// A shape's connection points; a chosen edge's handles (ends, points, middles to add one).
#[test]
fn connection_points_and_edge_handles() {
    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    assert_eq!(
        connection_points(r),
        [
            Point::new(50.0, 0.0),
            Point::new(100.0, 25.0),
            Point::new(50.0, 50.0),
            Point::new(0.0, 25.0)
        ]
    );
    let way = [Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(100.0, 100.0)];
    let handles = edge_handles(&way, &[Point::new(100.0, 0.0)]);
    assert_eq!(handles[0], (EdgeHandle::Source, Point::new(0.0, 0.0)));
    assert_eq!(handles[1], (EdgeHandle::Target, Point::new(100.0, 100.0)));
    assert_eq!(handles[2], (EdgeHandle::Point(0), Point::new(100.0, 0.0)));
    assert_eq!(handles[3], (EdgeHandle::Insert(0), Point::new(50.0, 0.0)));
    assert_eq!(handles[4], (EdgeHandle::Insert(1), Point::new(100.0, 50.0)));
}
