use super::*;

fn buttons() -> Vec<(Rect, ())> {
    vec![(Rect::new(160.0, 0.0, 40.0, 40.0), ())]
}

#[test]
fn it_is_hit_where_no_button_is_over_it() {
    let (mut input, buttons) = (TextInput::default(), buttons());
    let field =
        ChatField { input: &mut input, area: Rect::new(0.0, 0.0, 200.0, 40.0), buttons: &buttons };
    assert!(field.hit(Point::new(10.0, 10.0)));
    assert!(!field.hit(Point::new(170.0, 10.0)), "the send button");
    assert_eq!(field.cursor(Point::new(10.0, 10.0)), Some(ViewerCursor::Text));
}

#[test]
fn enter_escape_and_tab_are_not_its_own_and_typed_controls_go() {
    let (mut input, buttons) = (TextInput::new("fix"), buttons());
    input.end(false);
    let mut field =
        ChatField { input: &mut input, area: Rect::new(0.0, 0.0, 200.0, 40.0), buttons: &buttons };
    let reach = KeyReach::default();
    assert!(!field.key(ViewerKey::Enter, reach) && !field.key(ViewerKey::Escape, reach));
    assert!(!field.key(ViewerKey::Tab, reach) && field.keeps_tab());
    assert!(field.text(" it\n\t"));
    assert!(!field.command("cut"), "nothing selected");
    assert!(field.key(ViewerKey::Backspace, KeyReach { word: true, extend: false }));
    assert_eq!(input.text(), "fix ");
}
