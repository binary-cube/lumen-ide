use super::*;

/// Typed text loses its control characters; the editing keys are the field's, ↵ / Tab
/// aren't; a cut takes only what's selected.
#[test]
fn it_keeps_a_dialog_fields_own_keys_text_and_cut() {
    let mut input = TextInput::new("");
    let mut field = DialogField { input: &mut input };
    assert!(field.text("ab\ncd"));
    assert_eq!(TextInput::text(field.input), "abcd");
    assert!(field.key(ViewerKey::Backspace, KeyReach::default()));
    assert!(!field.key(ViewerKey::Enter, KeyReach::default()));
    assert!(!field.key(ViewerKey::Tab, KeyReach::default()));
    assert!(!field.command("cut"), "nothing selected: nothing cut");
    assert!(field.command("select_all"));
    assert_eq!(field.selected_text().as_deref(), Some("abc"));
    assert!(field.command("cut"));
    assert_eq!(TextInput::text(field.input), "");
}
