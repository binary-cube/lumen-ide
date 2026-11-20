use super::*;

/// The files' separator: one press holds it and the pointer moves it; two put it back.
#[test]
fn the_files_separator_is_dragged_or_put_back() {
    let mut splitter = Splitter::new(
        ui_components::SplitterStyle::default(),
        ui_components::Orientation::Horizontal,
    );
    splitter.set_bounds(kernel::Rect::new(0.0, 100.0, 300.0, 8.0), 1.0);
    let (mut at, mut reset) = (None, false);
    let mut seam = FilesSeam { splitter: &mut splitter, at: &mut at, reset: &mut reset };
    let on = Point::new(50.0, 104.0);
    assert!(seam.hit(on));
    assert_eq!(seam.press(on, 2), Press::Took);
    assert_eq!(seam.press(on, 1), Press::Held);
    assert!(seam.drag(Point::new(50.0, 160.0)));
    seam.release();
    assert!(reset && at.is_some_and(|y| y > 104.0), "{at:?}");
}

/// The bar's field: it takes the keys only while typed in and the Date picker is closed;
/// the text while the files' search has it is left to them; Esc leaves it.
#[test]
fn the_bar_s_field_takes_the_keys_while_typed_in() {
    let (mut text, mut branches) = (TextInput::default(), TextInput::default());
    let (mut focus, mut popup, mut answer) = (None, None, None);
    let mut field = BarField {
        focus: &mut focus,
        text: &mut text,
        branches: &mut branches,
        popup: &mut popup,
        picker_open: false,
        text_elsewhere: false,
        answer: &mut answer,
    };
    assert!(!field.captures_keys(), "not typed in");
    *field.focus = Some(Field::Text);
    assert!(field.captures_keys());
    field.picker_open = true;
    assert!(!field.captures_keys(), "the picker open has the keys");
    field.picker_open = false;
    assert!(field.text("ab"));
    assert_eq!(*field.answer, Some(FieldAnswer::Typed(Field::Text)));
    field.text_elsewhere = true;
    assert!(!field.text("c"), "the files' search has the text");
    assert!(field.key(ViewerKey::Tab, KeyReach::default()), "every key is kept");
    assert!(field.key(ViewerKey::Escape, KeyReach::default()));
    assert_eq!(focus, None, "Esc: left");
    assert_eq!(text.text(), "ab");
}
