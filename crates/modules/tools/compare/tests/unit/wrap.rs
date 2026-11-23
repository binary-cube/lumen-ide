use super::*;

/// A row takes as many screen rows as its longest side needs; a row's screen row and the
/// row of a screen row read back.
#[test]
fn a_row_takes_its_longest_side_s_rows() {
    let left = vec!["short".to_owned(), "a line long enough to need three rows here".to_owned()];
    let right = vec!["short".to_owned(), "short".to_owned()];
    let row = |l, r| Row { left: Some(l), result: None, right: Some(r), change: None };
    let rows = [row(0, 0), row(1, 1)];
    let mut heights = Heights::default();
    heights.refresh((&rows, &left, &right), 1, [16, 0, 16]);
    assert_eq!(heights.rows(0), 1);
    assert_eq!(heights.rows(1), 3);
    assert_eq!(heights.total(), 4);
    assert_eq!(heights.first(1), 1);
    assert_eq!(heights.at(2), (1, 1));
    assert_eq!(heights.at(0), (0, 0));
}
