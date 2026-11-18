use super::*;
use render_api::Scene;

#[test]
fn solid_after_input_and_schedules_a_toggle() {
    configure(CaretLook::default());
    reset();
    assert!(visible());
    let next = take_next_frame();
    assert!(next.is_some_and(|t| t > Instant::now()));
    // Nothing painted since: no redraw needed.
    assert_eq!(take_next_frame(), None);
}

/// Classic: on, then off; smooth: it holds, then fades out and back in; off: always shown.
#[test]
fn blinks_as_the_look_says() {
    let at = |ms| Duration::from_millis(ms);
    assert_eq!(blink_opacity(Blink::Classic, at(100)), 1.0);
    assert_eq!(blink_opacity(Blink::Classic, at(600)), 0.0);
    assert_eq!(blink_opacity(Blink::Off, at(600)), 1.0);
    assert_eq!(blink_opacity(Blink::Smooth, at(100)), 1.0, "holding");
    let fading = blink_opacity(Blink::Smooth, at(450));
    assert!(fading > 0.0 && fading < 1.0, "fading out: {fading}");
    assert_eq!(blink_opacity(Blink::Smooth, at(600)), 0.0, "off");
    assert!(blink_opacity(Blink::Smooth, at(1000)) > 0.0, "fading in");
}

/// The shape and thickness; gliding to a new place, then still: frames only while it moves;
/// a caret that doesn't blink asks for none.
#[test]
fn draws_its_shape_and_asks_frames_only_while_moving() {
    let cell = Rect::new(10.0, 20.0, 8.0, 18.0);
    assert_eq!(shape_rect(cell, CaretShape::Bar, 4.0), Rect::new(10.0, 20.0, 4.0, 18.0));
    assert_eq!(shape_rect(cell, CaretShape::Block, 2.0), cell);
    assert_eq!(shape_rect(cell, CaretShape::Underline, 2.0), Rect::new(10.0, 36.0, 8.0, 2.0));

    configure(CaretLook {
        blink: Blink::Off,
        motion: Motion::Glide,
        trail_ms: 100,
        glow: 0.5,
        ..CaretLook::default()
    });
    reset();
    let mut scene = Scene::new(Rgba::TRANSPARENT);
    let layer = scene.push_layer(None);
    let caret = |x: f32| CaretPaint {
        id: 7,
        cell: Rect::new(x, 0.0, 8.0, 18.0),
        color: Rgba::rgb(0x00ff41),
        scale: 1.0,
        shape: None,
        thickness: None,
        steady: false,
    };
    assert!(paint(layer, caret(0.0)).is_some());
    assert_eq!(take_next_frame(), None, "still and not blinking: no frames");
    let drawn = paint(layer, caret(80.0)).unwrap();
    assert!(drawn.min_x() < 80.0, "gliding there");
    assert!(take_next_frame().is_some_and(|t| t <= Instant::now() + FRAME));
    std::thread::sleep(Duration::from_millis(130));
    assert_eq!(paint(layer, caret(80.0)).unwrap().min_x(), 80.0, "arrived");
    assert_eq!(take_next_frame(), None, "the trail faded: still again");
    configure(CaretLook::default());
}

/// Without input for a while the caret rests: shown solid, its last frame the one at rest,
/// then none; the pointer moving wakes it (a blinking one goes on as it was).
#[test]
fn the_caret_rests_after_a_while_and_wakes_on_input() {
    for blink in [Blink::Classic, Blink::Smooth] {
        assert_eq!(blink_opacity(blink, REST), 1.0);
        assert_eq!(blink_opacity(blink, REST + HALF + Duration::from_millis(10)), 1.0);
    }
    configure(CaretLook::default());
    // Just before resting: the next frame is the one at rest at the latest.
    let start = Instant::now() - REST + Duration::from_millis(100);
    EPOCH.with(|e| e.set(Some(start)));
    let _ = opacity();
    let next = take_next_frame().expect("one more frame");
    assert!(next <= start + REST);
    // At rest: shown, no frame asked for.
    EPOCH.with(|e| e.set(Some(Instant::now() - REST)));
    assert!(visible());
    assert_eq!(take_next_frame(), None, "no frames at rest");
    // The pointer: awake again, blinking.
    assert!(wake());
    assert!(visible());
    assert!(take_next_frame().is_some());
    assert!(!wake(), "already awake: nothing to do");
}
