use super::*;

#[test]
fn typing_replaces_the_selection_and_keeps_line_breaks() {
    let mut a = TextArea::new("one");
    a.insert("\r\ntwo");
    assert_eq!(a.text(), "one\ntwo");
    a.select_all();
    a.insert("x");
    assert_eq!((a.text(), a.cursor()), ("x", 1));
}

#[test]
fn the_caret_moves_by_characters_words_and_lines() {
    let mut a = TextArea::new("ăla bala\nportocala\nx");
    a.home(true, false);
    a.right(true, false);
    assert_eq!(a.cursor(), "ăla".len());
    a.vertical(true, false);
    assert_eq!(a.position(a.cursor()), (1, 3));
    a.vertical(true, false);
    assert_eq!(a.position(a.cursor()), (2, 1), "a short line: its end");
    a.vertical(false, true);
    assert_eq!(a.selected_text(), "ortocala\nx");
    a.end(true, false);
    a.backspace(false);
    a.backspace(true);
    assert_eq!(a.text(), "ăla bala\n");
    a.left(false, false);
    a.delete();
    assert_eq!(a.text(), "ăla bala");
    a.home(false, false);
    a.end(false, true);
    assert_eq!(a.selected_text(), "ăla bala");
}
