use super::*;

use render_api::Scene;
use text_layout::TextSystem;

struct Pen {
    text: TextSystem,
    icons: icons::Icons,
    palette: ui_components::Palette,
    scene: Scene,
}

impl Pen {
    fn new() -> Self {
        Self {
            text: TextSystem::new(),
            icons: icons::Icons::builtin(),
            palette: ui_components::Palette::dark(),
            scene: Scene::default(),
        }
    }

    fn paint(&mut self, view: &mut RelaxView, now: Instant) {
        self.paint_focused(view, now, true);
    }

    fn paint_focused(&mut self, view: &mut RelaxView, now: Instant, focused: bool) {
        self.scene.reset(self.palette.window_background);
        let ctx = ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 2400.0, 1400.0),
            scale: 2.0,
            text: &mut self.text,
            scene: &mut self.scene,
            palette: &self.palette,
            icons: &self.icons,
            focused,
            waker: None,
        };
        view.paint_at(ctx, now);
    }
}

fn mid(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

fn config() -> Config {
    Config { motion: Motion::Full, ..Config::DEFAULT }
}

#[test]
fn esc_closes_it_and_other_keys_dont() {
    let mut view = RelaxView::at(config(), Instant::now());
    assert!(!view.edit_key(ViewerKey::Enter), "not its key");
    assert_eq!(view.take_action(), None);
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    assert_eq!(view.take_action(), None, "asked once");
}

/// The `esc` key is always there (moving the mouse never makes it come and go), in the top
/// right corner; its cap closes it, a click elsewhere does nothing.
#[test]
fn the_key_is_always_shown_and_its_cap_closes_it() {
    let start = Instant::now();
    let mut pen = Pen::new();
    let mut view = RelaxView::at(config(), start);
    pen.paint(&mut view, start + Duration::from_millis(500));
    let cap = view.esc;
    assert!(cap.size.width > 0.0 && cap.max_x() <= 2400.0 && cap.min_y() < 100.0);
    assert!(cap.min_x() > 2000.0, "{cap:?}");
    assert_eq!(view.cursor_at(mid(cap)), Some(ViewerCursor::Pointer));
    assert_eq!(view.click_at(Point::new(1200.0, 700.0)), None, "a stray click loses nothing");
    // Long after, the mouse still: still there.
    pen.paint(&mut view, start + Duration::from_secs(60));
    assert_eq!(view.esc, cap);
    assert_eq!(view.click_at(mid(cap)), Some(ViewerAction::Close));
}

#[test]
fn frames_come_at_the_pace_chosen_and_the_clock_never_jumps() {
    let start = Instant::now();
    let mut pen = Pen::new();
    let mut view = RelaxView::at(Config { fps: 60, ..config() }, start);
    pen.paint(&mut view, start);
    let next = view.next_frame().unwrap() - start;
    assert!(next > Duration::from_millis(12) && next < Duration::from_millis(17), "{next:?}");
    // 30 a second (the default).
    let mut slow = RelaxView::at(config(), start);
    pen.paint(&mut slow, start);
    assert!(slow.next_frame().unwrap() - start > Duration::from_millis(28));
    // The window in the background: 30 a second at most, the chosen rate back once focused.
    pen.paint_focused(&mut view, start, false);
    assert!(view.next_frame().unwrap() - start > Duration::from_millis(28));
    pen.paint(&mut view, start);
    assert!(view.next_frame().unwrap() - start < Duration::from_millis(17));
    // Back after a minute away (the window hidden): it goes on, a small step.
    pen.paint(&mut view, start + Duration::from_millis(16));
    let before = view.time();
    pen.paint(&mut view, start + Duration::from_secs(60));
    assert!(view.time() - before <= MAX_STEP.as_secs_f32() + 1e-4);
    // The pace multiplies the time; less motion slows it.
    let mut clock = SceneClock::default();
    clock.advance(start, 2.0);
    assert!((clock.advance(start + Duration::from_millis(50), 2.0) - 0.1).abs() < 1e-5);
    let reduced = RelaxView::at(Config { motion: Motion::Reduced, ..config() }, start);
    assert!(reduced.reduced() && reduced.frame_interval() >= Duration::from_millis(33));
}

#[test]
fn it_draws_its_scene_over_the_whole_window() {
    let start = Instant::now();
    let mut pen = Pen::new();
    let mut view = RelaxView::at(Config { clock: true, ..config() }, start);
    pen.paint(&mut view, start + Duration::from_secs(2));
    let layers = pen.scene.layers();
    // Its sky first, the whole window.
    let window = Rect::new(0.0, 0.0, 2400.0, 1400.0);
    assert_eq!(layers[0].images[0].bounds, window);
    assert!(layers.iter().map(|l| l.quads.len()).sum::<usize>() > 100);
    // The clock's digits and the key's label.
    assert!(layers.iter().map(|l| l.glyphs.len()).sum::<usize>() >= 5 + 3);
}

#[test]
fn the_arrows_change_the_scene_and_rotation_goes_through_them() {
    let start = Instant::now();
    let mut pen = Pen::new();
    let mut view = RelaxView::at(config(), start);
    pen.paint(&mut view, start);
    assert_eq!(view.scene_id(), "stars");
    view.edit_key(ViewerKey::Right);
    // Through the sky's color: the new one comes halfway.
    pen.paint(&mut view, Instant::now() + Duration::from_millis(100));
    assert_eq!(view.scene_id(), "stars");
    pen.paint(&mut view, Instant::now() + Duration::from_secs(1));
    assert_eq!(view.scene_id(), "waves");
    // Rotating every minute.
    let rotate = Config { scene: SceneChoice::Rotate, rotate_minutes: 1, ..config() };
    let mut view = RelaxView::at(rotate, start);
    pen.paint(&mut view, start + Duration::from_secs(30));
    assert_eq!(view.scene_id(), "stars");
    pen.paint(&mut view, start + Duration::from_secs(61));
    pen.paint(&mut view, start + Duration::from_secs(62));
    assert_eq!(view.scene_id(), "waves");
    // A fixed one: that one.
    let fixed = RelaxView::at(Config { scene: SceneChoice::Fixed("ribbon"), ..config() }, start);
    assert_eq!(fixed.scene_id(), "ribbon");
}

/// The whole view (`EDITOR_RELAX_SHOTS=<dir>`): the scene, the clock and the `esc` key.
#[test]
fn shot() {
    let Some(dir) = std::env::var_os("EDITOR_RELAX_SHOTS") else { return };
    let start = Instant::now();
    let mut pen = Pen::new();
    let mut view = RelaxView::at(
        Config { clock: true, scene: SceneChoice::Fixed("waves"), ..config() },
        start,
    );
    pen.paint(&mut view, start + Duration::from_secs(2));
    let size = render_api::PhysicalSize::new(2400, 1400);
    let mut renderer = render_software::SoftwareRenderer::new(size).unwrap();
    use render_api::Renderer;
    renderer.render(&pen.scene, &mut pen.text).unwrap();
    let path = std::path::PathBuf::from(dir).join("view.png");
    std::fs::write(path, renderer.encode_png().unwrap()).unwrap();
}

/// The widgets offered to calm screens are made with it, given what the app gave it, and
/// drawn along its bottom on glass — whoever offers them (it names none).
#[test]
fn the_widgets_offered_are_drawn_along_the_bottom() {
    use ui_components::screen_widgets::{
        self, ScreenWidget, WidgetContext, WidgetPlace, WidgetView,
    };

    #[derive(Debug)]
    struct Probe;
    struct ProbeView(bool);
    impl ScreenWidget for Probe {
        fn id(&self) -> &'static str {
            "test.probe"
        }
        fn place(&self) -> WidgetPlace {
            WidgetPlace::BottomRight
        }
        fn open(&self, ctx: &WidgetContext) -> Option<Box<dyn WidgetView>> {
            Some(Box::new(ProbeView(ctx.resources.is_some())))
        }
    }
    impl WidgetView for ProbeView {
        fn size(
            &mut self,
            _: &mut TextSystem,
            ds: &ui_components::ds::Ctx<'_>,
            _: f32,
        ) -> Option<(f32, f32)> {
            Some((ds.px(100.0), ds.px(50.0)))
        }
        fn paint(&mut self, p: screen_widgets::WidgetPaint<'_, '_>) {
            // Its color says whether it was given the app's resources.
            let color =
                if self.0 { kernel::Rgba::rgb(0x00ff00) } else { kernel::Rgba::rgb(0xff0000) };
            p.layer.quad(Quad::filled(p.rect, color));
        }
    }
    screen_widgets::register(std::sync::Arc::new(Probe));
    let start = Instant::now();
    let mut pen = Pen::new();
    let mut view = RelaxView::at(config(), start);
    view.set_data(&workbench_model::ResourceData(std::sync::Arc::new(NoResources)));
    pen.paint(&mut view, start + Duration::from_secs(3));
    screen_widgets::unregister("test.probe");
    let probe = pen
        .scene
        .layers()
        .iter()
        .flat_map(|l| &l.quads)
        .find(|q| q.color == kernel::Rgba::rgb(0x00ff00))
        .expect("drawn, given the resources");
    // 100 × 50 logical at scale 2, 28 from the bottom right corner of 2400 × 1400.
    assert_eq!(probe.bounds, Rect::new(2400.0 - 56.0 - 200.0, 1400.0 - 56.0 - 100.0, 200.0, 100.0));
}

#[derive(Debug)]
struct NoResources;

impl workbench_model::ResourceSource for NoResources {
    fn snapshot(&self) -> workbench_model::ResourceSnapshot {
        workbench_model::ResourceSnapshot::default()
    }
}
