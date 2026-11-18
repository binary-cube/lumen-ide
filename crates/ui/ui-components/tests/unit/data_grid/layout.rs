use super::*;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn columns_are_moved_hidden_and_shown() {
    let mut l = ColumnLayout::new(&names(&["a", "b", "c", "d"]));
    assert_eq!(l.visible(), vec![0, 1, 2, 3]);
    l.move_column(0, 2);
    assert_eq!(l.visible(), vec![1, 2, 0, 3]);
    l.move_column(3, 0);
    assert_eq!(l.visible(), vec![3, 1, 2, 0]);
    assert!(l.hide(1));
    assert_eq!(l.visible(), vec![3, 2, 0]);
    // Places count among the shown ones.
    l.move_column(2, 0);
    assert_eq!(l.visible(), vec![0, 3, 2]);
    l.show(1);
    assert_eq!(l.visible(), vec![0, 3, 1, 2]);
    assert!(l.hide(0) && l.hide(1) && l.hide(2));
    assert!(!l.hide(3), "the last one stays");
    l.show_all();
    assert_eq!(l.visible().len(), 4);
}

#[test]
fn widths_are_kept_within_limits_and_found_by_x() {
    let mut l = ColumnLayout::new(&names(&["a", "b"]));
    l.set_width(0, 10.0);
    assert_eq!(l.width(0), MIN_WIDTH);
    l.set_width(1, 100_000.0);
    assert_eq!(l.width(1), MAX_WIDTH);
    l.fit(1, 900.0);
    assert_eq!(l.width(1), FIT_WIDTH);
    l.set_width(0, 100.0);
    l.set_width(1, 50.0);
    assert_eq!(l.edges(), vec![0.0, 100.0, 150.0]);
    assert_eq!(l.at(20.0, 4.0), Some((0, false)));
    assert_eq!(l.at(98.0, 4.0), Some((0, true)));
    assert_eq!(l.at(120.0, 4.0), Some((1, false)));
    assert_eq!(l.at(151.0, 4.0), Some((1, true)));
    assert_eq!(l.at(300.0, 4.0), None);
}

#[test]
fn a_saved_layout_comes_back_by_name() {
    let mut l = ColumnLayout::new(&names(&["id", "name", "email"]));
    l.set_width(1, 220.0);
    l.move_column(2, 0);
    l.hide(0);
    let saved = l.save();
    assert_eq!(saved.order, names(&["email", "id", "name"]));
    assert_eq!(saved.hidden, names(&["id"]));
    assert_eq!(saved.widths, vec![("name".to_owned(), 220.0)]);
    // The table changed: a column gone, one new.
    let mut again = ColumnLayout::new(&names(&["id", "name", "phone"]));
    again.restore(&saved);
    assert_eq!(again.visible(), vec![1, 2]);
    assert_eq!(again.width(1), 220.0);
    assert!(again.is_hidden(0));
    assert!(!again.is_measured(2));
}
