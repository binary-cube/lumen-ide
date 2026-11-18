use std::io::Read as _;

use super::*;
use crate::data_grid::source::CellClass;

fn u16_at(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// Every part of a zip by its directory, inflated, its CRC checked.
fn parts(zip: &[u8]) -> Vec<(String, String)> {
    let end = zip.len() - 22;
    assert_eq!(u32_at(zip, end), 0x0605_4b50, "end of the directory");
    let count = u16_at(zip, end + 10);
    let mut at = u32_at(zip, end + 16);
    let mut out = Vec::new();
    for _ in 0..count {
        assert_eq!(u32_at(zip, at), 0x0201_4b50);
        let crc = u32_at(zip, at + 16) as u32;
        let compressed = u32_at(zip, at + 20);
        let size = u32_at(zip, at + 24);
        let name_len = u16_at(zip, at + 28);
        let offset = u32_at(zip, at + 42);
        let name = String::from_utf8(zip[at + 46..at + 46 + name_len].to_vec()).unwrap();
        assert_eq!(u32_at(zip, offset), 0x0403_4b50, "{name}'s local header");
        let local_name = u16_at(zip, offset + 26);
        let data_at = offset + 30 + local_name + u16_at(zip, offset + 28);
        let mut text = String::new();
        flate2::read::DeflateDecoder::new(&zip[data_at..data_at + compressed])
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text.len(), size, "{name}");
        let mut check = flate2::Crc::new();
        check.update(text.as_bytes());
        assert_eq!(check.sum(), crc, "{name}");
        out.push((name, text));
        at += 46 + name_len;
    }
    out
}

fn workbook(rows: &[Vec<CellRef<'_>>], blocks: usize) -> Vec<u8> {
    let columns = vec![
        GridColumn::new("id", "INTEGER", CellClass::Integer),
        GridColumn::new("name <&>", "TEXT", CellClass::Text),
        GridColumn::new("price", "NUMERIC", CellClass::Decimal),
        GridColumn::new("on", "BOOL", CellClass::Bool),
    ];
    let mut w = XlsxWriter::new("my/table", &columns);
    let mut out = w.begin().unwrap();
    for block in rows.chunks(blocks.max(1)) {
        out.extend(w.rows(block).unwrap());
    }
    out.extend(w.finish().unwrap());
    out
}

#[test]
fn a_workbook_holds_its_parts_and_rows() {
    let rows = vec![
        vec![
            CellRef::Int(1),
            CellRef::Text("Ann \"A\"\n<x>"),
            CellRef::Other("12.50"),
            CellRef::Bool(true),
        ],
        vec![CellRef::Int(2), CellRef::Null, CellRef::Float(f64::NAN), CellRef::Bytes(&[0xab])],
    ];
    let zip = workbook(&rows, 1);
    let parts = parts(&zip);
    let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/styles.xml",
            "xl/worksheets/sheet1.xml"
        ]
    );
    let workbook = &parts[2].1;
    assert!(workbook.contains("<sheet name=\"mytable\""), "{workbook}");
    let sheet = &parts[5].1;
    assert!(sheet.contains("<c r=\"B1\" s=\"1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">name &lt;&amp;&gt;</t>"), "{sheet}");
    assert!(sheet.contains("<c r=\"A2\"><v>1</v></c>"));
    assert!(sheet.contains("Ann &quot;A&quot;\n&lt;x&gt;"));
    // A decimal kept as text is a number; a boolean is one.
    assert!(sheet.contains("<c r=\"C2\"><v>12.50</v></c>"));
    assert!(sheet.contains("<c r=\"D2\" t=\"b\"><v>1</v></c>"));
    // NULL: no cell.
    assert!(!sheet.contains("r=\"B3\""));
    assert!(sheet.contains("<c r=\"D3\" t=\"inlineStr\"><is><t xml:space=\"preserve\">0xab</t>"));
    assert!(sheet.ends_with("</sheetData></worksheet>"));
}

#[test]
fn the_same_rows_give_the_same_file_in_any_blocks() {
    let rows: Vec<Vec<CellRef<'_>>> = (0..500)
        .map(|i| vec![CellRef::Int(i), CellRef::Text("x"), CellRef::Null, CellRef::Null])
        .collect();
    let a = workbook(&rows, 1);
    let b = workbook(&rows, 77);
    assert_eq!(parts(&a), parts(&b));
    assert_eq!(parts(&a)[5].1.matches("<row ").count(), 501);
}

#[test]
fn columns_are_lettered_as_excel_does() {
    assert_eq!(column_letters(0), "A");
    assert_eq!(column_letters(25), "Z");
    assert_eq!(column_letters(26), "AA");
    assert_eq!(column_letters(701), "ZZ");
    assert_eq!(column_letters(702), "AAA");
    assert_eq!(sheet_name(""), "Sheet1");
    assert_eq!(sheet_name(&"x".repeat(40)).len(), 31);
}

#[test]
fn too_many_rows_are_refused() {
    let columns = vec![GridColumn::new("a", "", CellClass::Text)];
    let mut w = XlsxWriter::new("t", &columns);
    w.begin().unwrap();
    w.written = MAX_ROWS - 2;
    assert!(w.rows(&[vec![CellRef::Null]]).is_ok());
    assert!(w.rows(&[vec![CellRef::Null]]).is_err());
}
