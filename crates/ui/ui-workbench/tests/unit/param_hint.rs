use super::*;

#[test]
fn keeps_the_call_and_follows_the_argument() {
    let card = ParamHintCard { code: "f(a, b)".into(), params: vec![2..3, 5..6], active: Some(0) };
    let mut hint = ParamHint::default();
    hint.show(4, card);
    assert_eq!(hint.open_at(), Some(4));
    assert!(hint.set_active(Some(1)));
    assert!(!hint.set_active(Some(1)), "no change");
    assert!(hint.hide() && !hint.is_open());
}
