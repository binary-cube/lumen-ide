use super::*;

#[test]
fn a_change_is_narrowed_to_what_differs() {
    let text = "keep select  a from t; keep";
    let edit = narrowed(text, 5..22, "SELECT a\nFROM t;").unwrap();
    // The same ` t;` at the end is left alone.
    assert_eq!(edit.range, 5..19);
    assert_eq!(edit.text, "SELECT a\nFROM");
    let mut changed = text.to_owned();
    changed.replace_range(edit.range.clone(), &edit.text);
    assert_eq!(changed, "keep SELECT a\nFROM t; keep");
}

#[test]
fn nothing_differs_nothing_changes() {
    assert_eq!(narrowed("abc", 0..3, "abc"), None);
    assert_eq!(narrowed("abc", 0..9, "x"), None, "a range outside the text");
}

#[test]
fn a_narrowed_change_stays_on_char_boundaries() {
    // `é` and `è` share their first byte: the change starts before it.
    let text = "aé b";
    let edit = narrowed(text, 0..text.len(), "aè b").unwrap();
    assert_eq!((edit.range, edit.text.as_str()), (1..3, "è"));
    let edit = narrowed("xé", 0..3, "xéé").unwrap();
    assert_eq!((edit.range, edit.text.as_str()), (3..3, "é"));
}
