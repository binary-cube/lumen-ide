use super::*;

fn line(value: CellRef<'_>) -> String {
    let mut buffer = String::from("left over");
    cell_line(&mut buffer, &value);
    buffer
}

/// A cell is drawn as its first line, at most `CELL_CHARS` characters; text cells copy
/// only that part, the others are cut after they're written.
#[test]
fn a_cell_is_drawn_as_its_first_line_cut() {
    assert_eq!(line(CellRef::Text("short")), "short");
    assert_eq!(line(CellRef::Json("{\"a\": 1,\n\"b\": 2}")), "{\"a\": 1, …");
    assert_eq!(line(CellRef::Other("x\r\ny")), "x …");
    let long = "ș".repeat(CELL_CHARS + 5);
    assert_eq!(line(CellRef::Text(&long)), format!("{} …", "ș".repeat(CELL_CHARS)));
    let late = format!("{}\nrest", "a".repeat(CELL_CHARS + 1));
    assert_eq!(line(CellRef::Text(&late)), format!("{} …", "a".repeat(CELL_CHARS)));
    assert_eq!(line(CellRef::Int(42)), "42");
    assert_eq!(line(CellRef::Null), "");
}
