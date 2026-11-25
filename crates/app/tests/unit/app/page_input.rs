//! Who a key goes to while a page the app draws has the keyboard (and where its release
//! goes), the wheel's lines as a page counts them, a page's menu, files dragged over a page,
//! what floats over the pages taking the mouse, the keyboard given back as the app moves
//! its focus.

use std::sync::Mutex;

use platform_api::WebView;
use workbench_model::{Notification, WorkbenchHost as _};

use super::*;
use crate::app::tests::app;

fn key(key: platform_api::Key, modifiers: platform_api::Modifiers) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers,
        text: None,
        numpad: false,
        physical: None,
    }
}

fn primary() -> platform_api::Modifiers {
    match MAC {
        true => platform_api::Modifiers { meta: true, ..Default::default() },
        false => platform_api::Modifiers { ctrl: true, ..Default::default() },
    }
}

/// Who `key` goes to, `ids` the commands it's bound to ([`EditorApp::page_key_owner`]'s
/// steps).
fn owner(key: &KeyEvent, ids: &[command_model::CommandId]) -> PageKey {
    if !may_be_the_apps(key) {
        return PageKey::Page;
    }
    page_key(ids)
}

/// ⌘C / Ctrl+C is the page's copy; a shortcut of the app's (with ⌘ / Ctrl, or a function
/// key) the app's; letters, arrows, Esc and keys bound to nothing the page's.
#[test]
fn a_pages_keys_its_editing_and_the_apps_shortcuts() {
    use platform_api::{Key, NamedKey};
    let c = key(Key::Character("c".into()), primary());
    assert_eq!(owner(&c, &[cmd::COPY]), PageKey::Edit(platform_api::PageEdit::Copy));
    assert_eq!(
        owner(&key(Key::Character("a".into()), primary()), &[cmd::SELECT_ALL]),
        PageKey::Edit(platform_api::PageEdit::SelectAll)
    );
    // An app's shortcut: ⌘ / Ctrl held, or a function key.
    assert_eq!(owner(&key(Key::Character("s".into()), primary()), &[cmd::SAVE]), PageKey::App);
    let f5 = key(Key::Named(NamedKey::F(5)), Default::default());
    assert_eq!(owner(&f5, &[cmd::SAVE]), PageKey::App);
    // ⌘ / Ctrl with a key the app binds nothing to: the page's.
    assert_eq!(owner(&key(Key::Character("k".into()), primary()), &[]), PageKey::Page);
    // Plain keys, even bound to something of the app's: the page's.
    let a = key(Key::Character("a".into()), Default::default());
    assert_eq!(owner(&a, &[cmd::SAVE]), PageKey::Page);
    let escape = key(Key::Named(NamedKey::Escape), Default::default());
    assert_eq!(owner(&escape, &[cmd::SAVE]), PageKey::Page);
}

/// The Edit menu's commands are the page's editing actions; others aren't.
#[test]
fn the_edit_commands_are_the_pages_editing() {
    assert_eq!(page_edit(cmd::UNDO.0), Some(platform_api::PageEdit::Undo));
    assert_eq!(page_edit(cmd::PASTE.0), Some(platform_api::PageEdit::Paste));
    assert_eq!(page_edit(cmd::SAVE.0), None);
}

/// A wheel's notch scrolls a page as far as the editor: [`super::events::WHEEL_LINES`] of
/// its lines (physical pixels); pixels stay as they are.
#[test]
fn a_wheels_notch_scrolls_a_page_as_far_as_the_editor() {
    let lines = ScrollDelta::Lines { x: 0.5, y: -1.0 };
    let line = 20.0;
    let notch = super::events::WHEEL_LINES * line;
    assert_eq!(page_wheel(lines, line), (0.5 * notch, -notch));
    assert_eq!(page_wheel(ScrollDelta::Pixels { x: 3.0, y: 4.0 }, line), (3.0, 4.0));
}

/// The editor's own commands (they need a text file) never take a key from a page: ⌘D,
/// ⌘B, ⌘L are the page's; the keys a field edits with (Backspace, Delete, the arrows, Home,
/// End) are the page's whatever is held and whatever they're bound to.
#[test]
fn a_pages_fields_keep_their_editing_keys() {
    use platform_api::{Key, NamedKey};
    let d = key(Key::Character("d".into()), primary());
    assert_eq!(owner(&d, &[cmd::DUPLICATE]), PageKey::Page);
    let l = key(Key::Character("l".into()), primary());
    assert_eq!(owner(&l, &[cmd::GOTO_LINE]), PageKey::Page);
    // Bound to the editor's and to something else too: the app's.
    assert_eq!(owner(&d, &[cmd::DUPLICATE, cmd::SAVE]), PageKey::App);
    for named in [
        NamedKey::Backspace,
        NamedKey::Delete,
        NamedKey::ArrowLeft,
        NamedKey::ArrowRight,
        NamedKey::ArrowUp,
        NamedKey::ArrowDown,
        NamedKey::Home,
        NamedKey::End,
    ] {
        let pressed = key(Key::Named(named), primary());
        assert_eq!(owner(&pressed, &[cmd::DELETE_LINE, cmd::SAVE]), PageKey::Page, "{named:?}");
        assert!(!may_be_the_apps(&pressed), "{named:?}: nothing looked up");
    }
    assert!(!may_be_the_apps(&key(Key::Character("a".into()), Default::default())));
}

/// A page's menu: Cut only where it's editable with a selection, Copy only with a
/// selection, Paste only where it's editable, Select All always.
#[test]
fn a_pages_menu_offers_what_is_under_the_click() {
    let ask = |editable, selection| platform_api::PageMenuAsk {
        at: kernel::Point::new(0.0, 0.0),
        editable,
        selection,
    };
    assert_eq!(
        page_menu_rows(&ask(true, true)),
        [cmd::CUT, cmd::COPY, cmd::PASTE, cmd::SELECT_ALL]
    );
    assert_eq!(page_menu_rows(&ask(true, false)), [cmd::PASTE, cmd::SELECT_ALL]);
    assert_eq!(page_menu_rows(&ask(false, true)), [cmd::COPY, cmd::SELECT_ALL]);
    assert_eq!(page_menu_rows(&ask(false, false)), [cmd::SELECT_ALL]);
}

/// A page the app draws that records what it's handed, takes the input and files.
#[derive(Debug)]
struct Recording(Arc<Mutex<Vec<String>>>);

impl WebView for Recording {
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, _: bool) {}
    fn load_html(&self, _: &str) {}
    fn takes_input(&self) -> bool {
        true
    }
    fn takes_files(&self) -> bool {
        true
    }
    fn input(&self, input: platform_api::PageInput) {
        let said = match input {
            platform_api::PageInput::Key(k) => format!("key {:?} {}", k.key, k.pressed),
            platform_api::PageInput::Focus(f) => format!("focus {f}"),
            platform_api::PageInput::Edit(e) => format!("edit {e:?}"),
            platform_api::PageInput::Drag(d) => match d {
                platform_api::PageDrag::Enter { .. } => "enter".into(),
                platform_api::PageDrag::Over { .. } => "over".into(),
                platform_api::PageDrag::Leave => "leave".into(),
                platform_api::PageDrag::Drop { .. } => "drop".into(),
            },
            _ => return,
        };
        self.0.lock().unwrap().push(said);
    }
    fn release_focus(&self) {
        self.0.lock().unwrap().push("released".into());
    }
}

/// A window's app with a page the app draws over `area`, what it's handed recorded.
fn with_page(area: kernel::Rect) -> (EditorApp, Arc<Mutex<Vec<String>>>) {
    let (app, calls, _) = with_page_image(area);
    (app, calls)
}

/// The same, with the image the app draws for the page.
fn with_page_image(
    area: kernel::Rect,
) -> (EditorApp, Arc<Mutex<Vec<String>>>, render_api::ImageData) {
    let mut app = app();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let made = Arc::clone(&calls);
    let html: Arc<str> = "<p>a</p>".into();
    let page = ui_workbench::WebPage { id: 1, source: ui_workbench::WebSource::Html(html), area };
    let image = render_api::ImageData::live();
    let drawn = image.clone();
    app.web_views.sync(vec![(page, true)], false, Instant::now(), move |_, _, _| {
        let view: Box<dyn WebView> = Box::new(Recording(Arc::clone(&made)));
        Some(web_views::MadePage { view, drawn: Some(drawn.clone()), popup: None })
    });
    (app, calls, image)
}

/// The page given the keyboard by a press on it, as the window's event does.
fn pressed_on(app: &mut EditorApp, p: Point) {
    app.on_mouse_down(MouseButton::Left, p, Default::default());
    app.on_mouse_up(MouseButton::Left, p);
}

/// Files dragged over a page that takes them are its (the editor shows no drop); off it,
/// the editor's drop follows them.
#[test]
fn files_dragged_go_to_the_page_or_the_editor() {
    let (mut app, calls) = with_page(kernel::Rect::new(0.0, 0.0, 300.0, 300.0));
    app.web_views.files_entered(&[std::path::PathBuf::from("/a.png")]);
    app.files_moved(Point::new(50.0, 50.0));
    assert!(!app.workbench.file_drag_active(), "the page's");
    assert_eq!(*calls.lock().unwrap(), ["enter"]);
    app.files_moved(Point::new(900.0, 500.0));
    assert!(app.workbench.file_drag_active(), "off it: the editor's");
    assert_eq!(calls.lock().unwrap().last().map(String::as_str), Some("leave"));
    app.files_moved(Point::new(60.0, 60.0));
    assert!(!app.workbench.file_drag_active(), "back over it: the editor's drop let go");
}

/// The pages get the mouse where nothing of the app's floats over them: a notification
/// over a page takes the mouse there.
#[test]
fn a_notification_over_a_page_takes_the_mouse() {
    let (mut app, _calls) = with_page(kernel::Rect::new(0.0, 0.0, 1200.0, 800.0));
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    assert!(app.pages_open_at(Point::new(600.0, 400.0)), "nothing over it");
    app.workbench.notify(Notification::info("Copied"));
    for _ in 0..20 {
        app.workbench.paint(&mut text, &mut scene);
        std::thread::sleep(Duration::from_millis(20));
    }
    let grid =
        (0..60).flat_map(|x| (0..40).map(move |y| Point::new(x as f32 * 20.0, y as f32 * 20.0)));
    let over = grid.clone().find(|p| app.workbench.over_toast(*p)).expect("the card drawn");
    assert!(!app.pages_open_at(over), "the notification's");
    let off = grid.into_iter().find(|p| !app.workbench.over_toast(*p)).unwrap();
    assert!(app.pages_open_at(off), "beside it: the page's");
}

/// A page has the keyboard after a press on it, and gives it back as soon as the app moves
/// its focus another way than by a click (a command, a tab opened): the next key is the
/// app's.
#[test]
fn a_page_gives_the_keyboard_back_as_the_app_moves_its_focus() {
    use platform_api::Key;
    let (mut app, calls) = with_page(kernel::Rect::new(0.0, 0.0, 300.0, 300.0));
    pressed_on(&mut app, Point::new(50.0, 50.0));
    let a = key(Key::Character("a".into()), Default::default());
    assert!(app.key_to_page(&a), "the page's");
    calls.lock().unwrap().clear();
    // The app's focus moved by a command (a new file: its tab opened, the keys its).
    app.workbench.new_file();
    app.pages_follow_focus();
    assert_eq!(*calls.lock().unwrap(), ["released"]);
    assert!(!app.key_to_page(&a), "the app's now");
    // Nothing moved since: nothing given back again.
    calls.lock().unwrap().clear();
    app.pages_follow_focus();
    assert!(calls.lock().unwrap().is_empty());
}

/// A key's release goes where its press went: an editing action's is nothing, an app's
/// shortcut's the app's — even once the modifier is let go first (the key alone then looks
/// like the page's).
#[test]
fn a_keys_release_goes_where_its_press_went() {
    use platform_api::Key;
    let (mut app, calls) = with_page(kernel::Rect::new(0.0, 0.0, 300.0, 300.0));
    pressed_on(&mut app, Point::new(50.0, 50.0));
    calls.lock().unwrap().clear();
    let copy = KeyEvent { physical: Some('c'), ..key(Key::Character("c".into()), primary()) };
    assert!(app.key_to_page(&copy));
    assert_eq!(*calls.lock().unwrap(), ["edit Copy"]);
    // ⌘ let go before C: the release alone is still the copy's (nothing to the page).
    let released = KeyEvent { pressed: false, modifiers: Default::default(), ..copy };
    assert!(app.key_to_page(&released));
    assert_eq!(*calls.lock().unwrap(), ["edit Copy"], "no lone key-up in the page");
    // A plain key's release is the page's as its press.
    let a = KeyEvent { physical: Some('a'), ..key(Key::Character("a".into()), Default::default()) };
    assert!(app.key_to_page(&a));
    assert!(app.key_to_page(&KeyEvent { pressed: false, ..a }));
    let told = calls.lock().unwrap();
    assert_eq!(told[1..], ["key Character(\"a\") true", "key Character(\"a\") false"]);
}

/// Only a new frame of a page the app draws, with nothing else new: the wake draws the
/// page again where it is (the scene kept), never a whole frame.
#[test]
fn a_pages_new_frame_alone_draws_only_the_pages() {
    let area = kernel::Rect::new(0.0, 0.0, 300.0, 300.0);
    let (mut app, _calls, image) = with_page_image(area);
    let copied = || render_api::CopiedFrame { size: (1, 1), changed: None, bytes: 0 };
    let html: Arc<str> = "<p>a</p>".into();
    let page = || ui_workbench::WebPage {
        id: 1,
        source: ui_workbench::WebSource::Html(Arc::clone(&html)),
        area,
    };
    // Its first frame shows it: a whole frame.
    image.frame_copied(copied());
    app.on_wake();
    app.web_views
        .sync(vec![(page(), true)], false, Instant::now(), |_, _, _| None::<web_views::MadePage>);
    // Whatever the first wake gathered of the workbench's is taken by now.
    app.on_wake();
    app.frames.dirty = false;
    app.frames.only_pages = false;
    image.frame_copied(copied());
    app.on_wake();
    assert!(app.frames.only_pages && !app.frames.dirty, "only the pages drawn again");
}
