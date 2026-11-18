use super::*;

#[test]
fn a_click_shift_and_cmd_make_spans() {
    let mut s = Selection::default();
    s.set(CellPos::new(5, 1));
    assert_eq!(s.count(), 1);
    s.extend(CellPos::new(7, 2));
    assert_eq!(s.count(), 6);
    assert!(s.contains(CellPos::new(6, 2)));
    assert!(!s.contains(CellPos::new(8, 2)));
    s.toggle(CellPos::new(10, 0));
    assert_eq!(s.count(), 7);
    assert_eq!(s.cursor(), Some(CellPos::new(10, 0)));
    s.toggle(CellPos::new(10, 0));
    assert_eq!(s.count(), 6);
    assert_eq!(s.row_set().into_iter().collect::<Vec<_>>(), vec![5, 6, 7]);
    // Asked one row at a time (what painting does), the same rows.
    assert_eq!((0..12).filter(|&r| s.has_row(r)).collect::<Vec<_>>(), vec![5, 6, 7]);
    assert_eq!(s.col_set().into_iter().collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn overlapping_spans_count_each_cell_once() {
    let mut s = Selection::default();
    s.set(CellPos::new(0, 0));
    s.extend(CellPos::new(1, 1));
    s.toggle(CellPos::new(1, 1));
    // The cell was in the first span too: it stays selected.
    s.toggle(CellPos::new(2, 2));
    assert_eq!(s.cells().len(), 5);
    assert_eq!(s.count(), 5);
}

#[test]
fn rows_columns_and_everything() {
    let mut s = Selection::default();
    s.rows(3, 4, 5, false);
    assert_eq!(s.count(), 10);
    s.column(2, 0, 9, true);
    assert_eq!(s.count(), 18);
    s.all(0, 9, 5);
    assert_eq!(s.count(), 50);
    assert_eq!(s.cursor(), Some(CellPos::new(0, 0)));
}

#[test]
fn keys_move_within_the_page_and_shift_extends() {
    let mut s = Selection::default();
    let page = (100, 199);
    assert!(s.step(1, 0, false, page, 3));
    assert_eq!(s.cursor(), Some(CellPos::new(100, 0)));
    s.step(-5, -5, false, page, 3);
    assert_eq!(s.cursor(), Some(CellPos::new(100, 0)));
    s.step(2, 1, true, page, 3);
    assert_eq!(s.count(), 6);
    s.step(1000, 1000, false, page, 3);
    assert_eq!(s.cursor(), Some(CellPos::new(199, 2)));
    assert_eq!(s.count(), 1);
    assert!(!s.step(1, 0, false, (5, 4), 3));
    assert!(!s.step(1, 0, false, page, 0));
}

#[test]
fn a_smaller_page_clamps_it() {
    let mut s = Selection::default();
    s.set(CellPos::new(50, 4));
    s.clamp((0, 9), 2);
    assert_eq!(s.cursor(), Some(CellPos::new(9, 1)));
    s.clamp((0, 0), 0);
    assert!(s.is_empty());
}
