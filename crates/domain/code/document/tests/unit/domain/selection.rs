use super::*;
use crate::domain::edit::Edit;

#[test]
fn merges_overlapping_and_keeps_primary() {
    let s = Selections::new(
        vec![
            Selection::new(5, 8),
            Selection::new(0, 2),
            Selection::new(6, 10),
            Selection::cursor(2),
        ],
        3,
    );
    assert_eq!(s.as_slice(), &[Selection::new(0, 2), Selection::cursor(2), Selection::new(5, 10)]);
    assert_eq!(s.primary(), Selection::cursor(2));
}

#[test]
fn duplicate_cursors_collapse() {
    let s = Selections::new(vec![Selection::cursor(3), Selection::cursor(3)], 0);
    assert_eq!(s.len(), 1);
}

#[test]
fn cursors_move_after_inserted_text() {
    let s = Selections::new(vec![Selection::cursor(1), Selection::cursor(4)], 0);
    let tx = Transaction::new(vec![Edit::new(1..1, "ab"), Edit::new(4..4, "ab")]);
    let mapped = s.map(&tx);
    assert_eq!(mapped.as_slice(), &[Selection::cursor(3), Selection::cursor(8)]);
}
