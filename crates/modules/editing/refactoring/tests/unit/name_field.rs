use ui_components::control::Controls;

use super::*;

/// A field 200 wide at the origin, "save" typed with the caret at its end.
fn input() -> TextInput {
    let mut input = TextInput::new("save");
    input.end(false);
    input
}

fn area() -> Option<Rect> {
    Some(Rect::new(0.0, 0.0, 200.0, 30.0))
}

#[test]
fn typed_text_loses_its_spaces_and_is_told() {
    let (mut input, mut edited) = (input(), false);
    let mut field = NameField { input: &mut input, area: area(), edited: &mut edited };
    assert!(field.text(" all \n"));
    assert!(edited);
    assert_eq!(input.text(), "saveall");
}

#[test]
fn keys_reach_one_character_and_never_tab() {
    let (mut input, mut edited) = (input(), false);
    let mut field = NameField { input: &mut input, area: area(), edited: &mut edited };
    // By word and extending asked: still one character, no selection.
    let reach = KeyReach { word: true, extend: true };
    assert!(field.key(ViewerKey::Left, reach));
    assert!(field.key(ViewerKey::Backspace, reach));
    assert!(!field.key(ViewerKey::Tab, reach), "Tab isn't the field's");
    assert!(!field.key(ViewerKey::Enter, reach), "Enter is the view's");
    assert!(edited);
    assert_eq!(input.text(), "sae");
}

#[test]
fn through_controls_it_keeps_the_focus_on_tab_and_takes_select_all() {
    let (mut input, mut edited) = (input(), false);
    let mut field = NameField { input: &mut input, area: area(), edited: &mut edited };
    let mut controls = Controls::default();
    let mut parts: [&mut dyn Control; 1] = [&mut field];
    controls.focus(&mut parts, Some(0));
    assert_eq!(controls.key(&mut parts, ViewerKey::Tab, KeyReach::default()), None);
    assert_eq!(controls.focused(), Some(0));
    assert_eq!(controls.command(&mut parts, "select_all"), Some(0));
    assert_eq!(controls.command(&mut parts, "copy"), None);
    assert_eq!(controls.press(&mut parts, Point::new(10.0, 10.0), 2), Some((0, Press::Took)));
}
