//! The input method's steps in a page and its cursor.

use super::*;
use platform_api::PageInput;

/// An input method's cursor in bytes, as Chromium counts it (UTF-16), never mid-character.
#[test]
fn an_input_methods_cursor_counts_as_chromium_does() {
    assert_eq!(utf16_at("ab", 1), 1);
    assert_eq!(utf16_at("ă😀b", 2), 1, "after ă (two bytes, one unit)");
    assert_eq!(utf16_at("ă😀b", 6), 3, "after the emoji (four bytes, two units)");
    assert_eq!(utf16_at("ă😀b", 4), 1, "mid-emoji: before it");
    assert_eq!(utf16_at("ab", 99), 2);
}

/// winit empties a composition right before its text: the text then replaces what was
/// composed (a commit), the give-up never told. Given up for good, it's told before what
/// comes next; nothing composed, text is typed.
#[test]
fn a_composition_emptied_before_its_text_is_committed() {
    let none = Composition::default();
    let (composing, step) = none.compose(false);
    assert_eq!(step, ImeStep::Compose);
    // winit: Preedit(""), then Commit(text).
    let (waiting, step) = composing.compose(true);
    assert_eq!(step, ImeStep::Nothing, "the give-up waits");
    assert_eq!(waiting.text(), (none, TextStep::Commit));
    // Given up, then a key: the give-up told first.
    let (waiting, _) = composing.compose(true);
    assert_eq!(waiting.settle(), (none, true));
    assert_eq!(composing.settle(), (composing, false), "still composing: nothing to tell");
    // Composing again after a give-up: no give-up told.
    let (again, step) = waiting.compose(false);
    assert_eq!((again, step), (composing, ImeStep::Compose));
    // Nothing composed: an empty one changes nothing, text is typed.
    assert_eq!(none.compose(true), (none, ImeStep::Nothing));
    assert_eq!(none.text(), (none, TextStep::Type));
}

/// The focus moving or a press ends what's composed; a move or a key doesn't.
#[test]
fn a_composition_ends_at_the_focus_and_a_press() {
    use platform_api::{MouseButton, PageMouse, PageMouseAction};
    let mouse = |action| {
        PageInput::Mouse(PageMouse {
            action,
            at: kernel::Point::new(0.0, 0.0),
            clicks: 1,
            modifiers: Default::default(),
        })
    };
    assert!(Composition::ends_at(&PageInput::Focus(false)));
    assert!(Composition::ends_at(&PageInput::Focus(true)));
    assert!(Composition::ends_at(&mouse(PageMouseAction::Down(MouseButton::Left))));
    assert!(!Composition::ends_at(&mouse(PageMouseAction::Move)));
    assert!(!Composition::ends_at(&PageInput::Text("a".into())));
}
