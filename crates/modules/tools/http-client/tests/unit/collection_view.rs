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
    view.side = Side::Tree;
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
