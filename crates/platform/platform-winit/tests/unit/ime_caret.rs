use super::*;

fn at(x: f32) -> Rect {
    Rect::new(x, 10.0, 2.0, 18.0)
}

/// macOS: the caret moving tells the system nothing (its "US" popup would show each time);
/// a composition tells it at its start and while it goes on, a new input source once.
#[test]
fn the_caret_is_told_only_when_the_system_needs_it() {
    let mut caret = ImeCaret::default();
    assert_eq!(caret.moved(at(1.0), true), None, "typing, clicking: nothing told");
    assert_eq!(caret.moved(at(2.0), true), None);
    assert_eq!(caret.composing(true), Some(at(2.0)), "a composition starts where it is");
    assert_eq!(caret.composing(true), None, "its next letters: the same place");
    assert_eq!(caret.moved(at(3.0), true), Some(at(3.0)), "it moves while composing");
    assert_eq!(caret.composing(false), None);
    assert_eq!(caret.moved(at(4.0), true), None);
    assert_eq!(caret.source_changed(), Some(at(4.0)), "the keyboard changed: shown once");
    assert_eq!(caret.moved(at(4.0), true), None);
    // Nothing said yet: nothing to tell.
    assert_eq!(ImeCaret::default().source_changed(), None);
}

/// Elsewhere every new place is told at once (as before), the same one once.
#[test]
fn without_the_popup_every_new_place_is_told() {
    let mut caret = ImeCaret::default();
    assert_eq!(caret.moved(at(1.0), false), Some(at(1.0)));
    assert_eq!(caret.moved(at(1.0), false), None);
    assert_eq!(caret.moved(at(2.0), false), Some(at(2.0)));
}
