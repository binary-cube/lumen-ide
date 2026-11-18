//! The question before an action that loses work, as a modal of its own: "Cancel" has the
//! keys, Tab moves them, Esc / × / Cancel close it with nothing done, a "yes" kept only for
//! the question it answers.

use super::*;

fn question(title: &str) -> Question {
    Question {
        title: title.into(),
        body: "Its work is lost.".into(),
        button: "Stop".into(),
        icon: "stop",
        yes: Yes::default(),
    }
}

fn view(q: &Question) -> ConfirmView {
    let mut view = ConfirmView::default();
    view.set_data(q);
    view
}

#[test]
fn cancel_has_the_keys_and_nothing_is_done_without_a_yes() {
    let q = question("Stop node?");
    let mut v = view(&q);
    assert_eq!(v.question(), Some(&q));
    assert_eq!(v.modal_width(), Some(440.0), "as narrow as the app's own questions");
    assert!(!v.on_yes());
    // ↵ on "Cancel": closed, nothing kept.
    assert!(v.edit_key(ViewerKey::Enter));
    assert!(v.closed());
    assert_eq!(v.take_action(), Some(ViewerAction::Close));
    assert!(!q.yes.take());
    // Esc: the same.
    let mut v = view(&q);
    assert!(v.edit_key(ViewerKey::Escape));
    assert!(v.closed() && !q.yes.take());
    // Typing goes nowhere (a modal: nothing behind it gets it either).
    let mut v = view(&q);
    assert!(!v.edit_key(ViewerKey::Backspace));
    assert!(!v.closed());
}

#[test]
fn a_yes_is_kept_for_its_question_only_and_taken_once() {
    let q = question("Stop node?");
    let other = question("Close the session?");
    let mut v = view(&q);
    assert!(v.edit_key(ViewerKey::Tab));
    assert!(v.on_yes());
    assert!(v.edit_key(ViewerKey::Left));
    assert!(!v.on_yes(), "← / → move the keys too");
    v.edit_key(ViewerKey::Right);
    assert!(v.edit_key(ViewerKey::Enter));
    assert!(v.closed());
    assert!(matches!(v.take_action(), Some(ViewerAction::WithHost(_))));
    assert!(!other.yes.take(), "another question's");
    assert!(q.yes.take());
    assert!(!q.yes.take(), "taken once");
}
