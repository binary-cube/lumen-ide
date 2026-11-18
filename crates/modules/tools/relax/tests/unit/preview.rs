use super::*;

use render_api::Scene;
use text_layout::TextSystem;
use ui_components::Palette;

fn paint(
    previews: &ScenePreviews,
    value: i64,
    live: bool,
    palette: &Palette,
    now: Instant,
) -> Scene {
    let mut scene = Scene::default();
    let mut text = TextSystem::new();
    let bounds = Rect::new(100.0, 100.0, 360.0, 225.0);
    previews.paint_at(
        value,
        live,
        CardPaint { bounds, clip: bounds, scale: 2.0, scene: &mut scene, text: &mut text, palette },
        now,
    );
    scene
}

fn quads(scene: &Scene) -> Vec<render_api::Quad> {
    scene.layers().iter().flat_map(|l| l.quads.clone()).collect()
}

#[test]
fn still_frames_are_placed_once_until_what_they_show_changes() {
    let previews = ScenePreviews::default();
    let palette = Palette::dark();
    let now = Instant::now();
    for value in 0..SCENES.len() as i64 {
        let first = paint(&previews, value, false, &palette, now);
        let images: usize = first.layers().iter().map(|l| l.images.len()).sum();
        assert!(!quads(&first).is_empty() || images > 10, "{value}: drawn");
        // Clipped to the card.
        assert!(
            first.layers().iter().all(|l| l.clip == Some(Rect::new(100.0, 100.0, 360.0, 225.0)))
        );
        let again = paint(&previews, value, false, &palette, now + Duration::from_secs(5));
        assert_eq!(quads(&first), quads(&again), "{value}: still");
    }
    assert_eq!(previews.stills_made(), SCENES.len() as u64, "each once");
    // Another theme: made again.
    let mut light = Palette::dark();
    light.window_background = Rgba::rgb(0xf4f5f8);
    paint(&previews, 0, false, &light, now);
    assert_eq!(previews.stills_made(), SCENES.len() as u64 + 1);
    // "In turn" still: a band of every scene, none of them alone.
    let turn = paint(&previews, SCENES.len() as i64, false, &palette, now);
    let clips: std::collections::HashSet<String> =
        turn.layers().iter().map(|l| format!("{:?}", l.clip)).collect();
    assert_eq!(clips.len(), SCENES.len());
    // The page closed: all of it goes.
    assert!(previews.memory() > 0);
    previews.release();
    assert_eq!(previews.memory(), 0);
}

#[test]
fn the_live_card_plays_and_in_turn_goes_through_the_scenes() {
    let previews = ScenePreviews::default();
    let palette = Palette::dark();
    let now = Instant::now();
    let a = paint(&previews, 1, true, &palette, now);
    let b = paint(&previews, 1, true, &palette, now + Duration::from_millis(80));
    assert_ne!(quads(&a), quads(&b), "it moves");
    assert_eq!(previews.stills_made(), 0, "a live card makes no still");
    assert_eq!(previews.frame_interval(), Duration::from_millis(33), "about 30 a second");
    // "In turn": a scene, then the next after a while.
    assert_eq!(scene_of(SCENES.len() as i64, 0.0).id, SCENES[0].id);
    assert_eq!(scene_of(SCENES.len() as i64, TURN + 0.1).id, SCENES[1].id);
    assert_eq!(scene_of(2, 100.0).id, SCENES[2].id, "a scene's card: always it");
}
