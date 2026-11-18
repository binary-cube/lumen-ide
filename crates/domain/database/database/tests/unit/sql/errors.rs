use super::*;

#[test]
fn positions_have_lines_and_columns_in_characters() {
    let text = "ab\nșț x\r\ny\rz";
    assert_eq!(TextPosition::of(text, 0), TextPosition { offset: 0, line: 0, column: 0 });
    assert_eq!(TextPosition::of(text, 2), TextPosition { offset: 2, line: 0, column: 2 });
    let x = text.find('x').unwrap();
    assert_eq!(TextPosition::of(text, x), TextPosition { offset: x, line: 1, column: 3 });
    let y = text.find('y').unwrap();
    assert_eq!(
        TextPosition::of(text, y),
        TextPosition { offset: y, line: 2, column: 0 },
        "\\r\\n is one break"
    );
    let z = text.find('z').unwrap();
    assert_eq!(TextPosition::of(text, z).line, 3, "a lone \\r is one too");
    // Past the end, or inside a character: kept in.
    assert_eq!(TextPosition::of(text, 999).offset, text.len());
    assert_eq!(TextPosition::of(text, 4).offset, 3, "inside `ș`: its start");
}

#[test]
fn postgres_counts_characters_from_one_in_the_statement_sent() {
    let doc = "select 1;\n-- next\nselect 'ș' frm t;";
    let start = doc.find("select 'ș'").unwrap();
    let end = doc.len() - 1;
    let sent = &doc[start..end];
    let map = BindMap::plain(start);
    // "syntax error at or near "frm"", position 14 (characters, `ș` is one).
    let p = sent[..sent.find("frm").unwrap()].chars().count() + 1;
    let at = locate(doc, sent, &map, EnginePosition::Character(p)).unwrap();
    assert_eq!(at.offset, doc.find("frm").unwrap());
    assert_eq!((at.line, at.column), (2, 11));
    // Position 0 isn't one; past the end ("at end of input") is the statement's end.
    assert_eq!(locate(doc, sent, &map, EnginePosition::Character(0)), None);
    let at = locate(doc, sent, &map, EnginePosition::Character(500)).unwrap();
    assert_eq!(at.offset, end);
}

#[test]
fn sqlite_counts_bytes_from_zero() {
    let doc = "\r\nselect 'ș', nope from t";
    let start = 2;
    let sent = &doc[start..];
    let map = BindMap::plain(start);
    let at = locate(doc, sent, &map, EnginePosition::Byte(sent.find("nope").unwrap())).unwrap();
    assert_eq!(at.offset, doc.find("nope").unwrap());
    assert_eq!((at.line, at.column), (1, 12));
    // Inside a character: its start.
    let inside = sent.find('ș').unwrap() + 1;
    let at = locate(doc, sent, &map, EnginePosition::Byte(inside)).unwrap();
    assert_eq!(at.offset, doc.find('ș').unwrap());
    assert_eq!(locate(doc, sent, &map, EnginePosition::Byte(10_000)).unwrap().offset, doc.len());
}
