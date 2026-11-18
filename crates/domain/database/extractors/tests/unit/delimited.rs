use super::*;
use crate::extractor::{Input, Options, extract};

use crate::test_support::{cols, rows};

fn format(id: &str) -> CsvFormat {
    CsvFormat::defaults().into_iter().find(|f| f.id == id).unwrap()
}

fn text(f: CsvFormat, rows: &[Vec<Cell<'_>>]) -> String {
    extract(&Delimited(f), Input::new("t", cols()), Options::default(), rows)
}

#[test]
fn tab_separated_quotes_only_what_needs_it() {
    assert_eq!(text(format("tsv"), &rows()), "1\tAnn\t2.5\n2\t\"O'Hara, \"\"Jo\"\"\nline\"\t");
    assert_eq!(text(format("tsv-header"), &rows()[..1]), "id\tname\tprice\n1\tAnn\t2.5");
}

#[test]
fn csv_has_a_header_and_quotes_commas() {
    assert_eq!(
        text(format("csv"), &rows()),
        "id,name,price\n1,Ann,2.5\n2,\"O'Hara, \"\"Jo\"\"\nline\","
    );
    assert_eq!(text(format("pipe"), &rows()[..1]), "id|name|price\n1|Ann|2.5");
    assert_eq!(
        text(format("semicolon"), &[vec![Cell::Int(1), Cell::Text("a;b"), Cell::Null]]),
        "id;name;price\n1;\"a;b\";"
    );
}

#[test]
fn a_format_s_options_are_kept() {
    let mut f = CsvFormat::new("x", "Mine", " | ", false);
    f.quote = Some('\'');
    f.null_text = "\\N".into();
    f.line_end = LineEnd::CrLf;
    let rows = vec![
        vec![Cell::Int(1), Cell::Text("it's"), Cell::Null],
        vec![Cell::Int(2), Cell::Text("\\N"), Cell::Bytes(&[0xab, 1])],
    ];
    assert_eq!(text(f.clone(), &rows), "1 | 'it''s' | \\N\r\n2 | '\\N' | 0xab01");
    f.quote = None;
    assert_eq!(text(f, &rows[..1]), "1 | it's | \\N");
}

#[test]
fn formats_go_through_the_settings_and_back() {
    for f in CsvFormat::defaults() {
        assert_eq!(CsvFormat::decode(&f.encode()), Some(f));
    }
    let mut odd = CsvFormat::new("csv-1", "a;b=c\\d", "\t;", true);
    odd.null_text = "=;".into();
    odd.quote = None;
    odd.line_end = LineEnd::CrLf;
    assert_eq!(CsvFormat::decode(&odd.encode()), Some(odd));
    assert_eq!(CsvFormat::decode("garbage"), None);
    // Without a separator, or a quote inside the separator: refused.
    assert_eq!(CsvFormat::decode("id=x;name=X;separator="), None);
    assert_eq!(CsvFormat::decode("id=x;name=X;separator=\";quote=\""), None);
    assert_eq!(CsvFormat::decode("id=x;name=X;separator=,;quote=ab"), None);
}

#[test]
fn extensions_follow_the_separator() {
    assert_eq!(format("tsv").extension(), "tsv");
    assert_eq!(format("csv").extension(), "csv");
    assert_eq!(format("pipe").extension(), "csv");
}
