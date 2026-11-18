use super::*;
use crate::{CellKind, ops};

fn doc() -> Document {
    Document::read(include_str!("../data/flow.drawio")).unwrap()
}

/// A new edit after an undo: what was undone can't be redone. Dirty until saved; an edit
/// that changes nothing isn't one.
#[test]
fn redo_goes_with_a_new_edit_and_dirty_follows_saves() {
    let mut d = Diagram::new(doc());
    assert!(!d.is_dirty());
    assert!(!d.edit(Transaction::default(), None), "nothing to do");
    let tx = ops::set_value(&d.doc, 0, "start", "A");
    d.edit(tx, None);
    assert!(d.is_dirty());
    d.mark_saved();
    assert!(!d.is_dirty());
    assert!(d.undo() && d.history.can_redo());
    let tx = ops::set_value(&d.doc, 0, "start", "B");
    d.edit(tx, None);
    assert!(!d.history.can_redo());
    assert!(!d.redo());
}

/// The oldest steps are let go past the limit; the newest stay.
#[test]
fn the_history_keeps_its_last_steps() {
    let mut d = Diagram::new(doc());
    for i in 0..HISTORY_STEPS + 20 {
        let tx = ops::set_value(&d.doc, 0, "start", &i.to_string());
        d.edit(tx, None);
    }
    let mut undone = 0;
    while d.undo() {
        undone += 1;
    }
    assert_eq!(undone, HISTORY_STEPS);
    assert_eq!(d.doc.pages[0].cell("start").unwrap().value, "19");
}

/// A change whose place isn't there is skipped (never a panic).
#[test]
fn a_change_out_of_place_is_skipped() {
    let mut d = doc();
    let cell = Box::new(crate::Cell::new("x", CellKind::Vertex, "1"));
    let tx = Transaction {
        changes: vec![
            Change::Remove { page: 0, at: 999 },
            Change::Insert { page: 5, at: 0, cell: cell.clone() },
            Change::Replace { page: 0, at: 999, cell },
        ],
    };
    assert!(d.apply(tx).is_empty());
}
