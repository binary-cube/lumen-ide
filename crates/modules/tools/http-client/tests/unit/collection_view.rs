//! A collection's tab: its tree beside the editor of the request chosen.

use super::*;

#[test]
fn a_collection_s_tab_shows_its_tree_and_nothing_open_at_first() {
    let mut view = CollectionView::default();
    view.set_data(&CollectionData {
        queries: None,
        project: None,
        collection: "http/users".into(),
        select: None,
    });
    assert_eq!(view.tree.as_ref().map(RequestTree::within), Some("http/users"));
    assert!(view.editor.is_none(), "nothing opened by itself");
    // Opened again for another collection: its own tree.
    view.set_data(&CollectionData {
        queries: None,
        project: None,
        collection: "http/orders".into(),
        select: None,
    });
    assert_eq!(view.tree.as_ref().map(RequestTree::within), Some("http/orders"));
}

/// The tab takes the whole keyboard (the app's one-key shortcuts stay away): its editing keys
/// still reach a name typed in the tree — ⌫ erases, ↵ settles it, Esc gives it up.
#[test]
fn a_name_typed_in_the_tab_s_tree_takes_its_keys() {
    use ui_components::{KeyPress, PressedKey};
    let press = |key| KeyPress { key, shift: false, ctrl: false, alt: false };
    let mut view = CollectionView::default();
    view.set_data(&CollectionData {
        queries: None,
        project: None,
        collection: "http/users".into(),
        select: None,
    });
    view.tree.as_mut().unwrap().new_collection(None);
    assert!(view.takes_keyboard() && view.wants_text());
    assert!(view.text_input("a/bx"));
    assert!(view.key_press(press(PressedKey::Backspace)));
    let typed = |view: &CollectionView| {
        view.tree.as_ref().unwrap().naming.as_ref().map(|(_, i)| i.text().to_owned())
    };
    assert_eq!(typed(&view).as_deref(), Some("a/b"));
    assert!(view.key_press(press(PressedKey::Enter)));
    assert!(view.tree.as_ref().unwrap().naming_error, "↵ reached it: not a name, asked again");
    assert!(view.key_press(press(PressedKey::Escape)));
    assert!(typed(&view).is_none(), "Esc gave it up");
    assert!(!view.key_press(press(PressedKey::Char('x'))), "a character is typed text, not a key");
}

/// Its parts take the input: the keys are the tree's at first, a press on the editor's side
/// gives them to it (the tree's search stops), the separator is held and dragged (the keys
/// stay where they were), a press on the tree gives them back.
#[test]
fn its_sides_take_the_keys_and_the_separator_is_dragged() {
    let mut view = CollectionView::default();
    view.set_data(&CollectionData {
        queries: None,
        project: None,
        collection: "http/users".into(),
        select: None,
    });
    // As last drawn: the tree, the separator, the editor's side.
    view.width = 300.0;
    view.left = Rect::new(0.0, 0.0, 300.0, 400.0);
    view.right = Rect::new(303.0, 0.0, 500.0, 400.0);
    let mut splitter = Splitter::new(SplitterStyle::default(), Orientation::Vertical);
    splitter.set_bounds(Rect::new(297.0, 0.0, 6.0, 400.0), 1.0);
    view.splitter = Some(splitter);
    let mods = PressMods::default();
    assert!(view.text_input("us"), "typed into the tree's search");
    assert!(view.wants_text());
    view.pointer_down(Point::new(500.0, 200.0), 1, mods);
    view.release();
    assert!(view.editor_has_keys());
    assert!(!view.tree.as_ref().unwrap().wants_text(), "the tree's search stopped");
    let on_line = Point::new(300.0, 50.0);
    view.hover(on_line);
    assert_eq!(view.cursor_at(on_line), Some(ViewerCursor::ResizeHorizontal));
    view.pointer_down(on_line, 1, mods);
    assert!(view.drag(Point::new(360.0, 50.0)));
    view.release();
    assert!(view.width > 300.0, "the tree wider: {}", view.width);
    assert!(view.editor_has_keys(), "the separator left the keys where they were");
    view.pointer_down(Point::new(100.0, 300.0), 1, mods);
    view.release();
    assert!(!view.editor_has_keys());
}
