use super::*;
use icons::Icons;
use render_api::Scene;
use text_layout::TextSystem;

#[test]
fn averages_recent_frames_and_ignores_skipped() {
    let mut o = DebugOverlay::new("test".into());
    o.record(Duration::from_millis(2), FrameStats::default());
    o.record(Duration::from_millis(4), FrameStats::default());
    o.record(Duration::from_millis(100), FrameStats { skipped: true, ..FrameStats::default() });
    assert_eq!(o.timings(), (Duration::from_millis(3), Duration::from_millis(4)));
}

#[test]
fn latency_p95() {
    let mut o = DebugOverlay::new("test".into());
    assert_eq!(o.latency_p95(), None);
    for ms in 1..=20 {
        o.record_latency(Duration::from_millis(ms));
    }
    assert_eq!(o.latency_p95(), Some(Duration::from_millis(19)));
}

fn paint(o: &mut DebugOverlay, scene: &mut Scene, area: Rect) {
    let (mut text, palette, icons) = (TextSystem::new(), Palette::dark(), Icons::builtin());
    let painter = crate::Painter { text: &mut text, scene, palette: &palette, icons: &icons };
    o.paint(painter, area, Size::new(800.0, 600.0), 1.0);
}

#[test]
fn hidden_overlay_draws_nothing() {
    let mut o = DebugOverlay::new("test".into());
    let mut scene = Scene::new(Rgba::BLACK);
    paint(&mut o, &mut scene, Rect::new(0.0, 0.0, 800.0, 600.0));
    assert!(scene.layers().is_empty());
}

#[test]
fn sits_in_the_bottom_right_corner_of_its_area() {
    let mut o = DebugOverlay::new("test".into());
    o.toggle();
    let mut scene = Scene::new(Rgba::BLACK);
    let area = Rect::new(40.0, 50.0, 700.0, 500.0);
    paint(&mut o, &mut scene, area);
    let layer = &scene.layers()[0];
    let panel = layer.quads[0].bounds;
    assert_eq!((panel.max_x(), panel.max_y()), (area.max_x(), area.max_y()));
    assert!(panel.min_y() >= area.min_y() && panel.min_x() >= area.min_x());
}

/// A stream of frames, one every 16 ms for two seconds, each different: the table it shows
/// changes at most twice a second (readable), and it never asks for a frame of its own —
/// shown with nothing happening, the app stays idle.
#[test]
fn the_table_changes_at_most_twice_a_second_and_asks_for_no_frames() {
    let mut o = DebugOverlay::new("test".into());
    o.toggle();
    assert_eq!(o.frame_deadline(), None, "shown: no frames asked for");
    let start = Instant::now();
    let mut changes = 0;
    for i in 0..125u64 {
        let stats =
            FrameStats { quads: i as usize, glyphs: 3 * i as usize, ..FrameStats::default() };
        o.record(Duration::from_micros(1_000 + 37 * i), stats);
        let now = start + Duration::from_millis(16 * i);
        if o.refresh(now) {
            changes += 1;
        }
        assert_eq!(o.frame_deadline(), None, "frame {i}");
    }
    // 2 s of frames: the first table, then one at 0.5 s, 1 s, 1.5 s.
    assert!(changes <= 4, "{changes} changes");
    assert!(changes >= 2, "it follows the frames");
    // The labels are the locales'; the values have their units.
    let labels: Vec<&str> = o.rows().iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels[1], i18n::t("debug.frame"));
    assert!(o.rows()[1].value.ends_with("ms"));
}

/// The values with a budget are colored by it: a frame over 16 ms is bad, over 8 warned.
#[test]
fn values_are_colored_by_their_budget() {
    assert_eq!(tone(4.0, 8.0, 16.0), Tone::Good);
    assert_eq!(tone(9.0, 8.0, 16.0), Tone::Warn);
    assert_eq!(tone(20.0, 8.0, 16.0), Tone::Bad);
    let mut o = DebugOverlay::new("test".into());
    for _ in 0..5 {
        o.record(Duration::from_millis(20), FrameStats::default());
    }
    o.refresh(Instant::now());
    assert_eq!(o.rows()[1].tone, Tone::Bad);
    assert_eq!(o.rows()[2].tone, Tone::Plain, "no input yet");
}

/// Its × (top right) takes the click and hides it, as F12; elsewhere the panel lets clicks
/// through. The mouse on it: hovered, its tooltip on its way.
#[test]
fn its_close_button_hides_it_and_nothing_else_takes_clicks() {
    let mut o = DebugOverlay::new("test".into());
    o.toggle();
    let mut scene = Scene::new(Rgba::BLACK);
    let area = Rect::new(0.0, 0.0, 800.0, 600.0);
    paint(&mut o, &mut scene, area);
    let panel = scene.layers()[0].quads[0].bounds;
    let close = o.close.expect("drawn");
    assert!(close.max_x() <= panel.max_x() && close.min_y() >= panel.min_y());
    let middle_of =
        |r: Rect| Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0);
    let centre = middle_of(panel);
    assert!(close.min_x() > centre.x && close.max_y() < centre.y, "top right");
    let middle = Point::new(panel.min_x() + 10.0, panel.max_y() - 10.0);
    assert!(!o.click(middle), "the rest lets clicks through");
    assert!(o.is_visible());
    let on = middle_of(close);
    assert!(o.hover(on));
    assert!(o.close_hovered());
    assert!(o.frame_deadline().is_some(), "its tooltip on its way");
    assert!(o.click(on));
    assert!(!o.is_visible(), "hidden, as F12");
    assert_eq!(o.frame_deadline(), None);
    assert!(!o.click(on), "hidden: nothing to click");
}
