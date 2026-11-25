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
        physical: None,
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

/// A window's app without the window (its platform side): its workbench, dialogs included,
/// as a window has it.
pub(super) fn app() -> EditorApp {
    app_with(Vec::new(), None)
}

/// The same, its modules' reporters followed by `reporters`, sharing `shared` with another
/// window (or with its own).
pub(super) fn app_with(
    reporters: Vec<Arc<dyn workbench_model::ResourceReporter>>,
    shared: Option<Rc<Shared>>,
) -> EditorApp {
    let (mut support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    support.resource_reporters.extend(reporters);
    let shared = shared.unwrap_or_else(|| {
        let themes = Themes::load(&support.themes);
        let syntax = Arc::clone(&support.syntax);
        let fonts = std::thread::spawn(|| (TextSystem::new(), FontFamily::Monospace));
        Rc::new(Shared::new(themes, syntax, fonts, RenderOptions::default(), Instant::now()))
    });
    shared.windows.set(shared.windows.get() + 1);
    let mut wb = Workbench::new(Arc::new(support), crate::workbench::Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    EditorApp::new(wb, shared).unwrap()
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

/// The window closing (or the app quitting) asked while the mouse's button is down waits
/// for its release (gone at the press, the release would fall on what's under the window);
/// let go, it goes on; with the button up, it goes at once; anything else isn't held.
#[test]
fn a_closing_asked_with_the_button_down_waits_for_its_release() {
    let mut app = app();
    assert_eq!(app.after_release(Control::Close), Control::Close, "the button up: at once");
    app.pointer.held = true;
    assert_eq!(app.after_release(Control::Continue), Control::Continue);
    assert_eq!(app.after_release(Control::Close), Control::Continue, "held until let go");
    assert_eq!(app.pending.exit_request, None);
    app.button_released();
    assert!(!app.pointer.held);
    assert_eq!(app.pending.exit_request.take(), Some(Control::Close), "then it closes");
    app.pointer.held = true;
    assert_eq!(app.after_release(Control::Exit), Control::Continue);
    app.button_released();
    assert_eq!(app.pending.exit_request.take(), Some(Control::Exit));
    app.button_released();
    assert_eq!(app.pending.exit_request, None, "once");
}

/// What a command chosen in the drawer asks of the event loop (a window closed, the app
/// quitting) isn't lost: it's answered after the event. (A window without its renderer
/// answers every event with Close: the drawer's row stands for a Quit here.)
#[test]
fn a_drawer_command_s_closing_reaches_the_event_loop() {
    let mut app = app();
    assert!(app.frames.renderer.is_none());
    app.mouse_result(MouseResult::MenuCommand(cmd::QUIT.0.to_owned()));
    assert_eq!(app.pending.exit_request.take(), Some(Control::Close));
    app.exit_after(Control::Continue);
    assert_eq!(app.pending.exit_request, None, "anything else asks nothing");
}

/// The terminal's clipboard keys follow the system's terminals: Ctrl+C copies only what's
/// selected (else it's the program's interrupt), Ctrl+Shift+C / V and Shift+Insert always;
/// Ctrl+V pastes on Windows and stays the program's on Linux; macOS leaves them to ⌘.
#[test]
fn the_terminal_s_clipboard_keys_follow_the_system() {
    use super::chords::{KeysOf, TerminalClip, terminal_clip};
    let ctrl = |c: &str, shift: bool| KeyEvent {
        modifiers: platform_api::Modifiers { shift, ctrl: true, alt: false, meta: false },
        ..key(c, true, false, None)
    };
    let insert = KeyEvent {
        key: Key::Named(NamedKey::Insert),
        modifiers: platform_api::Modifiers { shift: true, ctrl: false, alt: false, meta: false },
        ..key("x", false, false, None)
    };
    let (copy, paste) = (Some(TerminalClip::Copy), Some(TerminalClip::Paste));
    for keys in [KeysOf::Windows, KeysOf::Linux] {
        assert_eq!(terminal_clip(&ctrl("c", false), || true, keys), copy, "{keys:?}");
        assert_eq!(terminal_clip(&ctrl("c", false), || false, keys), None, "{keys:?} interrupts");
        assert_eq!(terminal_clip(&ctrl("C", true), || false, keys), copy, "{keys:?}");
        assert_eq!(terminal_clip(&ctrl("V", true), || false, keys), paste, "{keys:?}");
        assert_eq!(terminal_clip(&insert, || false, keys), paste, "{keys:?}");
        assert_eq!(terminal_clip(&ctrl("d", false), || true, keys), None, "{keys:?}");
    }
    assert_eq!(terminal_clip(&ctrl("v", false), || false, KeysOf::Windows), paste);
    assert_eq!(terminal_clip(&ctrl("v", false), || false, KeysOf::Linux), None);
    assert_eq!(terminal_clip(&ctrl("c", false), || true, KeysOf::Mac), None);
    assert_eq!(terminal_clip(&ctrl("v", false), || false, KeysOf::Mac), None);
    let mut asked = 0;
    terminal_clip(
        &ctrl("v", false),
        || {
            asked += 1;
            true
        },
        KeysOf::Windows,
    );
    assert_eq!(asked, 0, "the selection is read only for Ctrl+C");
}
