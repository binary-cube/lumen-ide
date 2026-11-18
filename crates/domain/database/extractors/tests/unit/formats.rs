use super::*;
use crate::cell::Kind;
use crate::extractor::{Input, Options, extract};

use crate::test_support::{cols, rows};

fn run(e: &dyn Extractor, columns: Vec<Column>, rows: &[Vec<Cell<'_>>]) -> String {
    extract(e, Input::new("t", columns), Options::default(), rows)
}

#[test]
fn json_keeps_types() {
    assert_eq!(
        run(&Json, cols(), &rows()),
        "[\n  {\"id\": 1, \"name\": \"Ann\", \"price\": 2.5},\n  {\"id\": 2, \"name\": \"O'Hara, \\\"Jo\\\"\\nline\", \"price\": null}\n]"
    );
    let other = vec![Column::new("x", Kind::Other)];
    let values = vec![
        vec![Cell::Bool(true)],
        vec![Cell::Json("{\"a\": [1]}")],
        vec![Cell::Bytes(&[0xde, 0xad])],
        vec![Cell::Float(f64::NAN)],
        vec![Cell::Other("tab\there\u{1}")],
        vec![Cell::Other("2024-01-02 03:04:05")],
    ];
    assert_eq!(
        run(&Json, other.clone(), &values),
        "[\n  {\"x\": true},\n  {\"x\": {\"a\": [1]}},\n  {\"x\": \"0xdead\"},\n  {\"x\": null},\n  {\"x\": \"tab\\there\\u0001\"},\n  {\"x\": \"2024-01-02 03:04:05\"}\n]"
    );
    assert_eq!(run(&Json, other, &[]), "[]");
}

#[test]
fn markdown_aligns_numbers_and_escapes_pipes() {
    let values = vec![vec![Cell::Int(1), Cell::Text("a|b\nc"), Cell::Null]];
    assert_eq!(
        run(&Markdown, cols(), &values),
        "| id | name | price |\n| ---: | --- | ---: |\n| 1 | a\\|b<br>c | NULL |"
    );
}

#[test]
fn html_escapes_and_breaks_lines() {
    let values = vec![vec![Cell::Int(1), Cell::Text("<a & 'b'>\nc"), Cell::Null]];
    assert_eq!(
        run(&Html, cols(), &values),
        "<table>\n  <tr><th>id</th><th>name</th><th>price</th></tr>\n  <tr><td>1</td><td>&lt;a &amp; &#39;b&#39;&gt;<br>c</td><td></td></tr>\n</table>"
    );
}

#[test]
fn xml_names_its_columns_and_marks_nulls() {
    let values = vec![vec![Cell::Int(1), Cell::Text("a\u{1}<\"x\">"), Cell::Null]];
    assert_eq!(
        run(&Xml, cols(), &values),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<data table=\"t\">\n  <row>\n    <column name=\"id\">1</column>\n    <column name=\"name\">a\u{fffd}&lt;&quot;x&quot;&gt;</column>\n    <column name=\"price\" null=\"true\"/>\n  </row>\n</data>"
    );
}

#[test]
fn one_row_puts_every_value_on_a_line() {
    assert_eq!(run(&OneRow, cols(), &rows()), "1,Ann,2.5,2,\"O'Hara, \"\"Jo\"\"\nline\",");
    assert_eq!(run(&OneRow, cols(), &[]), "");
}

#[test]
fn pretty_aligns_columns() {
    let values = vec![
        vec![Cell::Int(1), Cell::Text("Ann"), Cell::Float(2.5)],
        vec![Cell::Int(10), Cell::Text("a\nb"), Cell::Null],
    ];
    assert_eq!(
        run(&Pretty, cols(), &values),
        "+----+------+-------+\n| id | name | price |\n+----+------+-------+\n|  1 | Ann  |   2.5 |\n| 10 | a\\nb | NULL  |\n+----+------+-------+"
    );
    assert_eq!(
        run(&Pretty, cols(), &[]),
        "+----+------+-------+\n| id | name | price |\n+----+------+-------+\n+----+------+-------+"
    );
}

#[test]
fn every_format_writes_the_same_in_blocks() {
    let many: Vec<Vec<Cell<'_>>> = (0..25)
        .map(|i| {
            vec![
                Cell::Int(i),
                if i % 3 == 0 { Cell::Null } else { Cell::Text("x,y") },
                Cell::Float(0.5),
            ]
        })
        .collect();
    for e in crate::builtin(&crate::CsvFormat::defaults()) {
        if e.id() == "pretty" {
            continue; // Its widths come from the first block.
        }
        let whole = run(e.as_ref(), cols(), &many);
        let mut x = crate::Extraction::new(e.as_ref(), Input::new("t", cols()), Options::default());
        let mut pieces = x.begin();
        for block in many.chunks(4) {
            pieces.push_str(&x.rows(block));
        }
        pieces.push_str(&x.finish());
        assert_eq!(pieces, whole, "{}", e.id());
        assert_eq!(x.written(), 25);
    }
}

#[test]
fn dates_and_decimals_stay_as_the_engine_wrote_them() {
    let columns = vec![Column::new("at", Kind::DateTime), Column::new("price", Kind::Decimal)];
    let rows = vec![vec![Cell::Other("2024-02-29 23:59:59.123+02"), Cell::Other("10.50")]];
    assert_eq!(
        run(&Json, columns.clone(), &rows),
        "[\n  {\"at\": \"2024-02-29 23:59:59.123+02\", \"price\": \"10.50\"}\n]"
    );
    assert_eq!(
        run(&crate::SqlInserts { multirow: false }, columns.clone(), &rows),
        "INSERT INTO \"t\" (\"at\", \"price\") VALUES ('2024-02-29 23:59:59.123+02', '10.50');"
    );
    assert_eq!(
        run(&Markdown, columns.clone(), &rows),
        "| at | price |\n| --- | ---: |\n| 2024-02-29 23:59:59.123+02 | 10.50 |"
    );
    assert_eq!(
        run(&Pretty, columns, &rows),
        "+----------------------------+-------+\n| at                         | price |\n+----------------------------+-------+\n| 2024-02-29 23:59:59.123+02 | 10.50 |\n+----------------------------+-------+"
    );
}
