use super::*;
use std::path::PathBuf;

fn project(window: u64, name: &str, number: u8, current: bool) -> ProjectWindow {
    ProjectWindow {
        window,
        name: name.into(),
        path: PathBuf::from(format!("/work/{name}")),
        number: Some(number),
        current,
    }
}

fn opened() -> SwitcherView {
    let mut view = SwitcherView::default();
    let projects = vec![
        project(10, "shop", 1, true),
        project(20, "blog", 2, false),
        project(30, "admin.acme.test", 3, false),
    ];
    view.set_data(&SwitcherData { projects });
    view
}

/// Opened: this window's chosen (where you are); → then ↵ brings the next to the front.
#[test]
fn it_opens_on_this_window_and_enter_switches_to_the_one_chosen() {
    let mut view = opened();
    assert_eq!(view.chosen().map(|p| p.window), Some(10), "this window's");
    view.edit_key(ViewerKey::Right);
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(view.take_action(), Some(ViewerAction::Request(AppRequest::FocusProject(20))));
}

/// Typed: by name, path or number; this window's chosen just closes the list.
#[test]
fn the_filter_keeps_by_name_path_or_number() {
    let mut view = opened();
    view.text_input("acme");
    assert_eq!(view.shown().len(), 1);
    assert_eq!(view.chosen().map(|p| p.window), Some(30));
    let mut view = opened();
    view.text_input("1");
    assert_eq!(view.chosen().map(|p| p.window), Some(10), "by its number");
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.take_action(), Some(ViewerAction::Close), "its own window: just closed");
    let mut view = opened();
    view.text_input("nothing");
    assert!(view.shown().is_empty() && view.chosen().is_none());
    view.edit_key(ViewerKey::Backspace);
    assert!(view.shown().is_empty(), "still `nothin`");
}

/// The arrows move over the grid (wrapping); Esc closes.
#[test]
fn the_arrows_choose_and_esc_closes() {
    let mut view = opened();
    view.edit_key(ViewerKey::Right);
    view.edit_key(ViewerKey::Right);
    assert_eq!(view.chosen().map(|p| p.window), Some(30));
    view.edit_key(ViewerKey::Right);
    assert_eq!(view.chosen().map(|p| p.window), Some(10), "wrapping");
    view.edit_key(ViewerKey::Escape);
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
}

/// Tab goes on as → (wrapping from the last to the first), ⇧Tab back as ←.
#[test]
fn tab_chooses_as_the_arrows_do() {
    let mut view = opened();
    let tab = |view: &mut SwitcherView, shift: bool| {
        assert!(view.edit_key_with(ViewerKey::Tab, KeyReach { word: false, extend: shift }));
        view.chosen().map(|p| p.window)
    };
    assert_eq!(tab(&mut view, false), Some(20));
    assert_eq!(tab(&mut view, false), Some(30));
    assert_eq!(tab(&mut view, false), Some(10), "wrapping");
    assert_eq!(tab(&mut view, true), Some(30), "⇧Tab: back");
    assert_eq!(tab(&mut view, true), Some(20));
    view.text_input("blog");
    assert_eq!(tab(&mut view, false), Some(20), "only the one shown");
}

/// A window's picture, once the app keeps one, is its card's (made once); a later one
/// replaces it, and the list hears so (drawn again).
#[test]
fn a_card_shows_its_windows_latest_picture() {
    // Windows of their own: the pictures are process-wide.
    let (here, there) = (7_101, 7_102);
    let mut view = SwitcherView::default();
    let projects = vec![project(here, "shop", 1, true), project(there, "blog", 2, false)];
    workbench_model::pictures::keep(there, 4, 2, vec![255; 32]);
    view.set_data(&SwitcherData { projects });
    assert!(view.picture(there).is_some_and(|i| (i.width, i.height) == (4, 2)));
    assert!(view.picture(here).is_none(), "none yet: its initials");
    assert!(!view.poll(), "nothing new");
    workbench_model::pictures::keep(here, 2, 2, vec![0; 16]);
    assert!(view.poll(), "this window's came");
    assert!(view.picture(here).is_some());
    workbench_model::pictures::keep_only(&[]);
}

/// A picture fits its card's area whole, centered.
#[test]
fn a_picture_fits_its_area_centered() {
    let area = Rect::new(10.0, 20.0, 200.0, 100.0);
    assert_eq!(fitted(area, 400, 100), Rect::new(10.0, 45.0, 200.0, 50.0));
    assert_eq!(fitted(area, 100, 100), Rect::new(60.0, 20.0, 100.0, 100.0));
}
