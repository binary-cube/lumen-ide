use super::*;

/// Two files' rows as the comparison lays them (a line only on the right: a row of
/// filler on the left), its places as a paint left them: rows 20 tall from y 100, the
/// code from x 50 in both columns, a character 10 wide.
fn compared() -> Compare {
    let mut c = Compare {
        left: vec!["\tlet a = 1;".into(), "b".into()],
        right: vec!["\tlet a = 1;".into(), "added".into(), "b".into()],
        rows: vec![
            Row { left: Some(0), result: None, right: Some(0), change: None },
            Row { left: None, result: None, right: Some(1), change: Some(0) },
            Row { left: Some(1), result: None, right: Some(2), change: None },
        ],
        list: Rect::new(0.0, 100.0, 1000.0, 400.0),
        row_h: 20.0,
        code_x: [50.0, 0.0, 550.0],
        advance: 10.0,
        loaded: true,
        ..Compare::default()
    };
    c.across[0].area = Rect::new(0.0, 100.0, 500.0, 400.0);
    c.across[2].area = Rect::new(500.0, 100.0, 500.0, 400.0);
    c
}

/// Pressed and dragged over two rows: their text as in the file (its tab a tab).
#[test]
fn a_drag_selects_text_as_in_the_file() {
    let mut c = compared();
    // Row 0, column 4 (after the tab, drawn as four): `let`…
    c.begin_select(0, Point::new(50.0 + 40.0, 105.0), 1);
    assert!(c.drag_select(Point::new(50.0 + 10.0, 145.0)), "to row 2, column 1");
    assert_eq!(
        c.selection_text().as_deref(),
        Some("let a = 1;\nb"),
        "the left's filler row left out"
    );
    c.release();
    // From before the tab.
    c.begin_select(2, Point::new(550.0, 105.0), 1);
    c.drag_select(Point::new(550.0 + 70.0, 105.0));
    assert_eq!(c.selection_text().as_deref(), Some("\tlet"));
}

/// Two clicks: the word; three: the line; ⌘A: the column; Esc: nothing.
#[test]
fn words_lines_and_columns_are_selected() {
    let mut c = compared();
    c.begin_select(2, Point::new(550.0 + 25.0, 125.0), 2);
    assert_eq!(c.selection_text().as_deref(), Some("added"));
    c.begin_select(2, Point::new(550.0 + 25.0, 105.0), 3);
    assert_eq!(c.selection_text().as_deref(), Some("\tlet a = 1;"));
    assert!(ui_components::Viewer::select_all(&mut c));
    assert_eq!(c.selection_text().as_deref(), Some("\tlet a = 1;\nadded\nb"), "the right column");
    assert!(ui_components::Viewer::edit_key(&mut c, ViewerKey::Escape));
    assert_eq!(ui_components::Viewer::selected_text(&c), None);
}
