use super::*;

fn hits() -> Vec<(Rect, Hit)> {
    vec![
        (Rect::new(0.0, 0.0, 200.0, 30.0), Hit::Search),
        // The search's ×, drawn over it.
        (Rect::new(170.0, 0.0, 30.0, 30.0), Hit::ClearSearch),
        (Rect::new(0.0, 100.0, 20.0, 20.0), Hit::Handle(2)),
        (Rect::new(0.0, 200.0, 50.0, 20.0), Hit::Menu(1)),
    ]
}

#[test]
fn a_field_is_hit_only_where_it_s_the_topmost() {
    let (hits, mut input, mut asked) = (hits(), TextInput::new("caret"), None);
    let mut field =
        FieldPart { input: &mut input, own: Hit::Search, hits: &hits, asked: &mut asked };
    assert!(field.hit(Point::new(10.0, 10.0)));
    assert!(!field.hit(Point::new(180.0, 10.0)), "its × is above it");
    assert!(field.text("s"));
    assert_eq!(asked, Some(FieldAsk::Searched));
}

#[test]
fn the_search_shows_its_first_result_on_enter_and_empties_on_escape() {
    let (hits, mut input, mut asked) = (hits(), TextInput::new("caret"), None);
    let mut field =
        FieldPart { input: &mut input, own: Hit::Search, hits: &hits, asked: &mut asked };
    assert!(field.key(ViewerKey::Enter, KeyReach::default()));
    assert_eq!(*field.asked, Some(FieldAsk::Jump));
    assert!(field.key(ViewerKey::Escape, KeyReach::default()));
    assert_eq!(*field.asked, Some(FieldAsk::Leave));
    assert!(!field.key(ViewerKey::Delete, KeyReach::default()), "not one of its keys");
    assert_eq!(input.text(), "");
}

#[test]
fn an_open_menu_catches_a_press_beside_it_only() {
    let (hits, mut menu_for) = (hits(), Some(1));
    let mut catch = Catch { hits: &hits, menu_for: &mut menu_for, gallery_open: false };
    assert!(!catch.hit(Point::new(10.0, 210.0)), "on its button");
    assert!(catch.hit(Point::new(10.0, 10.0)));
    assert_eq!(catch.press(Point::new(10.0, 10.0), 1), Press::Took);
    assert_eq!(menu_for, None);
    let closed = Catch { hits: &hits, menu_for: &mut menu_for, gallery_open: false };
    assert!(!closed.hit(Point::new(10.0, 10.0)), "nothing open: nothing caught");
    assert!(!closed.focusable());
}

#[test]
fn a_handle_holds_its_card_while_dragged() {
    let (hits, mut drag) = (hits(), None);
    let mut handle = HandlePart { hits: &hits, drag: &mut drag };
    assert_eq!(handle.press(Point::new(5.0, 105.0), 1), Press::Held);
    assert!(handle.drag(Point::new(5.0, 150.0)));
    assert!(drag.is_some_and(|d| d.layer == 2 && d.y == 150.0));
}
