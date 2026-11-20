use super::*;
use crate::view::tests::{editing_view, screen};
use text_layout::TextSystem;
use ui_components::Viewer;

/// A double click opens the shape's text (its HTML's tags left out); typed and ended with
/// Esc, it's set — written back as HTML, one step; unchanged, nothing done.
#[test]
fn a_shape_s_text_is_edited_in_place() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    let at = screen(&view, 400.0, 200.0);
    view.pointer_down(at, 1, ui_components::PressMods::default());
    view.release();
    view.pointer_down(at, 2, ui_components::PressMods::default());
    assert!(view.editing.is_some() && view.wants_text());
    assert_eq!(view.text_selected().as_deref(), Some("Valid?"), "its text, chosen whole");
    view.text_input("Is it\nvalid & ok?");
    crate::view::tests::paint_view(&mut view, &mut text);
    assert!(view.editing.as_ref().is_some_and(|e| e.drawn.size.width > 0.0), "the field drawn");
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(view.editing.is_none());
    let value = |v: &DiagramView| {
        v.diagram.as_ref().unwrap().doc.pages[0].cell("check").unwrap().value.clone()
    };
    assert_eq!(value(&view), "Is it<br>valid &amp; ok?");
    assert!(view.diagram.as_mut().unwrap().undo());
    assert_eq!(value(&view), "<b>Valid?</b>", "one step");
    // ↩ on the one chosen shape opens its text too.
    view.selection = vec!["save".into()];
    assert!(view.key_edit(ViewerKey::Enter, KeyReach::default()));
    assert!(view.editing.is_some());
    view.end_text();
    // Opened and closed without a change: nothing.
    let version = view.diagram.as_ref().unwrap().version;
    view.edit_text("save");
    view.end_text();
    assert_eq!(view.diagram.as_ref().unwrap().version, version);
    // Locked: not opened.
    view.locked = true;
    assert!(!view.edit_text("save"));
}

/// A plain (not HTML) text stays plain, its lines kept.
#[test]
fn a_plain_text_stays_plain() {
    assert_eq!(as_html("a < b\nc"), "a &lt; b<br>c");
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    // The swimlane's style has no `html=1`.
    assert!(view.edit_text("pool"));
    assert!(view.command("select_all"));
    view.text_input("Lane\none");
    view.end_text();
    let cell = view.diagram.as_ref().unwrap().doc.pages[0].cell("pool").unwrap().value.clone();
    assert_eq!(cell, "Lane\none");
}

/// In the field, ⇧ and a press extend the selection to there; a press outside it ends the
/// editing (its text set).
#[test]
fn a_shift_press_extends_and_a_press_outside_ends() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    assert!(view.edit_text("pool"));
    crate::view::tests::paint_view(&mut view, &mut text);
    let drawn = view.editing.as_ref().unwrap().drawn;
    let start = drawn.center();
    assert!(view.editing.as_ref().unwrap().field.contains(start), "on its text");
    view.pointer_down(start, 1, ui_components::PressMods::default());
    view.release();
    assert_eq!(view.text_selected(), None, "the caret put");
    let shift = ui_components::PressMods { shift: true, ..Default::default() };
    view.pointer_down(Point::new(drawn.min_x() + 12.0, drawn.center().y), 1, shift);
    view.release();
    assert!(view.text_selected().is_some(), "extended");
    assert!(!view.edit_key_with(ViewerKey::Tab, KeyReach::default()), "Tab moves nothing");
    assert!(view.editing.is_some());
    view.pointer_down(screen(&view, 5.0, 5.0), 1, ui_components::PressMods::default());
    view.release();
    assert!(view.editing.is_none(), "a press outside ended it");
}
