use super::*;

fn bar() -> Scrollbar {
    let mut bar = Scrollbar::new(ScrollbarStyle::default(), Orientation::Vertical);
    bar.set_track(Rect::new(488.0, 0.0, 12.0, 400.0), 1.0);
    bar
}

#[test]
fn thumb_reflects_viewport_and_offset() {
    let b = bar();
    let m = ScrollMetrics { content: 4000.0, viewport: 400.0, offset: 0.0 };
    assert_eq!(b.thumb_span(m), (0.0, 40.0));
    let end = ScrollMetrics { offset: 3600.0, ..m };
    assert_eq!(b.thumb_span(end), (360.0, 40.0));
}

#[test]
fn dragging_and_track_clicks_move_the_offset() {
    let mut b = bar();
    let m = ScrollMetrics { content: 4000.0, viewport: 400.0, offset: 0.0 };
    // Click on the thumb: the offset doesn't change; dragged halfway → half the way.
    assert_eq!(b.begin_drag(Point::new(494.0, 10.0), m), Some(0.0));
    let half = b.drag_to(Point::new(494.0, 190.0), m).unwrap();
    assert!((half - 1800.0).abs() < 1.0, "{half}");
    assert!(b.end_drag());
    // Click low on the track: the thumb jumps there.
    let jumped = b.begin_drag(Point::new(494.0, 399.0), m).unwrap();
    assert_eq!(jumped, 3600.0);
    // With no content to scroll, the bar doesn't react.
    let short = ScrollMetrics { content: 300.0, viewport: 400.0, offset: 0.0 };
    assert_eq!(b.begin_drag(Point::new(494.0, 10.0), short), None);
}

fn edge(auto: bool) -> EdgeScrollbar {
    let mut e = if auto { EdgeScrollbar::auto_hiding() } else { EdgeScrollbar::default() };
    let area = Rect::new(0.0, 0.0, 500.0, 400.0);
    e.paint(&mut Scene::default(), &Palette::dark(), 1.0, area, 4000.0, 0.0);
    e
}

/// Hidden at rest; a scroll shows it (fading in), it stays a moment after the last one,
/// then fades out; frames are asked only while it fades or its hold ends — none at rest.
#[test]
fn an_auto_hiding_bar_shows_while_scrolled() {
    let mut e = edge(true);
    let t0 = Instant::now() + Duration::from_secs(10);
    assert_eq!(e.opacity_at(t0), 0.0, "hidden at rest");
    assert_eq!(e.next_frame_at(t0), None, "no frames at rest");
    e.observe(4000.0, 120.0, t0);
    assert!(e.opacity_at(t0) < 0.1, "fading in");
    assert_eq!(e.next_frame_at(t0), Some(t0 + FRAME));
    let shown = t0 + FADE_IN;
    assert_eq!(e.opacity_at(shown), 1.0);
    assert_eq!(e.next_frame_at(shown), Some(t0 + HOLD), "one frame when the hold ends");
    // Scrolled again: the hold starts over.
    let later = t0 + HOLD - Duration::from_millis(100);
    e.observe(4000.0, 300.0, later);
    assert_eq!(e.opacity_at(t0 + HOLD + FADE_OUT), 1.0);
    let fading = later + HOLD + FADE_OUT / 2;
    assert!((e.opacity_at(fading) - 0.5).abs() < 0.05, "{}", e.opacity_at(fading));
    let gone = later + HOLD + FADE_OUT;
    assert_eq!(e.opacity_at(gone), 0.0);
    assert_eq!(e.next_frame_at(gone), None);
}

/// Hidden, the pointer in its lane doesn't bring it (only a scroll does); shown, the
/// pointer keeps it (no frames while it rests there); leaving it, the hold starts.
#[test]
fn an_auto_hiding_bar_stays_while_hovered() {
    let mut e = edge(true);
    let t0 = Instant::now() + Duration::from_secs(10);
    assert!(!e.hover_at(Point::new(494.0, 100.0), t0), "hidden: not brought by the pointer");
    assert_eq!(e.opacity_at(t0 + FADE_IN), 0.0);
    assert!(!e.hover_at(Point::new(100.0, 100.0), t0), "nor seen leaving");
    e.observe(4000.0, 120.0, t0);
    assert!(e.hover_at(Point::new(494.0, 100.0), t0 + FADE_IN));
    let much_later = t0 + Duration::from_secs(60);
    assert_eq!(e.opacity_at(much_later), 1.0, "kept while hovered");
    assert_eq!(e.next_frame_at(much_later), None);
    assert!(e.hover_at(Point::new(100.0, 100.0), much_later));
    assert_eq!(e.opacity_at(much_later + HOLD / 2), 1.0);
    assert_eq!(e.opacity_at(much_later + HOLD + FADE_OUT), 0.0);
}

/// Always shown (the default): never fades, never asks for frames.
#[test]
fn a_plain_bar_is_always_shown() {
    let mut e = edge(false);
    let t0 = Instant::now();
    e.observe(4000.0, 200.0, t0);
    assert_eq!(e.opacity_at(t0 + Duration::from_secs(30)), 1.0);
    assert_eq!(e.next_frame_at(t0), None);
}

/// Only the thumb answering the pointer: hovered (widened by whoever animates it), the bar
/// keeps the thin line's room; the rail's look widens it.
#[test]
fn only_the_thumb_grows_when_the_look_says_so() {
    let track = Rect::new(0.0, 0.0, 12.0, 200.0);
    let span = |hover| {
        let style = ScrollbarStyle { hover, ..ScrollbarStyle::default() };
        let mut bar = Scrollbar::new(style, Orientation::Vertical);
        bar.set_track(track, 1.0);
        bar.set_expansion(1.0);
        bar.inner_span()
    };
    assert_eq!(span(ScrollbarHover::Thumb), (2.0, 198.0), "the thin line's room");
    assert_eq!(span(ScrollbarHover::Rail), (7.0, 193.0), "inside the rail");
    assert_eq!(ScrollbarHover::parse("thumb"), Some(ScrollbarHover::Thumb));
    assert_eq!(ScrollbarVisibility::parse("scrolling"), Some(ScrollbarVisibility::WhileScrolling));
    assert_eq!(ScrollbarVisibility::parse("sometimes"), None);
}

/// A lane: its background, its tint, and the part in view a block across the whole lane,
/// its alpha by how the pointer holds it.
#[test]
fn a_lane_draws_its_block_by_the_pointers_state() {
    let p = Palette::dark();
    let look = LaneLook::new(&p, p.editor_background);
    let m = ScrollMetrics { content: 4000.0, viewport: 400.0, offset: 0.0 };
    let paint = |b: &Scrollbar| {
        let mut layer = Layer::default();
        b.paint_lane(&mut layer, m, &look);
        layer.quads
    };
    let mut b = bar();
    let rest = paint(&b);
    assert_eq!(rest.len(), 3, "background, tint, block");
    assert_eq!(rest[0].color, p.editor_background);
    assert_eq!(rest[1].color, p.text.with_alpha(0.025));
    assert_eq!(rest[2].bounds, Rect::new(488.0, 0.0, 12.0, 40.0), "square, the lane's width");
    assert_eq!(rest[2].corner_radius, 0.0);
    assert_eq!(rest[2].color, p.text.with_alpha(0.06));
    b.hover(Point::new(490.0, 10.0));
    assert_eq!(paint(&b)[2].color, p.text.with_alpha(0.10));
    b.begin_drag(Point::new(490.0, 10.0), m);
    assert_eq!(paint(&b)[2].color, p.text.with_alpha(0.14));
    let none = ScrollMetrics { content: 100.0, viewport: 400.0, offset: 0.0 };
    let mut layer = Layer::default();
    b.paint_lane(&mut layer, none, &look);
    assert_eq!(layer.quads.len(), 2, "nothing to scroll: no block");
}

/// A bar in a window that fades (a modal, a menu appearing) fades with it; the same bar
/// drawn plainly is as it was.
#[test]
fn a_bar_fades_with_its_window() {
    let palette = Palette::dark();
    let area = Rect::new(0.0, 0.0, 500.0, 400.0);
    let quads = |fade: Option<f32>| {
        let mut e = EdgeScrollbar::default();
        let mut scene = Scene::default();
        match fade {
            None => e.paint(&mut scene, &palette, 1.0, area, 4000.0, 0.0),
            Some(o) => {
                let ctx = crate::ds::Ctx::new(&palette, 1.0).with_opacity(o);
                e.paint_in(&mut scene, &ctx, area, 4000.0, 0.0);
            }
        }
        scene.layers().iter().flat_map(|l| l.quads.iter().map(|q| q.color.a)).collect::<Vec<_>>()
    };
    let (plain, whole, half) = (quads(None), quads(Some(1.0)), quads(Some(0.5)));
    assert!(!plain.is_empty());
    assert_eq!(plain, whole);
    for (a, b) in plain.iter().zip(&half) {
        assert!((b - a * 0.5).abs() < 1e-4);
    }
}

/// To a screen reader: a bar that can scroll is a scrollbar, its value the share scrolled.
#[test]
fn a_bar_tells_how_far_it_scrolled() {
    let mut e = EdgeScrollbar::default();
    let recording = ui_kit::access::begin();
    let area = Rect::new(0.0, 0.0, 500.0, 400.0);
    e.paint(&mut Scene::default(), &Palette::dark(), 1.0, area, 4000.0, 1800.0);
    let frame = recording.take();
    let bar = frame.nodes().iter().find(|n| n.role == ui_kit::access::Role::ScrollBar);
    assert_eq!(bar.and_then(|b| b.value.as_deref()), Some("50%"));
}
