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

/// Two clicks inside a word, then the mouse moved a little while held (as a real double
/// click does): the whole word stays selected, never cut where the mouse is; dragged on, it
/// takes whole words (the first kept whole). The line as reported: `TURNSTILE_ENABLED`
/// double-clicked between its `A` and `B` was cut to `TURNSTILE_ENA`.
#[test]
fn a_double_click_keeps_its_word_whole_while_the_mouse_moves() {
    let line = "#   TURNSTILE_ENABLED       1 = Turnstile on …";
    let mut c = compared();
    c.right[1] = line.into();
    // Between `ENA` and `BLED`: column 4 + 13.
    let at = Point::new(550.0 + 170.0, 125.0);
    c.begin_select(2, at, 1);
    c.release();
    c.begin_select(2, at, 2);
    c.drag_select(at);
    c.drag_select(Point::new(550.0 + 171.0, 125.0));
    assert_eq!(c.selection_text().as_deref(), Some("TURNSTILE_ENABLED"));
    // Dragged onto `1`: whole words from the first one to it.
    c.drag_select(Point::new(550.0 + 285.0, 125.0));
    assert_eq!(c.selection_text().as_deref(), Some("TURNSTILE_ENABLED       1"));
    // Back before it: the first word kept whole, from its end.
    c.drag_select(Point::new(550.0, 125.0));
    assert_eq!(c.selection_text().as_deref(), Some("#   TURNSTILE_ENABLED"));
    c.release();
}

/// Three clicks, then dragged to the next row: whole lines.
#[test]
fn a_triple_click_drags_by_whole_lines() {
    let mut c = compared();
    c.begin_select(2, Point::new(550.0 + 25.0, 125.0), 3);
    c.drag_select(Point::new(550.0 + 5.0, 145.0));
    assert_eq!(c.selection_text().as_deref(), Some("added\nb"));
}

/// The word around a column, in drawn columns (a tab as four): from its first character to
/// past its last; a column on no word, or past the line, is that column alone.
#[test]
fn the_word_around_a_column_counts_tabs_as_drawn() {
    assert_eq!(word_at("let a_b = $x;", 5), (4, 7), "a_b");
    assert_eq!(word_at("let a_b = $x;", 4), (4, 7), "its first column");
    assert_eq!(word_at("let a_b = $x;", 11), (10, 12), "$x");
    assert_eq!(word_at("let a_b = $x;", 7), (7, 7), "a space: no word");
    assert_eq!(word_at("\tab", 5), (4, 6), "after a tab: four columns in");
    assert_eq!(word_at("\tab", 2), (2, 2), "inside the tab");
    assert_eq!(word_at("ab", 9), (9, 9), "past the line");
    assert_eq!(word_at("ăb c", 1), (0, 2), "a letter of two bytes, one column");
}
