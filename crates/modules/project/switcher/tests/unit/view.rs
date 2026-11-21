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

/// The filter is the list's control with the keyboard from the start: what's typed and ⌫
/// reach it through the controls (the list filtered again), the arrows and Tab still choose
/// the cards, and Tab without its reach does nothing (as before).
#[test]
fn the_filter_has_the_keyboard_through_the_controls() {
    let mut view = opened();
    assert_eq!(view.controls.focused(), Some(FILTER));
    assert!(!view.text_input("\u{7}"), "a control character alone: nothing taken");
    view.text_input("blogx");
    assert!(view.shown().is_empty());
    assert!(view.edit_key(ViewerKey::Backspace));
    assert_eq!(view.chosen().map(|p| p.window), Some(20), "`blog` again");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("blog"));
    assert!(!view.edit_key(ViewerKey::Tab), "Tab is the cards' (with its reach)");
    assert_eq!(view.controls.focused(), Some(FILTER), "the focus stays in the filter");
    assert!(view.command("cut"), "the field's edit commands");
    assert_eq!(view.shown().len(), 3, "emptied: every window again");
    assert!(view.command("undo"));
    assert_eq!(view.chosen().map(|p| p.window), Some(20), "`blog` back");
}

/// The filter is the app's search field: drawn with its ×, a press on it empties it (the
/// list filtered again) and leaves the keys there; a press on a card doesn't take them.
#[test]
fn the_filter_is_the_apps_search_field() {
    let mut view = opened();
    let palette = ui_components::Palette::dark();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, WIDTH, 600.0);
    let mut paint = |view: &mut SwitcherView, scene: &mut render_api::Scene| {
        let (palette, icons, text) = (&palette, &icons, &mut text);
        view.paint(ViewerPaint {
            bounds,
            scale: 1.0,
            text,
            scene,
            palette,
            icons,
            focused: true,
            waker: None,
        });
    };
    view.text_input("shop");
    assert_eq!(view.shown().len(), 1);
    paint(&mut view, &mut scene);
    let field = view.filter.area();
    let clear = Point::new(field.max_x() - 17.0, field.min_y() + field.size.height / 2.0);
    assert_eq!(view.cursor_at(clear), None, "no hover yet");
    view.hover(clear);
    assert_eq!(view.cursor_at(clear), Some(ViewerCursor::Pointer), "× is a hand");
    assert!(view.click(clear).is_none());
    assert_eq!(view.filter.text(), "");
    assert_eq!(view.shown().len(), 3, "filtered again");
    assert_eq!(view.controls.focused(), Some(FILTER), "the keys kept");
    paint(&mut view, &mut scene);
    let (card, _) = view.cards[1];
    view.click(Point::new(card.min_x() + 4.0, card.min_y() + 4.0));
    assert_eq!(view.controls.focused(), Some(FILTER), "a card leaves them in the filter");
}

/// Only a change of the filter's text is an edit (the field's own `take_edited`): ⌫ on an
/// empty filter or ⌘A isn't; typing and ⌘X are.
#[test]
fn the_filter_tells_only_its_text_changed() {
    let mut filter = SearchField::new();
    let mut edited = false;
    let mut part = FilterPart { filter: &mut filter, edited: &mut edited };
    assert!(part.key(ViewerKey::Backspace, KeyReach::default()));
    assert!(part.command("select_all"));
    assert!(!*part.edited, "nothing changed");
    assert!(part.text("ab"));
    assert!(*part.edited);
    *part.edited = false;
    assert!(part.command("select_all") && part.command("cut"));
    assert!(*part.edited, "⌘X: an edit");
}
