use refactor::signature::Param;

use super::*;

fn kept(name: &str, from: usize) -> ParamRow {
    ParamRow::new(NewParam::Kept { from, param: Param { name: name.into(), ..Param::default() } })
}

fn added(name: &str) -> ParamRow {
    let param = Param { name: name.into(), ..Param::default() };
    ParamRow::new(NewParam::Added { param, value: String::new() })
}

#[test]
fn tab_goes_row_by_row_and_back_to_the_first() {
    let mut rows = vec![kept("a", 0), added("b")];
    let (mut focus, mut edited) = ((0, NAME), false);
    let mut fields =
        ParamFields { rows: &mut rows, focus: &mut focus, areas: &[], edited: &mut edited };
    let mut seen = Vec::new();
    for _ in 0..8 {
        assert!(fields.key(ViewerKey::Tab, KeyReach::default()));
        seen.push(*fields.focus);
    }
    // A kept parameter stops at its default; a new one has the calls' value too.
    let expected = [(0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (1, 3), (0, 0), (0, 1)];
    assert_eq!(seen, expected);
    assert!(!edited, "moving edits nothing");
}

#[test]
fn up_and_down_keep_the_column_a_kept_row_has() {
    let mut rows = vec![kept("a", 0), added("b")];
    let (mut focus, mut edited) = ((1, VALUE), false);
    let mut fields =
        ParamFields { rows: &mut rows, focus: &mut focus, areas: &[], edited: &mut edited };
    assert!(fields.key(ViewerKey::Up, KeyReach::default()));
    assert_eq!(*fields.focus, (0, DEFAULT), "a kept row has no value");
    assert!(fields.key(ViewerKey::Down, KeyReach::default()));
    assert!(fields.key(ViewerKey::Down, KeyReach::default()));
    assert_eq!(*fields.focus, (1, DEFAULT), "the last row stays");
}

#[test]
fn a_press_gives_the_field_pressed_and_typing_takes_one_line() {
    let mut rows = vec![kept("a", 0), kept("b", 1)];
    let areas =
        [(Rect::new(0.0, 0.0, 100.0, 20.0), (0, 0)), (Rect::new(0.0, 30.0, 100.0, 20.0), (1, 1))];
    let (mut focus, mut edited) = ((0, NAME), false);
    let mut fields =
        ParamFields { rows: &mut rows, focus: &mut focus, areas: &areas, edited: &mut edited };
    assert_eq!(fields.press(Point::new(5.0, 60.0), 1), Press::Ignored);
    assert_eq!(fields.press(Point::new(5.0, 35.0), 1), Press::Took);
    assert_eq!(*fields.focus, (1, 1));
    assert!(fields.text("int\nlater"));
    assert!(edited);
    assert_eq!(rows[1].fields[TYPE].text(), "int");
}
