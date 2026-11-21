use super::*;
use ui_components::search_field::SearchField;

fn hits() -> Vec<(Rect, Hit)> {
    vec![
        (Rect::new(0.0, 0.0, 200.0, 30.0), Hit::Search),
        // A found setting drawn over it.
        (Rect::new(170.0, 0.0, 30.0, 30.0), Hit::Found(0)),
        (Rect::new(0.0, 100.0, 20.0, 20.0), Hit::Handle(2)),
        (Rect::new(0.0, 200.0, 50.0, 20.0), Hit::Menu(1)),
        (Rect::new(0.0, 300.0, 200.0, 30.0), Hit::Name),
    ]
}

#[test]
fn a_field_is_hit_only_where_it_s_the_topmost() {
    let (hits, mut field, mut asked) = (hits(), SearchField::new(), None);
    let mut search = SearchPart { field: &mut field, hits: &hits, asked: &mut asked };
    assert!(search.hit(Point::new(10.0, 10.0)));
    assert!(!search.hit(Point::new(180.0, 10.0)), "what's above it");
    assert!(search.text("s"));
    assert_eq!(asked, Some(FieldAsk::Searched));
    let (mut input, mut asked) = (TextInput::new("caret"), None);
    let mut name = FieldPart { input: &mut input, own: Hit::Name, hits: &hits, asked: &mut asked };
    assert!(name.hit(Point::new(10.0, 310.0)));
    assert!(name.text("s"));
    assert!(name.key(ViewerKey::Enter, KeyReach::default()));
    assert_eq!(asked, Some(FieldAsk::Leave), "the name lets go on ↵");
}

#[test]
fn the_search_shows_its_first_result_on_enter_and_empties_on_escape() {
    let (hits, mut field, mut asked) = (hits(), SearchField::new(), None);
    field.set_text("caret");
    let mut search = SearchPart { field: &mut field, hits: &hits, asked: &mut asked };
    assert!(search.key(ViewerKey::Enter, KeyReach::default()));
    assert_eq!(*search.asked, Some(FieldAsk::Jump));
    assert!(search.key(ViewerKey::Escape, KeyReach::default()));
    assert_eq!(*search.asked, Some(FieldAsk::Leave));
    assert!(!search.key(ViewerKey::Down, KeyReach::default()), "not one of its keys");
    assert_eq!(field.text(), "");
}

/// The app's search field's editing: ⌘A selects it all, ⌘X cuts it (its results from the
/// top), ⌘Z brings it back, ⌥⌫ erases a word.
#[test]
fn the_search_edits_as_the_app_s_search_field() {
    let (hits, mut field, mut asked) = (hits(), SearchField::new(), None);
    let mut search = SearchPart { field: &mut field, hits: &hits, asked: &mut asked };
    assert!(search.text("caret trail"));
    *search.asked = None;
    assert!(search.command("select_all"));
    assert_eq!(*search.asked, None, "selecting changes nothing found");
    assert_eq!(search.selected_text().as_deref(), Some("caret trail"));
    assert!(search.command("cut"));
    assert_eq!(*search.asked, Some(FieldAsk::Searched));
    assert_eq!(SearchField::text(search.field), "");
    assert!(search.command("undo"));
    assert_eq!(SearchField::text(search.field), "caret trail");
    assert!(search.key(ViewerKey::End, KeyReach::default()));
    let word = KeyReach { word: true, extend: false };
    assert!(search.key(ViewerKey::Backspace, word));
    assert_eq!(SearchField::text(search.field), "caret ");
}

#[test]
fn the_open_gallery_catches_a_press_beside_it_only() {
    let mut hits = hits();
    hits.push((Rect::new(0.0, 400.0, 30.0, 30.0), Hit::Gallery(false)));
    let mut catch = Catch { hits: &hits, gallery_open: true };
    assert!(!catch.hit(Point::new(10.0, 410.0)), "on its ×");
    assert!(catch.hit(Point::new(10.0, 10.0)));
    assert_eq!(catch.press(Point::new(10.0, 10.0), 1), Press::Took);
    let closed = Catch { hits: &hits, gallery_open: false };
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
