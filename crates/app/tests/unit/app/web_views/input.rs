//! The input of the pages the app draws: the mouse, the keys, a double click, the keyboard
//! given back.

use std::sync::Arc;
use std::time::{Duration, Instant};

use platform_api::WebView;
use ui_workbench::WebSource;

use super::super::tests::{Calls, Taking, page, taking_at};
use super::super::{MadePage, WebViews};
use super::*;

/// The input of a page the app draws: the mouse over it in its own pixels with the pointer
/// it asks; a press takes the keyboard and holds the mouse past its edge until let go; a
/// double click counted; keys and text to it while it has the keyboard; nothing while
/// something of the app's is over it; a click elsewhere gives the keyboard back.
#[test]
fn a_page_the_app_draws_gets_the_mouse_and_the_keys() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let made = Arc::clone(&calls);
    views.sync_at(vec![(page(1, &html, 10.0), true)], false, move |_: &WebSource, _, _| {
        let view: Box<dyn WebView> = Box::new(Taking(Arc::clone(&made)));
        Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
    });
    let at = |x: f32, y: f32| kernel::Point::new(x, y);
    let taken = || std::mem::take(&mut *calls.0.lock().unwrap());
    assert_eq!(
        views.pointer_moved(at(30.0, 5.0), true),
        Pointer::Page(platform_api::CursorIcon::Crosshair)
    );
    assert_eq!(taken(), ["Move 20 5 x1"], "in its own pixels");
    assert_eq!(views.pointer_moved(at(30.0, 5.0), false), Pointer::Window, "something over it");
    assert_eq!(taken(), ["Leave 20 5 x1"]);
    let left = platform_api::MouseButton::Left;
    assert!(views.pointer_down(left, at(30.0, 5.0), (true, false)));
    assert!(views.pointer_up(left, at(30.0, 5.0)));
    assert!(views.pointer_down(left, at(31.0, 5.0), (true, false)));
    assert_eq!(
        taken(),
        ["focus true", "Down(Left) 20 5 x1", "Up(Left) 20 5 x1", "Down(Left) 21 5 x2"]
    );
    // Held: a move past its edge is still its.
    assert!(matches!(views.pointer_moved(at(500.0, 5.0), true), Pointer::Page(_)));
    assert!(views.pointer_up(left, at(500.0, 5.0)));
    assert_eq!(taken(), ["Move 490 5 x1", "Up(Left) 490 5 x2"]);
    let key = platform_api::KeyEvent {
        key: platform_api::Key::Character("a".into()),
        pressed: true,
        repeat: false,
        modifiers: platform_api::Modifiers::default(),
        text: Some("a".into()),
        numpad: false,
        physical: None,
    };
    assert!(views.key(&key, true) && views.text("ă", true));
    assert!(!views.key(&key, false), "a window of the app's has the keyboard");
    assert!(views.wheel(at(30.0, 5.0), (0.0, -12.0), true));
    assert!(views.edit(platform_api::PageEdit::Copy, true));
    assert!(views.compose("ni", Some((2, 2)), true));
    assert_eq!(
        taken(),
        ["key Character(\"a\")", "text ă", "wheel 20 -12", "edit Copy", "compose ni"]
    );
    // A click on the window: the keyboard back.
    assert!(!views.pointer_down(left, at(500.0, 500.0), (true, false)));
    assert!(!views.key(&key, true));
    assert_eq!(taken(), ["released"]);
}

/// Two buttons held on a page: each let go reaches it, wherever the mouse is, and the page
/// holds the mouse until the last one (no button left stuck down in it).
#[test]
fn a_page_holds_the_mouse_until_every_button_is_let_go() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let made = Arc::clone(&calls);
    views.sync_at(vec![(page(1, &html, 10.0), true)], false, move |_: &WebSource, _, _| {
        let view: Box<dyn WebView> = Box::new(Taking(Arc::clone(&made)));
        Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
    });
    let at = |x: f32| kernel::Point::new(x, 5.0);
    let (left, right) = (platform_api::MouseButton::Left, platform_api::MouseButton::Right);
    assert!(views.pointer_down(left, at(30.0), (true, false)));
    assert!(views.pointer_down(right, at(500.0), (true, false)), "held: off it, still its");
    assert!(views.pointer_up(left, at(500.0)));
    assert!(matches!(views.pointer_moved(at(600.0), true), Pointer::Page(_)), "right still held");
    assert!(views.pointer_up(right, at(600.0)));
    assert!(!views.pointer_up(right, at(600.0)), "nothing held now");
    let calls = calls.0.lock().unwrap();
    let ups: Vec<&String> = calls.iter().filter(|c| c.starts_with("Up")).collect();
    assert_eq!(ups.len(), 2, "{calls:?}");
    assert!(ups[0].starts_with("Up(Left)") && ups[1].starts_with("Up(Right)"), "{calls:?}");
}

/// A page's own menu (the app's, over it): a click on its row keeps the keyboard with the
/// page, so the row's Copy reaches it once the menu closed; a click on the window itself
/// still gives the keyboard back.
#[test]
fn a_pages_menu_row_reaches_the_page() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    taking_at(&mut views, &calls, &[(1, 10.0)]);
    let at = |x: f32, y: f32| kernel::Point::new(x, y);
    let right = platform_api::MouseButton::Right;
    let left = platform_api::MouseButton::Left;
    assert!(views.pointer_down(right, at(30.0, 5.0), (true, false)));
    assert!(views.pointer_up(right, at(30.0, 5.0)));
    // The menu open over it: its row clicked.
    assert!(!views.pointer_down(left, at(40.0, 20.0), (false, true)));
    assert!(views.edit(platform_api::PageEdit::Copy, true), "the page still has the keyboard");
    let calls_now = std::mem::take(&mut *calls.0.lock().unwrap());
    assert!(!calls_now.contains(&"released".to_owned()), "{calls_now:?}");
    assert_eq!(calls_now.last().map(String::as_str), Some("edit Copy"));
    assert!(!views.pointer_down(left, at(500.0, 500.0), (true, false)));
    assert!(!views.edit(platform_api::PageEdit::Copy, true), "the window's click: given back");
}

/// A right click right after a left one is a click of its own, not the second of a double.
#[test]
fn another_button_starts_its_own_clicks() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    taking_at(&mut views, &calls, &[(1, 10.0)]);
    let at = kernel::Point::new(30.0, 5.0);
    let (left, right) = (platform_api::MouseButton::Left, platform_api::MouseButton::Right);
    assert!(views.pointer_down(left, at, (true, false)) && views.pointer_up(left, at));
    assert!(views.pointer_down(right, at, (true, false)));
    let calls = calls.0.lock().unwrap();
    assert_eq!(calls.last().map(String::as_str), Some("Down(Right) 20 5 x1"), "{calls:?}");
}

/// The window losing the focus mid-press: the page is told its buttons were let go, and
/// holds the mouse no more.
#[test]
fn a_press_held_ends_as_the_window_loses_the_focus() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    taking_at(&mut views, &calls, &[(1, 10.0)]);
    let left = platform_api::MouseButton::Left;
    assert!(views.pointer_down(left, kernel::Point::new(30.0, 5.0), (true, false)));
    views.window_left(kernel::Point::new(40.0, 5.0));
    assert_eq!(calls.0.lock().unwrap().last().map(String::as_str), Some("Up(Left) 30 5 x1"));
    assert!(!views.pointer_up(left, kernel::Point::new(40.0, 5.0)), "nothing held");
    let moved = views.pointer_moved(kernel::Point::new(500.0, 5.0), true);
    assert_eq!(moved, Pointer::Window, "off it: the window's again");
}

/// The keyboard given back only by the page that had it: the others the app draws aren't
/// told at each click.
#[test]
fn only_the_focused_page_gives_the_keyboard_back() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    taking_at(&mut views, &calls, &[(1, 10.0), (2, 200.0)]);
    let left = platform_api::MouseButton::Left;
    views.release_focus();
    assert!(calls.0.lock().unwrap().is_empty(), "none had it");
    assert!(views.pointer_down(left, kernel::Point::new(30.0, 5.0), (true, false)));
    assert!(views.pointer_up(left, kernel::Point::new(30.0, 5.0)));
    calls.0.lock().unwrap().clear();
    assert!(views.pointer_down(left, kernel::Point::new(220.0, 5.0), (true, false)));
    let told = calls.0.lock().unwrap();
    assert_eq!(told.iter().filter(|c| *c == "released").count(), 1, "the first only: {told:?}");
}

/// A screen reader's value for a page's field: its text chosen, then the value typed, to
/// the page with the keyboard only; none has it: not taken.
#[test]
fn a_screen_readers_value_replaces_a_pages_field() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let made = Arc::clone(&calls);
    views.sync_at(vec![(page(1, &html, 10.0), true)], false, move |_: &WebSource, _, _| {
        let view: Box<dyn WebView> = Box::new(Taking(Arc::clone(&made)));
        Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
    });
    assert!(!views.set_value("new", true), "no page has the keyboard");
    let at = kernel::Point::new(30.0, 5.0);
    let left = platform_api::MouseButton::Left;
    views.pointer_down(left, at, (true, false));
    views.pointer_up(left, at);
    calls.0.lock().unwrap().clear();
    assert!(views.set_value("new", true));
    assert_eq!(*calls.0.lock().unwrap(), ["edit SelectAll", "text new"]);
}

/// Presses of one button soon and near enough count 1, 2, 3, then 1 again; one too far, or
/// too late, or of another button, starts again.
#[test]
fn a_press_counts_with_the_ones_before_it_while_near_and_soon() {
    use platform_api::MouseButton::{Left, Right};
    let within = Duration::from_millis(400);
    let at = Instant::now();
    let p = kernel::Point::new(10.0, 10.0);
    let press = |clicks, dt: u64, p| LastPress {
        at: at + Duration::from_millis(dt),
        p,
        button: Left,
        clicks,
    };
    let soon = at + Duration::from_millis(100);
    assert_eq!(clicks_of(None, (Left, p), at, within), 1);
    assert_eq!(clicks_of(Some(press(1, 0, p)), (Left, p), soon, within), 2);
    assert_eq!(clicks_of(Some(press(2, 0, p)), (Left, p), soon, within), 3);
    assert_eq!(clicks_of(Some(press(3, 0, p)), (Left, p), soon, within), 1, "after three");
    let near = kernel::Point::new(10.0 + CLICK_SLOP - 1.0, 10.0);
    assert_eq!(clicks_of(Some(press(1, 0, p)), (Left, near), soon, within), 2, "near");
    let far = kernel::Point::new(10.0, 10.0 + CLICK_SLOP);
    assert_eq!(clicks_of(Some(press(1, 0, p)), (Left, far), soon, within), 1, "too far");
    let late = at + within;
    assert_eq!(clicks_of(Some(press(1, 0, p)), (Left, p), late, within), 1, "too late");
    assert_eq!(clicks_of(Some(press(1, 0, p)), (Right, p), soon, within), 1, "another button");
}

/// The wheel goes to the page under the mouse only, and to none while something of the
/// app's is over it there.
#[test]
fn the_wheel_goes_to_the_page_under_the_mouse_only() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    taking_at(&mut views, &calls, &[(1, 10.0), (2, 200.0)]);
    let taken = || std::mem::take(&mut *calls.0.lock().unwrap());
    assert!(views.wheel(kernel::Point::new(230.0, 5.0), (0.0, 3.0), true));
    assert_eq!(taken(), ["wheel 30 3"], "the second page's, in its pixels");
    assert!(!views.wheel(kernel::Point::new(30.0, 5.0), (0.0, 3.0), false), "something over it");
    assert!(!views.wheel(kernel::Point::new(150.0, 500.0), (0.0, 3.0), true), "over no page");
    assert!(taken().is_empty());
}

/// The mouse leaving the window mid-drag on a page isn't a leave for it: the press holds
/// it; without a press it is.
#[test]
fn a_page_holding_the_mouse_isnt_left_as_it_leaves_the_window() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    taking_at(&mut views, &calls, &[(1, 10.0)]);
    let at = kernel::Point::new(30.0, 5.0);
    let left = platform_api::MouseButton::Left;
    views.pointer_moved(at, true);
    assert!(views.pointer_down(left, at, (true, false)));
    calls.0.lock().unwrap().clear();
    views.pointer_left(kernel::Point::new(-5.0, 5.0));
    assert!(calls.0.lock().unwrap().is_empty(), "held: no leave");
    assert!(views.pointer_up(left, kernel::Point::new(-5.0, 5.0)));
    calls.0.lock().unwrap().clear();
    views.pointer_left(kernel::Point::new(-5.0, 5.0));
    assert_eq!(*calls.0.lock().unwrap(), ["Leave -15 5 x1"]);
}

/// The window losing the focus blurs the page with the keyboard, kept as the one to have
/// it: back in front, it has it again.
#[test]
fn the_page_with_the_keyboard_follows_the_windows_focus() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    taking_at(&mut views, &calls, &[(1, 10.0)]);
    let at = kernel::Point::new(30.0, 5.0);
    let left = platform_api::MouseButton::Left;
    views.window_back();
    assert!(calls.0.lock().unwrap().is_empty(), "none has the keyboard: nothing told");
    assert!(views.pointer_down(left, at, (true, false)) && views.pointer_up(left, at));
    calls.0.lock().unwrap().clear();
    views.window_left(at);
    assert_eq!(*calls.0.lock().unwrap(), ["focus false"]);
    assert!(views.has_keyboard(true), "kept as the one to have it");
    views.window_back();
    assert_eq!(calls.0.lock().unwrap().last().map(String::as_str), Some("focus true"));
}
