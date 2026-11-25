//! The theme's decorations in the window: nothing made for a theme without them; for the
//! Christmas theme a layer right above the window's background, behind every card; frames
//! asked only while the window is shown, focused and in use (none once idle, hidden,
//! unfocused or still); a frame of only them redrawing their layer and nothing else; let
//! go when the theme changes.

use std::time::{Duration, Instant};

use super::*;

fn theme(id: &str) -> ResolvedTheme {
    let mut registry = theming::ThemeRegistry::default();
    for theme in themes_builtin::builtin_themes() {
        registry.register(theme);
    }
    registry.resolve(&theming::ThemeId::parse(id)).unwrap()
}

fn window(theme_id: &str) -> (Workbench, TextSystem, Scene) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(2400, 1400), 2.0);
    wb.set_theme(theme(theme_id));
    wb.set_decorations_window(true, true, false);
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    (wb, text, scene)
}

#[test]
fn a_theme_without_decorations_makes_nothing_and_asks_no_frame() {
    let (wb, _, _) = window("default-dark");
    assert!(!wb.decorations_drawn());
    assert_eq!(wb.decorations_deadline(Instant::now()), None);
    assert_eq!(wb.decorations_memory(), (0, 0));
}

#[test]
fn the_christmas_theme_draws_behind_the_cards() {
    let (wb, _, scene) = window("christmas-dark");
    assert!(wb.decorations_drawn());
    let index = wb.chrome.decorations.layer.unwrap();
    let layer = &scene.layers()[index];
    assert!(!layer.images.is_empty(), "the garland drawn");
    // Before the editor's card: covered by it.
    let card =
        scene.layers().iter().position(|l| l.quads.iter().any(|q| q.bounds == wb.card)).unwrap();
    assert!(index < card, "behind the editor's card");
    // Nothing of it wholly under a card (not drawn there).
    for image in &layer.images {
        assert!(!ui_workbench::decorations::hidden(&image.bounds, &wb.chrome.decorations.covered));
    }
    assert!(wb.decorations_memory().0 > 0);
}

#[test]
fn frames_are_asked_only_while_the_window_is_in_use() {
    let (mut wb, _, _) = window("christmas-dark");
    let now = Instant::now();
    wb.decorations_input(now);
    let next = wb.decorations_deadline(now).expect("moving while in use");
    // At the pace set (the user's setting, read by the module: 10–30 frames a second).
    let fps = wb.chrome.decorations.fps;
    assert!((1..=ui_workbench::decorations::MAX_FPS).contains(&fps));
    assert_eq!(next - now, Duration::from_secs(1) / fps, "at most {fps} frames a second");
    // Unfocused, hidden, still (static, reduced motion, saving power): none.
    for (focused, visible, still) in
        [(false, true, false), (true, false, false), (true, true, true)]
    {
        wb.set_decorations_window(focused, visible, still);
        assert_eq!(wb.decorations_deadline(now), None, "{focused} {visible} {still}");
    }
    wb.set_decorations_window(true, true, false);
    // No input for a while: no more frames (a still frame, 0 CPU) — until input comes (after
    // the pause set; none when they never stop).
    let Some(pause) = wb.chrome.decorations.idle else { return };
    let idle = now + pause + Duration::from_secs(1);
    assert_eq!(wb.decorations_deadline(idle), None, "idle: no redraw requested");
    wb.decorations_input(idle);
    assert!(wb.decorations_deadline(idle).is_some());
}

#[test]
fn a_frame_of_only_the_decorations_redraws_their_layer_alone() {
    let (mut wb, _, mut scene) = window("christmas-dark");
    let index = wb.chrome.decorations.layer.unwrap();
    let others: Vec<(usize, usize, usize)> =
        scene.layers().iter().map(|l| (l.quads.len(), l.images.len(), l.glyphs.len())).collect();
    std::thread::sleep(Duration::from_millis(80));
    assert!(wb.repaint_decorations(&mut scene));
    for (i, l) in scene.layers().iter().enumerate() {
        if i != index {
            assert_eq!((l.quads.len(), l.images.len(), l.glyphs.len()), others[i], "layer {i}");
        }
    }
    assert!(!scene.layers()[index].images.is_empty());
}

#[test]
fn another_theme_lets_them_go() {
    let (mut wb, mut text, mut scene) = window("christmas-light");
    assert!(wb.decorations_drawn());
    wb.set_theme(theme("default-light"));
    wb.paint(&mut text, &mut scene);
    assert!(!wb.decorations_drawn());
    assert_eq!(wb.decorations_memory(), (0, 0), "nothing kept");
    assert!(!wb.repaint_decorations(&mut scene));
}

/// A view drawing the whole window (Relax mode) suspends them: not drawn, no frame of
/// only them, none asked, their clock stopped; once it closes they're back from where they
/// were.
#[test]
fn a_view_covering_the_window_suspends_them_until_it_closes() {
    for id in ["christmas-dark", "matrix"] {
        let (mut wb, mut text, mut scene) = window(id);
        assert!(wb.decorations_drawn(), "{id}");
        let now = Instant::now();
        wb.decorations_input(now);
        assert!(wb.decorations_deadline(now).is_some());
        assert_eq!(wb.run_command(command_model::CommandId("relax.start")), Some(true));
        scene.reset(Rgba::BLACK);
        wb.paint(&mut text, &mut scene);
        let time = wb.chrome.decorations.clock.time();
        assert!(!wb.decorations_drawn(), "{id}: not in the scene");
        assert!(!wb.repaint_decorations(&mut scene), "{id}: no frame of only them");
        assert_eq!(wb.decorations_deadline(Instant::now()), None, "{id}: no frames asked");
        std::thread::sleep(Duration::from_millis(60));
        wb.paint(&mut text, &mut scene);
        assert_eq!(wb.chrome.decorations.clock.time(), time, "{id}: their clock stopped");
        wb.edit_key(&mut text, EditKey::Escape);
        assert!(!wb.has_modal_view());
        scene.reset(Rgba::BLACK);
        wb.paint(&mut text, &mut scene);
        assert!(wb.decorations_drawn(), "{id}: back");
        assert!(wb.decorations_deadline(Instant::now()).is_some(), "{id}: moving again");
        assert!(wb.chrome.decorations.clock.time() - time < 0.05, "{id}: from where they were");
    }
}

/// Over minutes: idle (by design) they stand still; the window unfocused, hidden, covered
/// by Relax mode (started by itself while unfocused) — and back in front, they move again
/// at once, their clock going on, without any other input.
#[test]
fn they_move_again_whenever_the_window_comes_back() {
    let (mut wb, mut text, mut scene) = window("matrix");
    let start = Instant::now();
    wb.decorations_input(start);
    let pause = wb.chrome.decorations.idle.unwrap_or(Duration::from_secs(20));
    assert!(wb.decorations_deadline(start).is_some());
    // Minutes without input: still (by design).
    let later = start + pause + Duration::from_secs(120);
    assert_eq!(wb.decorations_deadline(later), None);
    // Unfocused, then hidden, then back and focused: moving again, no input needed.
    wb.set_decorations_window(false, true, false);
    wb.set_decorations_window(false, false, false);
    let back = later + Duration::from_secs(300);
    wb.set_decorations_window(true, true, false);
    assert_eq!(wb.decorations_deadline(back), None, "still waiting for the window to return");
    wb.window_returned(back, true);
    assert!(wb.decorations_deadline(back).is_some(), "back in front: they move");
    // Relax mode started by itself in the unfocused window, closed once back: moving again.
    // Its settings are the process's (another test's window would set them back).
    let _alone = crate::modules::hold_process();
    let relax = relax::settings::config();
    relax::settings::set_config(relax::settings::Config {
        idle_minutes: Some(5),
        ..relax::settings::Config::DEFAULT
    });
    wb.set_decorations_window(false, true, false);
    let idle = back + Duration::from_secs(301);
    assert!(wb.run_idle_actions(idle, true), "started unfocused");
    relax::settings::set_config(relax);
    scene.reset(Rgba::BLACK);
    wb.paint(&mut text, &mut scene);
    assert!(!wb.decorations_drawn());
    wb.set_decorations_window(true, true, false);
    wb.window_returned(idle + Duration::from_secs(60), true);
    wb.edit_key(&mut text, EditKey::Escape);
    let after = idle + Duration::from_secs(61);
    wb.decorations_input(after);
    scene.reset(Rgba::BLACK);
    let before = wb.chrome.decorations.clock.time();
    wb.paint(&mut text, &mut scene);
    assert!(wb.decorations_drawn(), "drawn again");
    assert!(wb.decorations_deadline(Instant::now()).is_some(), "asking frames again");
    std::thread::sleep(Duration::from_millis(30));
    scene.reset(Rgba::BLACK);
    wb.paint(&mut text, &mut scene);
    assert!(wb.chrome.decorations.clock.time() > before, "their clock goes on");
}
