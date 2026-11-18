use super::*;

fn viewport() -> Viewport {
    let mut v = Viewport::new(52.0);
    v.scale = 2.0;
    v.bounds = Rect::new(0.0, 0.0, 2000.0, 1200.0);
    v
}

#[test]
fn screen_and_diagram_points_round_trip() {
    let mut v = viewport();
    v.pan = Point::new(-40.0, 10.0);
    v.zoom = 0.75;
    let p = Point::new(120.0, 300.0);
    let back = v.to_diagram(v.to_screen(p));
    assert!((back.x - p.x).abs() < 1e-3 && (back.y - p.y).abs() < 1e-3);
    // The area starts under the toolbar.
    assert_eq!(v.area().min_y(), 104.0);
}

#[test]
fn fit_zooms_within_its_limits_and_centers_a_focus_when_too_large() {
    let mut v = viewport();
    v.fit(Rect::new(0.0, 0.0, 300.0, 200.0), None, 0.6, 1.0);
    assert_eq!(v.zoom, 1.0);
    let huge = Rect::new(0.0, 0.0, 20_000.0, 9_000.0);
    let focus = Rect::new(5_000.0, 4_000.0, 200.0, 100.0);
    v.fit(huge, Some(focus), 0.6, 1.0);
    assert_eq!(v.zoom, 0.6);
    let a = v.area();
    let mid =
        v.to_diagram(Point::new(a.min_x() + a.size.width / 2.0, a.min_y() + a.size.height / 2.0));
    assert!((mid.x - 5_100.0).abs() < 1.0 && (mid.y - 4_050.0).abs() < 1.0, "{mid:?}");
    assert!(!v.fit_pending);
}

#[test]
fn center_on_puts_a_card_in_the_middle() {
    let mut v = viewport();
    v.zoom = 1.3;
    let r = Rect::new(700.0, -200.0, 230.0, 90.0);
    v.center_on(r);
    let a = v.area();
    let mid =
        v.to_diagram(Point::new(a.min_x() + a.size.width / 2.0, a.min_y() + a.size.height / 2.0));
    assert!((mid.x - 815.0).abs() < 0.01 && (mid.y + 155.0).abs() < 0.01, "{mid:?}");
}

#[test]
fn zoom_keeps_the_point_under_the_mouse() {
    let mut v = viewport();
    let at = Point::new(800.0, 600.0);
    let before = v.to_diagram(at);
    assert!(v.zoom_at(1.5, at, 0.2, 2.5));
    let after = v.to_diagram(at);
    assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
    assert!(!v.zoom_at(100.0, at, 0.2, 1.5) || v.zoom == 1.5);
    assert!(!v.zoom_at(2.0, at, 0.2, 1.5), "already at its limit");
}

#[test]
fn a_press_is_a_click_unless_it_moves() {
    let mut v = viewport();
    v.press(Point::new(10.0, 10.0));
    assert!(!v.drag(Point::new(12.0, 10.0)), "a few pixels aren't a pan");
    assert_eq!(v.release(), Release::Click(Point::new(10.0, 10.0)));
    v.press(Point::new(10.0, 10.0));
    assert!(v.drag(Point::new(60.0, 10.0)));
    assert_eq!(v.release(), Release::Panned);
    assert_eq!(v.pan.x, -25.0);
    assert_eq!(v.release(), Release::None);
}

#[test]
fn layout_bounds_hold_every_card() {
    let card = |i: usize, x: f32, y: f32| Card {
        node: i,
        rect: Rect::new(x, y, 100.0, card_height(2, 1)),
        shown: 2,
        more: 1,
    };
    let layout: Layout<()> =
        Layout::new(vec![card(0, -50.0, 0.0), card(1, 200.0, 300.0)], Vec::new());
    assert_eq!(layout.bounds, Rect::new(-50.0, 0.0, 350.0, 300.0 + HEADER_H + 3.0 * ROW_H + PAD));
    assert_eq!(layout.card(1).map(|c| c.rect.min_x()), Some(200.0));
    assert_eq!(card_height(0, 0), HEADER_H);
}

#[test]
fn history_goes_back_in_order() {
    let mut h = History::default();
    h.push("a");
    h.push("b");
    assert_eq!(h.pop(), Some("b"));
    assert_eq!(h.items(), ["a"]);
    h.clear();
    assert!(h.is_empty());
}

/// The middle of `r`.
fn middle(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

#[test]
fn insets_leave_the_rest_of_the_area_to_fit_and_center_in() {
    let mut v = viewport();
    assert_eq!(v.visible(), v.area(), "nothing covers it by default");
    v.insets = Insets { top: 0.0, right: 800.0, bottom: 128.0, left: 0.0 };
    assert_eq!(v.visible(), Rect::new(0.0, 104.0, 1200.0, 968.0));
    // Fitted: the diagram's middle in the middle of what's left.
    let bounds = Rect::new(0.0, 0.0, 1000.0, 400.0);
    v.fit(bounds, None, 0.1, 1.0);
    let at = v.to_screen(middle(bounds));
    let mid = middle(v.visible());
    assert!((at.x - mid.x).abs() < 0.5 && (at.y - mid.y).abs() < 0.5, "{at:?} {mid:?}");
    // The diagram fits in it, not under the bar or the panel.
    let shown = v.rect_to_screen(bounds);
    assert!(contains_rect(v.visible(), shown), "{shown:?}");
    // Centered there too.
    let card = Rect::new(2000.0, 900.0, 200.0, 120.0);
    v.center_on(card);
    let at = v.to_screen(middle(card));
    assert!((at.x - mid.x).abs() < 0.5 && (at.y - mid.y).abs() < 0.5);
    // Too little left: the whole area.
    v.insets.right = 1950.0;
    assert_eq!(v.visible(), v.area());
}

#[test]
fn a_point_is_measured_to_the_nearest_segment() {
    let line = [Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(100.0, 50.0)];
    assert_eq!(distance_to_polyline(Point::new(50.0, 3.0), &line), 3.0);
    assert_eq!(distance_to_polyline(Point::new(104.0, 25.0), &line), 4.0);
    // Past an end: to the end.
    assert_eq!(distance_to_polyline(Point::new(-3.0, 4.0), &line), 5.0);
    assert!(distance_to_polyline(Point::new(0.0, 0.0), &[]).is_infinite());
    let edge = Edge { points: line.to_vec(), dir: Dir::Down, kind: () };
    assert_eq!(edge.distance(Point::new(100.0, 60.0)), 10.0);
}

#[test]
fn label_spots_come_beside_the_longest_segment_holding_them_first() {
    // A short stub, a long vertical run, a short stub.
    let line = [
        Point::new(0.0, 0.0),
        Point::new(30.0, 0.0),
        Point::new(30.0, 300.0),
        Point::new(60.0, 300.0),
    ];
    let spots = label_spots(&line, (80.0, 20.0), 5.0);
    assert_eq!(spots[0], Rect::new(35.0, 140.0, 80.0, 20.0), "right of the run's middle");
    assert_eq!(spots[1], Rect::new(-55.0, 140.0, 80.0, 20.0), "then left of it");
    // Over and under the stubs last.
    assert!(spots.contains(&Rect::new(-25.0, -25.0, 80.0, 20.0)));
    assert_eq!(spots.len(), 3 * 10);
}

#[test]
fn labels_are_placed_clear_of_the_cards_and_of_each_other() {
    let card = Rect::new(0.0, 0.0, 100.0, 100.0);
    let a = vec![Rect::new(50.0, 50.0, 40.0, 10.0), Rect::new(150.0, 50.0, 40.0, 10.0)];
    let b = vec![Rect::new(160.0, 52.0, 40.0, 10.0), Rect::new(150.0, 80.0, 40.0, 10.0)];
    let c = vec![Rect::new(10.0, 10.0, 10.0, 10.0)];
    let placed = place_labels(&[a, b, c], &[card]);
    // The first spot covers the card; the second label's first, the first label.
    assert_eq!(placed, [Some(1), Some(1), None]);
}

#[test]
fn a_rectangle_is_kept_inside_another() {
    let within = Rect::new(0.0, 0.0, 500.0, 300.0);
    assert_eq!(
        clamp_into(Rect::new(-20.0, 290.0, 100.0, 20.0), within),
        Rect::new(0.0, 280.0, 100.0, 20.0)
    );
    let inside = Rect::new(10.0, 10.0, 50.0, 50.0);
    assert_eq!(clamp_into(inside, within), inside);
    assert!(contains_rect(within, inside));
    assert!(!contains_rect(inside, within));
    assert_eq!(inflate(inside, 5.0), Rect::new(5.0, 5.0, 60.0, 60.0));
}

#[test]
fn the_dot_grid_keeps_its_spacing_at_any_zoom() {
    let area = Rect::new(10.0, 100.0, 400.0, 300.0);
    let step = DOT_GRID_STEP * 2.0;
    // The first dot is inside the area, on the grid anchored at the diagram's origin.
    for origin in [Point::new(-130.0, 517.0), Point::new(10.0, 100.0), Point::new(999.5, -3.25)] {
        let (x, y) = dot_grid_start(area, origin, step);
        assert!(x >= area.min_x() && x < area.min_x() + step);
        assert!(y >= area.min_y() && y < area.min_y() + step);
        let (dx, dy) = ((x - origin.x) / step, (y - origin.y) / step);
        assert!((dx - dx.round()).abs() < 1e-3 && (dy - dy.round()).abs() < 1e-3);
    }
}
