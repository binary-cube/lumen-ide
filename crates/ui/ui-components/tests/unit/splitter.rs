use super::*;

#[test]
fn drag_keeps_the_grab_offset() {
    let mut splitter = Splitter::default();
    splitter.set_bounds(Rect::new(100.0, 0.0, 10.0, 500.0), 1.0);
    assert!(!splitter.begin_drag(Point::new(50.0, 10.0)));
    assert!(splitter.begin_drag(Point::new(103.0, 10.0)));
    // Grabbed 2px left of center (105): the center follows the cursor with the same offset.
    assert_eq!(splitter.drag_to(Point::new(203.0, 300.0)), Some(205.0));
    assert!(splitter.end_drag());
    assert_eq!(splitter.drag_to(Point::new(0.0, 0.0)), None);
}

#[test]
fn horizontal_splitter_drags_vertically() {
    let mut splitter = Splitter::new(SplitterStyle::default(), Orientation::Horizontal);
    splitter.set_bounds(Rect::new(0.0, 300.0, 800.0, 10.0), 1.0);
    assert!(splitter.begin_drag(Point::new(400.0, 306.0)));
    assert_eq!(splitter.drag_to(Point::new(10.0, 206.0)), Some(205.0));
    let mut scene = Scene::default();
    splitter.paint(&mut scene);
    // The handle lies flat: wider than tall.
    let handle = scene.layers()[0].quads.iter().find(|q| q.corner_radius > 2.0).unwrap();
    assert!(handle.bounds.size.width > handle.bounds.size.height);
}

#[test]
fn line_fades_towards_the_ends() {
    let mut splitter = Splitter::default();
    splitter.set_bounds(Rect::new(0.0, 0.0, 10.0, 480.0), 1.0);
    let mut scene = Scene::default();
    splitter.paint(&mut scene);
    let quads = &scene.layers()[0].quads;
    let (first, middle) = (quads[0].color.a, quads[24].color.a);
    assert!(first < 0.2 && middle > 0.9, "end {first}, middle {middle}");
}

#[test]
fn hover_starts_a_short_transition() {
    let mut splitter = Splitter::default();
    splitter.set_bounds(Rect::new(0.0, 0.0, 10.0, 480.0), 1.0);
    assert!(!splitter.is_animating());
    assert!(splitter.hover(Point::new(5.0, 100.0)));
    assert!(splitter.is_animating());
    std::thread::sleep(TRANSITION + Duration::from_millis(20));
    assert!(!splitter.is_animating());
    assert!((splitter.activity(Instant::now()) - 1.0).abs() < 1e-6);
    // The exit starts from the current state toward rest.
    assert!(splitter.hover(Point::new(50.0, 100.0)));
    assert!(splitter.is_animating());
}

#[test]
fn the_handle_is_on_top_only_when_painted_after_both_panes() {
    let mut splitter = Splitter::default();
    splitter.set_bounds(Rect::new(200.0, 0.0, 6.0, 400.0), 1.0);
    let handle = splitter.handle();
    // Wider than its strip: it reaches over both panes.
    assert!(handle.min_x() < 200.0 && handle.max_x() > 206.0, "{handle:?}");
    let panes = |scene: &mut Scene| {
        scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 200.0, 400.0), Rgba::WHITE));
        scene
            .push_layer(Some(Rect::new(206.0, 0.0, 300.0, 400.0)))
            .quad(Quad::filled(Rect::new(206.0, 0.0, 300.0, 400.0), Rgba::WHITE));
    };
    let mut after = Scene::default();
    panes(&mut after);
    splitter.paint(&mut after);
    assert!(splitter.is_on_top(&after));
    // The right pane painted after it eats its right half.
    let mut before = Scene::default();
    before.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 200.0, 400.0), Rgba::WHITE));
    splitter.paint(&mut before);
    before
        .push_layer(Some(Rect::new(206.0, 0.0, 300.0, 400.0)))
        .quad(Quad::filled(Rect::new(206.0, 0.0, 300.0, 400.0), Rgba::WHITE));
    assert!(!splitter.is_on_top(&before));
    // A later layer clipped away from it, or a transparent quad, doesn't cover it.
    let mut clipped = Scene::default();
    splitter.paint(&mut clipped);
    clipped
        .push_layer(Some(Rect::new(300.0, 0.0, 100.0, 400.0)))
        .quad(Quad::filled(Rect::new(0.0, 0.0, 600.0, 400.0), Rgba::WHITE));
    clipped
        .push_layer(None)
        .quad(Quad::filled(Rect::new(0.0, 0.0, 600.0, 400.0), Rgba::TRANSPARENT));
    assert!(splitter.is_on_top(&clipped));
    // Not painted at all: not on top.
    assert!(!splitter.is_on_top(&Scene::default()));
}
