use super::*;
use crate::domain::selection::Selection;

fn type_text(buffer: &mut TextBuffer, history: &mut History, at: usize, text: &str) {
    let tx = Transaction::single(at..at, text);
    let (inverse, _) = tx.apply(buffer);
    let sel = Selections::single(Selection::cursor(at + text.len()));
    history.record(tx, inverse, Selections::single(Selection::cursor(at)), sel, EditKind::Typing);
}

#[test]
fn typing_is_grouped_by_word() {
    let mut b = TextBuffer::new("");
    let mut h = History::default();
    for (i, c) in "ab cd".chars().enumerate() {
        type_text(&mut b, &mut h, i, &c.to_string());
    }
    assert_eq!(b.to_string(), "ab cd");
    h.undo(&mut b);
    assert_eq!(b.to_string(), "ab ");
    h.undo(&mut b);
    assert_eq!(b.to_string(), "");
    h.redo(&mut b);
    h.redo(&mut b);
    assert_eq!(b.to_string(), "ab cd");
}

#[test]
fn seal_starts_new_group_and_new_edit_clears_redo() {
    let mut b = TextBuffer::new("");
    let mut h = History::default();
    type_text(&mut b, &mut h, 0, "a");
    h.seal();
    type_text(&mut b, &mut h, 1, "b");
    h.undo(&mut b);
    assert_eq!(b.to_string(), "a");
    type_text(&mut b, &mut h, 1, "c");
    assert!(!h.can_redo());
}
