use super::*;

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

/// The items told one by one become one hover, told once; leaving tells its end.
#[test]
fn a_hover_is_told_once_with_all_its_items() {
    let mut drag = FileDrag::default();
    drag.hovered("a".into());
    drag.hovered("b".into());
    assert!(!drag.active(), "not told yet");
    assert_eq!(drag.take_news(), Some(DragNews::Hovered(paths(&["a", "b"]))));
    assert!(drag.active());
    assert_eq!(drag.take_news(), None, "told once");
    assert!(drag.cancelled(), "the app knew of it: told it ended");
    assert!(!drag.active());
    assert_eq!(drag.take_news(), None);
}

/// A hover that leaves before the app heard of it isn't told at all.
#[test]
fn a_hover_gone_before_being_told_is_silent() {
    let mut drag = FileDrag::default();
    drag.hovered("a".into());
    assert!(!drag.cancelled());
    assert_eq!(drag.take_news(), None);
}

/// The drop wins over its hover and ends it; a later drag starts over.
#[test]
fn a_drop_ends_the_hover() {
    let mut drag = FileDrag::default();
    drag.hovered("a".into());
    assert!(drag.take_news().is_some());
    drag.dropped("a".into());
    drag.dropped("b".into());
    assert_eq!(drag.take_news(), Some(DragNews::Dropped(paths(&["a", "b"]))));
    assert!(!drag.active());
    assert_eq!(drag.take_news(), None);
    drag.hovered("c".into());
    assert_eq!(drag.take_news(), Some(DragNews::Hovered(paths(&["c"]))));
    // Another drag entering without the end of this one seen: only its items.
    drag.hovered("d".into());
    assert_eq!(drag.take_news(), Some(DragNews::Hovered(paths(&["d"]))));
}
