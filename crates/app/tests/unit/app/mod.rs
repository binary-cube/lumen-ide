use super::chords::{altgr_text, chord};
use super::*;

fn key(c: &str, ctrl: bool, alt: bool, text: Option<&str>) -> KeyEvent {
    KeyEvent {
        key: Key::Character(c.into()),
        pressed: true,
        repeat: false,
        modifiers: platform_api::Modifiers { shift: false, ctrl, alt, meta: false },
        text: text.map(str::to_owned),
        numpad: false,
    }
}

/// AltGr (Ctrl + Alt on Windows and Linux) types `{`, `@`, `ș`…: text, never a shortcut
/// (`{` was fold). A Ctrl + Alt shortcut types nothing: it stays one.
#[test]
fn what_altgr_types_is_text_not_a_shortcut() {
    let brace = key("{", true, true, Some("{"));
    let shortcut = key("c", true, true, None);
    assert_eq!(altgr_text(&brace), !MAC);
    assert_eq!(chord(&brace).is_none(), !MAC);
    assert!(!altgr_text(&shortcut));
    assert!(chord(&shortcut).is_some());
    assert!(!altgr_text(&key("s", false, false, Some("s"))), "plain typing isn't AltGr");
}

/// The keypad's + and − are keys of their own (Expand All / Collapse All), the main row's
/// + and − another (Zoom In / Out).
#[test]
fn the_keypads_plus_and_minus_are_their_own_keys() {
    let mut pad = key("+", false, false, Some("+"));
    pad.numpad = true;
    let main = key("+", false, false, Some("+"));
    let parsed = command_model::KeyChord::parse("NumpadAdd").unwrap();
    assert_eq!(chord(&pad).map(|c| c.key), Some(parsed.key));
    assert_eq!(chord(&main).map(|c| c.key), Some(command_model::ChordKey::Char('=')));
    let mut minus = key("-", false, false, Some("-"));
    minus.numpad = true;
    let parsed = command_model::KeyChord::parse("NumpadSubtract").unwrap();
    assert_eq!(chord(&minus).map(|c| c.key), Some(parsed.key));
}

/// A renderer that counts what it's asked: frames drawn, idle releases.
#[derive(Default)]
struct Counting {
    drawn: Rc<Cell<u32>>,
    released: Rc<Cell<u32>>,
}

impl Renderer for Counting {
    fn description(&self) -> String {
        "counting".into()
    }

    fn resize(&mut self, _size: PhysicalSize) {}

    fn render(
        &mut self,
        _scene: &Scene,
        _glyphs: &mut dyn render_api::GlyphRasterizer,
    ) -> Result<render_api::FrameStats, render_api::RenderError> {
        self.drawn.set(self.drawn.get() + 1);
        Ok(render_api::FrameStats::default())
    }

    fn release_idle(&mut self) {
        self.released.set(self.released.get() + 1);
    }
}

/// A window standing still: the frame asked for only so that the kept frame and the spare
/// image go back gives them back without drawing (drawing it would show a frame anew and
/// wait again, for ever). Anything else due, or something that happened, draws as before.
#[test]
fn a_standing_window_gives_back_its_frames_without_drawing() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let themes = Themes::load(&support.themes);
    let syntax = Arc::clone(&support.syntax);
    let fonts = std::thread::spawn(|| (TextSystem::new(), FontFamily::Monospace));
    let shared =
        Rc::new(Shared::new(themes, syntax, fonts, RenderOptions::default(), Instant::now()));
    shared.windows.set(1);
    let mut wb = Workbench::new(Arc::new(support), crate::workbench::Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let mut app = EditorApp::new(wb, shared).unwrap();
    let renderer = Counting::default();
    let (drawn, released) = (Rc::clone(&renderer.drawn), Rc::clone(&renderer.released));
    app.frames.renderer = Some(Box::new(renderer));
    app.startup.splash = splash::SplashClock::new(false);
    app.startup.first_frame_logged = true;
    let now = Instant::now();
    let standing = |app: &mut EditorApp| {
        app.frames.dirty = false;
        app.frames.kept_frame_check = Some(now - Duration::from_millis(1));
        app.frames.other_due = None;
    };
    standing(&mut app);
    assert!(app.release_without_frame(now));
    assert_eq!((released.get(), drawn.get()), (1, 0));
    assert_eq!(app.frames.kept_frame_check, None, "asked once");
    assert!(!app.release_without_frame(now), "nothing more to give back");
    // Something happened: a frame, as before.
    standing(&mut app);
    app.frames.dirty = true;
    assert!(!app.release_without_frame(now));
    // Something else due now (a caret's blink): drawn too.
    standing(&mut app);
    app.frames.other_due = Some(now);
    assert!(!app.release_without_frame(now));
    // Due later only: released now.
    app.frames.other_due = Some(now + Duration::from_secs(1));
    assert!(app.release_without_frame(now));
    assert_eq!(released.get(), 2);
}

/// A drawn frame's cost reaches a profile recording (what the GPU was asked to do, what
/// the scene held), and nothing while none records.
#[test]
fn a_frames_cost_is_recorded_only_while_recording() {
    let stats = render_api::FrameStats {
        quads: 40,
        glyphs: 300,
        draw_calls: 12,
        drawn_pixels: 1000,
        cached_glyphs: 90,
        ..render_api::FrameStats::default()
    };
    kernel::probe::start();
    for _ in 0..3 {
        super::frame::record_frame(&stats, 5);
    }
    let gathered = kernel::probe::take();
    kernel::probe::stop();
    let counted =
        |what: &str| gathered.counts.iter().find(|c| c.0 == "render" && c.1 == what).map(|c| c.2);
    assert_eq!(counted("draw calls"), Some(36));
    assert_eq!(counted("layers"), Some(15));
    assert_eq!(counted("glyphs"), Some(900));
    let level = gathered.levels.iter().find(|l| l.0 == "render" && l.1 == "glyphs cached");
    assert_eq!(level.map(|l| l.3), Some(90));
    super::frame::record_frame(&stats, 5);
    kernel::probe::start();
    let after = kernel::probe::take();
    kernel::probe::stop();
    assert!(after.counts.iter().all(|c| c.0 != "render"), "nothing kept while stopped");
}
