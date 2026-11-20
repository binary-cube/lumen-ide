//! A modal's text field as a control: the editing keys, the text without control
//! characters, the edit commands, what changed said back; ⌦ at the end left to the view.

use super::*;
use ui_components::control::Controls;

fn field<'a>(input: &'a mut TextInput, edited: &'a mut bool) -> KeyField<'a> {
    KeyField { input, area: Rect::new(0.0, 0.0, 100.0, 20.0), selects: false, edited }
}

#[test]
fn it_takes_the_text_and_the_editing_keys_through_controls() {
    let mut input = TextInput::default();
    let mut edited = false;
    let mut controls = Controls::default();
    {
        let mut f = field(&mut input, &mut edited);
        let mut parts: [&mut dyn Control; 1] = [&mut f];
        controls.focus(&mut parts, Some(0));
        assert_eq!(controls.text(&mut parts, "ab\u{7}c"), Some(0));
        assert_eq!(controls.key(&mut parts, ViewerKey::Backspace, KeyReach::default()), Some(0));
        // ⌦ with nothing after the caret: not its own.
        assert_eq!(controls.key(&mut parts, ViewerKey::Delete, KeyReach::default()), None);
        assert_eq!(controls.key(&mut parts, ViewerKey::Enter, KeyReach::default()), None);
    }
    assert_eq!(input.text(), "ab");
    assert!(edited);
}

#[test]
fn a_press_puts_the_caret_and_holds_only_when_it_selects() {
    let mut input = TextInput::default();
    let mut edited = false;
    let mut f = field(&mut input, &mut edited);
    assert_eq!(f.press(Point::new(5.0, 5.0), 1), Press::Took);
    f.selects = true;
    assert_eq!(f.press(Point::new(5.0, 5.0), 1), Press::Held);
    assert_eq!(f.cursor(Point::new(5.0, 5.0)), Some(ViewerCursor::Text));
}

#[test]
fn its_commands_say_what_changed() {
    let mut input = TextInput::default();
    input.set_text("orders");
    input.end(false);
    let mut edited = false;
    let mut f = field(&mut input, &mut edited);
    assert!(!f.command("cut"));
    assert!(f.command("select_all"));
    assert_eq!(f.selected_text().as_deref(), Some("orders"));
    assert!(f.command("cut"));
    assert!(!f.command("paste"));
    assert_eq!(input.text(), "");
    assert!(edited);
}
