use super::*;
use kernel::Rect;
use ui_components::control::Controls;

/// The secret's field with its eye drawn over its right end.
fn drawn() -> Vec<(Rect, Hit)> {
    vec![
        (Rect::new(0.0, 0.0, 200.0, 30.0), Hit::Field(Field::Secret)),
        (Rect::new(170.0, 0.0, 30.0, 30.0), Hit::Eye),
    ]
}

/// A field is pressed only where it's the topmost (never under the eye); with the focus
/// given from code it takes what's typed and its keys, while it's in the step; Tab never
/// stops at it.
#[test]
fn a_field_is_pressed_where_it_is_on_top_and_types_in_its_step() {
    let hits = drawn();
    let edited = Cell::new(None);
    let mut input = TextInput::default();
    {
        let mut field = FormField {
            field: Field::Secret,
            input: &mut input,
            hits: &hits,
            in_step: true,
            edited: &edited,
        };
        assert!(field.hit(Point::new(20.0, 10.0)));
        assert!(!field.hit(Point::new(180.0, 10.0)), "the eye's");
        assert_eq!(field.press(Point::new(20.0, 10.0), 3), Press::Took, "one click");
        let mut c = Controls::sticky();
        let mut parts: [&mut dyn Control; 1] = [&mut field];
        assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), None, "not Tab's");
        c.focus(&mut parts, Some(0));
        assert_eq!(c.text(&mut parts, "a\nb"), Some(0));
        assert_eq!(c.key(&mut parts, ViewerKey::Left, KeyReach::default()), Some(0));
        assert_eq!(c.command(&mut parts, "select_all"), Some(0));
        assert_eq!(c.command(&mut parts, "undo"), None, "only Select All");
    }
    assert_eq!((input.text(), edited.get()), ("ab", Some(Field::Secret)));
    let edited = Cell::new(None);
    let mut other = FormField {
        field: Field::Path,
        input: &mut input,
        hits: &hits,
        in_step: false,
        edited: &edited,
    };
    assert!(!other.text("x") && !other.key(ViewerKey::Backspace, KeyReach::default()));
    assert_eq!(edited.get(), None, "not in the step: nothing typed");
}
