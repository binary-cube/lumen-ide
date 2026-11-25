//! A page of the app's own engine, end to end: the host started on a worker, the page
//! opened once connected, its frames imported on the reader thread into the live image the
//! app draws, its view told it's shown. Needs the built engine (`EDITOR_WEB_HOST`), else
//! skipped; ignored by default (it starts Chromium), and only on macOS (where it's built).
//! The engine coming and a host's end counted everywhere; the other pure parts beside the
//! files they test (`page`, `sink`, `ime`, `access`).

#[cfg(target_os = "macos")]
use std::cell::RefCell;
#[cfg(target_os = "macos")]
use std::rc::Rc;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use super::*;
#[cfg(target_os = "macos")]
use platform_api::{WebEvent, WebSource};

/// How long a page takes to answer a drag over it (a person's drag takes longer).
#[cfg(target_os = "macos")]
const DRAG_ANSWER: Duration = Duration::from_millis(200);

/// How long the engine may take to start and draw (a cold start).
#[cfg(target_os = "macos")]
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(30);

#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn a_page_of_the_engine_is_drawn_by_the_app() {
    if built_here().is_none() {
        eprintln!("EDITOR_WEB_HOST names no engine: skipped");
        return;
    }
    // The app's folders in a place of the test's own (never the user's).
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.clone());
    let Ok(mut gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(400, 200)) else {
        return;
    };
    let told: Rc<RefCell<Vec<WebEvent>>> = Rc::default();
    let into = Rc::clone(&told);
    let html = "<html><body style='margin:0;background:#0000ff'><p>page</p></body></html>";
    let source = WebSource::Html(html);
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 400.0, 200.0),
        scale: 1.0,
        inbox: Box::new(move |event| into.borrow_mut().push(event)),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    let started = Instant::now();
    while !told.borrow().contains(&WebEvent::Shown) {
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "no frame in time");
        std::thread::sleep(Duration::from_millis(20));
        platform_api::WebView::poll(&page);
    }
    eprintln!("first frame drawn {:?} after the page was made", started.elapsed());
    assert!(page.image().has_frame());
    let mut scene = render_api::Scene::default();
    scene.reset(kernel::Rgba::rgb(0x000000));
    scene.push_layer(None).image(page.image(), kernel::Rect::new(0.0, 0.0, 400.0, 200.0));
    // The page's blue, drawn by the app (its next frames may still be coming).
    let blue = loop {
        gpu.draw(&scene, &mut NoGlyphs, true);
        let (width, _, pixels) = gpu.pixels().expect("drawn");
        let at = (100 * width as usize + 200) * 4;
        let seen = [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]];
        if seen == [0, 0, 255, 255] || started.elapsed() > FIRST_FRAME_WAIT {
            break seen;
        }
        std::thread::sleep(Duration::from_millis(20));
        platform_api::WebView::poll(&page);
    };
    eprintln!(
        "blue drawn {:?} after the page was made (frame {})",
        started.elapsed(),
        page.image().generation()
    );
    assert_eq!(blue, [0, 0, 255, 255], "the page's blue, drawn by the app");
    drop(page);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "macos")]
struct NoGlyphs;

#[cfg(target_os = "macos")]
impl render_api::GlyphRasterizer for NoGlyphs {
    fn rasterize(&mut self, _key: render_api::GlyphKey) -> Option<render_api::RasterizedGlyph> {
        None
    }
}

/// An app's page and its view talk both ways: what it posts (`window.ipc.postMessage`)
/// reaches the view's inbox, what the view sends reaches the page's `__hostReceive`.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn an_app_page_and_its_view_talk_both_ways() {
    if built_here().is_none() {
        eprintln!("EDITOR_WEB_HOST names no engine: skipped");
        return;
    }
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.join("places"));
    let app = root.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let page = "<html><body><script>\
        window.__hostReceive = m => window.ipc.postMessage('got:' + m);\
        window.ipc.postMessage('hello');\
        </script></body></html>";
    std::fs::write(app.join("index.html"), page).unwrap();
    let Ok(gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(200, 100)) else {
        return;
    };
    let told: Rc<RefCell<Vec<WebEvent>>> = Rc::default();
    let into = Rc::clone(&told);
    let source = WebSource::App { root: &app, entry: "index.html" };
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 200.0, 100.0),
        scale: 1.0,
        inbox: Box::new(move |event| into.borrow_mut().push(event)),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    let started = Instant::now();
    let wait_for = |wanted: &str| {
        let wanted = WebEvent::Message(wanted.to_owned());
        while !told.borrow().contains(&wanted) {
            assert!(
                started.elapsed() < FIRST_FRAME_WAIT,
                "{wanted:?} never came: {:?}",
                told.borrow()
            );
            std::thread::sleep(Duration::from_millis(20));
            platform_api::WebView::poll(&page);
        }
    };
    wait_for("hello");
    eprintln!("the page spoke {:?} after it was made", started.elapsed());
    platform_api::WebView::send(&page, "ping");
    wait_for("got:ping");
    drop(page);
    let _ = std::fs::remove_dir_all(root);
}

/// draw.io through the engine follows the editor's theme as before: the module's page
/// around it, at the address the view makes for the theme (dark or light), configured with
/// the theme's colors once it asks; the page drawn by the app is dark for a dark theme,
/// light for a light one. Needs the draw.io package (`EDITOR_DRAWIO`: the module writes its
/// page into it, as into its install).
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST) with draw.io (EDITOR_DRAWIO)"]
fn drawio_follows_the_editors_theme() {
    use diagram_web::{page, theme};
    let Some(drawio) = std::env::var_os("EDITOR_DRAWIO").map(std::path::PathBuf::from) else {
        eprintln!("EDITOR_DRAWIO names no draw.io: skipped");
        return;
    };
    if built_here().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.join("places"));
    page::write(&drawio).expect("the module's page written");
    let dark_palette = ui_workbench::Palette::dark();
    let mut light_palette = ui_workbench::Palette::dark();
    light_palette.editor_background = kernel::Rgba::rgb(0xffffff);
    light_palette.text = kernel::Rgba::rgb(0x111111);
    let lightness = |palette: &ui_workbench::Palette| {
        let mut gpu = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(600, 400)).ok()?;
        let look = theme::Theme::from_palette(palette);
        let told: Rc<RefCell<Vec<WebEvent>>> = Rc::default();
        let into = Rc::clone(&told);
        let entry = page::entry("en", look.look());
        let source = WebSource::App { root: &drawio, entry: &entry };
        let page = EnginePage::new(Made {
            source: &source,
            area: kernel::Rect::new(0.0, 0.0, 600.0, 400.0),
            scale: 1.0,
            inbox: Box::new(move |event| into.borrow_mut().push(event)),
            wake: Arc::new(|| {}),
            importer: gpu.frame_importer(),
        });
        let config = serde_json::json!({"action": "configure", "config": theme::config(&look)});
        let started = Instant::now();
        let mut loaded = false;
        while !loaded {
            assert!(started.elapsed() < FIRST_FRAME_WAIT, "draw.io never started");
            std::thread::sleep(Duration::from_millis(20));
            platform_api::WebView::poll(&page);
            for event in told.borrow_mut().drain(..) {
                let WebEvent::Message(text) = event else { continue };
                if text.contains("\"configure\"") {
                    platform_api::WebView::send(&page, &config.to_string());
                } else if text.contains("\"init\"") {
                    let xml = "<mxfile><diagram><mxGraphModel><root><mxCell id=\"0\"/></root></mxGraphModel></diagram></mxfile>";
                    let load = serde_json::json!({"action": "load", "xml": xml, "autosave": 1});
                    platform_api::WebView::send(&page, &load.to_string());
                    loaded = true;
                }
            }
        }
        // Its frames after the load drawn in.
        std::thread::sleep(Duration::from_millis(1500));
        platform_api::WebView::poll(&page);
        let mut scene = render_api::Scene::default();
        scene.reset(kernel::Rgba::rgb(0x808080));
        scene.push_layer(None).image(page.image(), kernel::Rect::new(0.0, 0.0, 600.0, 400.0));
        gpu.draw(&scene, &mut NoGlyphs, true);
        let (_, _, pixels) = gpu.pixels()?;
        let sum: u64 =
            pixels.chunks(4).map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2])).sum();
        let mean = sum / (pixels.len() as u64 / 4 * 3);
        eprintln!("draw.io {}: lightness {mean}, ready in {:?}", look.look().1, started.elapsed());
        Some(mean)
    };
    let (Some(dark), Some(light)) = (lightness(&dark_palette), lightness(&light_palette)) else {
        return;
    };
    assert!(dark < 100 && light > 160, "dark {dark}, light {light}");
    let _ = std::fs::remove_dir_all(root);
}

/// The app's input reaches a page as the page's own events: a click where it was (in CSS
/// pixels, the window's scale taken out), a key with its name, typed text into a field, an
/// arrow, the wheel.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn the_apps_input_reaches_a_page() {
    use platform_api::{Key, KeyEvent, Modifiers, MouseButton, NamedKey, PageInput, PageMouse};
    if built_here().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.join("places"));
    let app = root.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let page = "<html><body style='height:3000px'><input id=f>\
        <script>var say = t => window.ipc.postMessage(t);\
        document.addEventListener('mousedown', e => say('down ' + e.clientX + ' ' + e.clientY + ' ' + e.detail));\
        document.addEventListener('keydown', e => say('key ' + e.key));\
        document.getElementById('f').addEventListener('input', e => say('typed ' + e.target.value));\
        window.addEventListener('scroll', () => say('scrolled ' + (window.scrollY > 0)));\
        say('ready');</script></body></html>";
    std::fs::write(app.join("index.html"), page).unwrap();
    let Ok(gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(400, 200)) else {
        return;
    };
    let told: Rc<RefCell<Vec<WebEvent>>> = Rc::default();
    let into = Rc::clone(&told);
    let source = WebSource::App { root: &app, entry: "index.html" };
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 400.0, 200.0),
        scale: 2.0,
        inbox: Box::new(move |event| into.borrow_mut().push(event)),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    let started = Instant::now();
    let wait_for = |wanted: &str| {
        let wanted = WebEvent::Message(wanted.to_owned());
        while !told.borrow().contains(&wanted) {
            assert!(
                started.elapsed() < FIRST_FRAME_WAIT,
                "{wanted:?} never came: {:?}",
                told.borrow()
            );
            std::thread::sleep(Duration::from_millis(20));
            platform_api::WebView::poll(&page);
        }
    };
    wait_for("ready");
    use platform_api::WebView as _;
    page.input(PageInput::Focus(true));
    // The field (at the page's top left) clicked: the page's pixels are half the window's.
    for action in [
        platform_api::PageMouseAction::Down(MouseButton::Left),
        platform_api::PageMouseAction::Up(MouseButton::Left),
    ] {
        let at = kernel::Point::new(40.0, 30.0);
        page.input(PageInput::Mouse(PageMouse {
            action,
            at,
            clicks: 1,
            modifiers: Modifiers::default(),
        }));
    }
    wait_for("down 20 15 1");
    let key = |key: Key, text: Option<&str>| KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
        text: text.map(str::to_owned),
        numpad: false,
        physical: None,
    };
    // Pressed and let go, as a keyboard does.
    let press = |pressed: Key, text: Option<&str>| {
        page.input(PageInput::Key(key(pressed.clone(), text)));
        page.input(PageInput::Key(KeyEvent { pressed: false, text: None, ..key(pressed, None) }));
    };
    press(Key::Character("a".into()), Some("a"));
    wait_for("key a");
    wait_for("typed a");
    page.input(PageInput::Text("ă".into()));
    wait_for("typed aă");
    press(Key::Named(NamedKey::ArrowLeft), None);
    wait_for("key ArrowLeft");
    // Its editing actions (the app's Edit menu): everything selected, then typed over; undone.
    page.input(PageInput::Text("z".into()));
    wait_for("typed ază");
    page.input(PageInput::Edit(platform_api::PageEdit::Undo));
    wait_for("typed aă");
    page.input(PageInput::Edit(platform_api::PageEdit::SelectAll));
    page.input(PageInput::Text("x".into()));
    wait_for("typed x");
    // An input method composing, then choosing: what it chose in the composition's place —
    // as winit tells it, the composition emptied right before its text comes.
    let compose = |text: &str, cursor| {
        PageInput::Compose(platform_api::PageCompose { text: text.into(), cursor })
    };
    page.input(compose("ni", Some((2, 2))));
    wait_for("typed xni");
    page.input(compose("", None));
    page.input(PageInput::Text("你".into()));
    wait_for("typed x你");
    // A right click on the field: its menu asked of the app (the field typed in).
    for action in [
        platform_api::PageMouseAction::Down(MouseButton::Right),
        platform_api::PageMouseAction::Up(MouseButton::Right),
    ] {
        let at = kernel::Point::new(40.0, 30.0);
        page.input(PageInput::Mouse(PageMouse {
            action,
            at,
            clicks: 1,
            modifiers: Modifiers::default(),
        }));
    }
    let asked = loop {
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "no menu asked");
        std::thread::sleep(Duration::from_millis(20));
        page.poll();
        if let Some(asked) = page.take_menu() {
            break asked;
        }
    };
    assert!(asked.editable, "{asked:?}");
    page.input(PageInput::Wheel(platform_api::PageWheel {
        at: kernel::Point::new(200.0, 150.0),
        delta: (0.0, -200.0),
        modifiers: Modifiers::default(),
    }));
    wait_for("scrolled true");
    eprintln!("input checked {:?} after the page was made", started.elapsed());
    drop(page);
    let _ = std::fs::remove_dir_all(root);
}

/// A `<select>` clicked opens its list as the page's popup: where it is, its own frames
/// (drawn by the app over the page); Esc closes it.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn a_selects_list_opens_as_the_pages_popup() {
    use platform_api::{
        Key, KeyEvent, Modifiers, MouseButton, NamedKey, PageInput, PageMouse, WebView as _,
    };
    if built_here().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.join("places"));
    let app = root.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let page = "<html><body style='margin:0'><select id=s style='font-size:20px;width:150px'>\
        <option>one</option><option>two</option><option>three</option></select>\
        <script>window.ipc.postMessage('ready');</script></body></html>";
    std::fs::write(app.join("index.html"), page).unwrap();
    let Ok(gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(400, 300)) else {
        return;
    };
    let told: Rc<RefCell<Vec<WebEvent>>> = Rc::default();
    let into = Rc::clone(&told);
    let source = WebSource::App { root: &app, entry: "index.html" };
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 400.0, 300.0),
        scale: 1.0,
        inbox: Box::new(move |event| into.borrow_mut().push(event)),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    let started = Instant::now();
    let wait_until = |what: &str, done: &dyn Fn() -> bool| {
        while !done() {
            assert!(started.elapsed() < FIRST_FRAME_WAIT, "{what} never happened");
            std::thread::sleep(Duration::from_millis(20));
            page.poll();
        }
    };
    wait_until("ready", &|| told.borrow().contains(&WebEvent::Message("ready".into())));
    page.input(PageInput::Focus(true));
    for action in [
        platform_api::PageMouseAction::Down(MouseButton::Left),
        platform_api::PageMouseAction::Up(MouseButton::Left),
    ] {
        let at = kernel::Point::new(40.0, 10.0);
        page.input(PageInput::Mouse(PageMouse {
            action,
            at,
            clicks: 1,
            modifiers: Modifiers::default(),
        }));
    }
    wait_until("the list open", &|| page.popup().is_some() && page.popup_image().has_frame());
    let popup = page.popup().unwrap();
    eprintln!("the list open at {popup:?}, {:?} after the page was made", started.elapsed());
    assert!(popup.size.width > 0.0 && popup.size.height > 0.0);
    let escape = KeyEvent {
        key: Key::Named(NamedKey::Escape),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
        text: None,
        numpad: false,
        physical: None,
    };
    page.input(PageInput::Key(escape.clone()));
    page.input(PageInput::Key(KeyEvent { pressed: false, ..escape }));
    wait_until("the list closed", &|| page.popup().is_none());
    drop(page);
    let _ = std::fs::remove_dir_all(root);
}

/// A file dragged from the system and dropped on an app's page reaches it as a drop of that
/// file (its name).
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn a_file_dropped_reaches_an_apps_page() {
    use platform_api::{PageDrag, PageInput, WebView as _};
    if built_here().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.join("places"));
    let app = root.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let dropped = root.join("picture.txt");
    std::fs::write(&dropped, "x").unwrap();
    let page = "<html><body style='margin:0;height:300px'><script>\
        document.addEventListener('dragover', e => e.preventDefault());\
        document.addEventListener('drop', e => { e.preventDefault();\
          window.ipc.postMessage('dropped ' + e.dataTransfer.files[0].name); });\
        window.ipc.postMessage('ready');</script></body></html>";
    std::fs::write(app.join("index.html"), page).unwrap();
    let Ok(gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(400, 300)) else {
        return;
    };
    let told: Rc<RefCell<Vec<WebEvent>>> = Rc::default();
    let into = Rc::clone(&told);
    let source = WebSource::App { root: &app, entry: "index.html" };
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 400.0, 300.0),
        scale: 1.0,
        inbox: Box::new(move |event| into.borrow_mut().push(event)),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    let started = Instant::now();
    let wait_for = |wanted: &str| {
        let wanted = WebEvent::Message(wanted.to_owned());
        while !told.borrow().contains(&wanted) {
            assert!(
                started.elapsed() < FIRST_FRAME_WAIT,
                "{wanted:?} never came: {:?}",
                told.borrow()
            );
            std::thread::sleep(Duration::from_millis(20));
            page.poll();
        }
    };
    wait_for("ready");
    assert!(page.takes_files());
    let at = kernel::Point::new(100.0, 100.0);
    // As a real drag: the mouse over the page first.
    page.input(PageInput::Focus(true));
    let mouse = platform_api::PageMouse {
        action: platform_api::PageMouseAction::Move,
        at,
        clicks: 1,
        modifiers: platform_api::Modifiers::default(),
    };
    page.input(PageInput::Mouse(mouse));
    page.input(PageInput::Drag(PageDrag::Enter { files: vec![dropped], at }));
    page.input(PageInput::Drag(PageDrag::Over { at }));
    // The page answers that it takes them (its `dragover`) before the hand lets go.
    std::thread::sleep(DRAG_ANSWER);
    page.input(PageInput::Drag(PageDrag::Over { at }));
    std::thread::sleep(DRAG_ANSWER);
    page.input(PageInput::Drag(PageDrag::Drop { at }));
    wait_for("dropped picture.txt");
    drop(page);
    let _ = std::fs::remove_dir_all(root);
}

/// With a screen reader, a page drawn by the engine tells its tree: its heading, its button,
/// where they are in it; without one, nothing.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn a_page_tells_its_accessibility_tree() {
    use platform_api::WebView as _;
    use platform_api::access::Role;
    if built_here().is_none() {
        return;
    }
    let Ok(gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(400, 300)) else {
        return;
    };
    let html = "<body><h2>Title</h2><button>Press</button></body>";
    let source = WebSource::Html(html);
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 400.0, 300.0),
        scale: 2.0,
        inbox: Box::new(|_| {}),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    assert!(page.access().is_none(), "nothing without a screen reader");
    page.set_access(true);
    let started = Instant::now();
    let nodes = loop {
        page.poll();
        let nodes = page.access().map(|a| a.nodes).unwrap_or_default();
        if nodes.iter().any(|n| n.role == Role::Button) {
            break nodes;
        }
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "no tree came: {nodes:?}");
        std::thread::sleep(Duration::from_millis(20));
    };
    let heading = nodes.iter().find(|n| n.role == Role::Heading).expect("the heading");
    assert_eq!(&*heading.name, "Title");
    assert_eq!(heading.level, Some(2));
    let button = nodes.iter().find(|n| n.role == Role::Button).expect("the button");
    assert_eq!(&*button.name, "Press");
    assert!(button.bounds.min_y() > heading.bounds.min_y(), "under the heading: {nodes:?}");
    assert!(button.bounds.size.width > 0.0 && button.bounds.max_x() <= 400.0);
    page.set_access(false);
    assert!(page.access().is_none());
}

/// A node moved by the layout alone (a block above it grew) is told where it is now.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn a_node_moved_by_the_layout_is_told_where_it_is() {
    use platform_api::WebView as _;
    use platform_api::access::Role;
    if built_here().is_none() {
        return;
    }
    let Ok(gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(400, 300)) else {
        return;
    };
    // An app's page: a response's runs no script.
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.join("places"));
    let app = root.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let html = "<body><div id=a style='height:10px'></div><button>Press</button>\
        <script>setTimeout(() => a.style.height = '100px', 500)</script></body>";
    std::fs::write(app.join("index.html"), html).unwrap();
    let source = WebSource::App { root: &app, entry: "index.html" };
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 400.0, 300.0),
        scale: 1.0,
        inbox: Box::new(|_| {}),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    page.set_access(true);
    let started = Instant::now();
    let button_y = || {
        page.poll();
        let nodes = page.access().map(|a| a.nodes).unwrap_or_default();
        nodes.iter().find(|n| n.role == Role::Button).map(|n| n.bounds.min_y())
    };
    let before = loop {
        if let Some(y) = button_y() {
            break y;
        }
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "no tree came");
        std::thread::sleep(Duration::from_millis(20));
    };
    loop {
        if button_y().is_some_and(|y| y >= before + 80.0) {
            break;
        }
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "the button never moved from {before}");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(page);
    let _ = std::fs::remove_dir_all(root);
}

/// Each window keeps the install's generation it saw: a second window still sees the engine
/// installed after the first moved its pages.
#[test]
fn each_window_sees_the_engine_come() {
    let (mut first, mut second) = (0, 0);
    assert!(newer(&mut first, 3));
    assert!(!newer(&mut first, 3), "seen");
    assert!(newer(&mut second, 3), "its own");
}

/// The engine comes once for a window: as it's installed and usable; another change of its
/// install while it stays installed (an update looked for, one failed) brings nothing; not
/// usable yet, it's told again at the next change.
#[test]
fn the_engine_comes_only_as_it_is_installed() {
    let mut seen = EngineSeen::default();
    assert!(!came(&mut seen, false, || true), "not there");
    assert!(!came(&mut seen, true, || false), "installed, not usable yet");
    assert!(came(&mut seen, true, || true), "installed and usable: it came");
    assert!(!came(&mut seen, true, || true), "an update looked for: nothing");
    assert!(!came(&mut seen, false, || true), "gone");
    assert!(came(&mut seen, true, || true), "back");
}

/// A host's end is counted once, however many of its pages see it; another host's is
/// counted again.
#[test]
fn a_hosts_end_is_counted_once_for_its_pages() {
    let (one, two) = (Arc::new(1_u8), Arc::new(2_u8));
    let mut last = None;
    assert!(first_of(&mut last, &Arc::downgrade(&one)));
    assert!(!first_of(&mut last, &Arc::downgrade(&one)), "its second page");
    assert!(!first_of(&mut last, &Arc::downgrade(&one)), "its third");
    assert!(first_of(&mut last, &Arc::downgrade(&two)), "another host");
}

/// A node in a transformed block (scaled twice, moved 50 px) is told where it shows.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "starts the built web engine (EDITOR_WEB_HOST)"]
fn a_transformed_node_is_told_where_it_shows() {
    use platform_api::WebView as _;
    use platform_api::access::Role;
    if built_here().is_none() {
        return;
    }
    let Ok(gpu) = render_wgpu::Offscreen::new(kernel::PhysicalSize::new(400, 300)) else {
        return;
    };
    let root = std::env::temp_dir().join(format!("web-engine-test-{}", uuid::Uuid::new_v4()));
    files::places::isolate(root.join("places"));
    let app = root.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let html = "<body style='margin:0'><div style='transform:scale(2);transform-origin:0 0;\
        margin-left:50px'><button style='width:50px'>Press</button></div></body>";
    std::fs::write(app.join("index.html"), html).unwrap();
    let source = WebSource::App { root: &app, entry: "index.html" };
    let page = EnginePage::new(Made {
        source: &source,
        area: kernel::Rect::new(0.0, 0.0, 400.0, 300.0),
        scale: 1.0,
        inbox: Box::new(|_| {}),
        wake: Arc::new(|| {}),
        importer: gpu.frame_importer(),
    });
    page.set_access(true);
    let started = Instant::now();
    let button = loop {
        page.poll();
        let nodes = page.access().map(|a| a.nodes).unwrap_or_default();
        if let Some(button) = nodes.iter().find(|n| n.role == Role::Button) {
            break button.bounds;
        }
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "no tree came");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!((button.min_x(), button.size.width), (50.0, 100.0), "{button:?}");
    drop(page);
    let _ = std::fs::remove_dir_all(root);
}
