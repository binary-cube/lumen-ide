use super::*;

fn only(name: &str, text: &str) -> DataTable {
    let mut found = tables(name, text, Limits::default());
    assert!(!found.is_empty(), "a table in {name}");
    found.remove(0)
}

fn names(t: &DataTable) -> Vec<&str> {
    t.columns.iter().map(|c| c.name.as_str()).collect()
}

/// A document's format by its name, else by its content.
#[test]
fn formats_are_told_by_name_then_content() {
    assert_eq!(detect("a.json", ""), Format::Json);
    assert_eq!(detect("a.yml", ""), Format::Yaml);
    assert_eq!(detect("a.tsv", ""), Format::Csv(b'\t'));
    assert_eq!(detect("data", "  [1, 2]"), Format::Json);
    assert_eq!(detect("data", "<?xml version=\"1.0\"?><a/>"), Format::Xml);
    assert_eq!(detect("data", "<!DOCTYPE html><table></table>"), Format::Html);
    assert_eq!(detect("data", "name: x\nage: 3\n"), Format::Yaml);
    assert_eq!(detect("data", "a;b\n1;2\n"), Format::Csv(b';'));
    assert_eq!(detect("data", "just words"), Format::Unknown);
}

/// A JSON list of records: its keys the columns (nested ones flattened), kinds inferred;
/// a list inside said by its length; found wherever it is.
#[test]
fn json_records_are_a_table() {
    let t = only(
        "o.json",
        r#"{"meta": 1, "orders": [
        {"id": 1, "total": 10.5, "at": "2025-11-19", "customer": {"city": "Cluj"}, "items": [1, 2]},
        {"id": 2, "total": "12", "customer": {"city": "Iași"}}
    ]}"#,
    );
    assert_eq!(t.path, "$.orders");
    assert_eq!(names(&t), ["id", "total", "at", "customer.city", "items"]);
    assert_eq!(t.columns[1].kind, ColumnKind::Number);
    assert_eq!(t.columns[2].kind, ColumnKind::Time);
    assert_eq!(t.columns[3].kind, ColumnKind::Text);
    assert_eq!(t.number(1, 1), Some(12.0), "a number written as text");
    assert_eq!(t.number(0, 4), Some(2.0), "a list: its length");
    assert_eq!(t.text(1, 3).as_deref(), Some("Iași"));
    assert_eq!(t.text(1, 2), None, "missing: empty");
}

/// A list of lists (a header row of text), a record of records (its keys a column), JSON
/// Lines.
#[test]
fn json_lists_records_of_records_and_lines() {
    let t = only("a.json", r#"[["year", "sales"], [2024, 10], [2025, 12]]"#);
    assert_eq!(names(&t), ["year", "sales"]);
    assert_eq!(t.number(1, 1), Some(12.0));
    let t = only("b.json", r#"{"ro": {"pop": 19}, "de": {"pop": 84}}"#);
    assert_eq!(names(&t), ["key", "pop"]);
    assert_eq!((t.text(1, 0).as_deref(), t.number(1, 1)), (Some("de"), Some(84.0)));
    let t = only("c.jsonl", "{\"a\": 1}\n{\"a\": 2}\n");
    assert_eq!(t.rows(), 2);
}

/// YAML read as JSON is: a list of records.
#[test]
fn yaml_records_are_a_table() {
    let t = only("s.yaml", "servers:\n  - name: a\n    cpu: 2\n  - name: b\n    cpu: 4\n");
    assert_eq!(t.path, "$.servers");
    assert_eq!(names(&t), ["name", "cpu"]);
    assert_eq!(t.number(1, 1), Some(4.0));
}

/// XML: an element repeated under one parent, its attributes and text children the
/// columns; deeper groups found too.
#[test]
fn xml_repeated_elements_are_a_table() {
    let found = tables(
        "c.xml",
        r#"<catalog>
        <book id="1"><title>A</title><price>10,5</price></book>
        <book id="2"><title>B</title><price>12</price></book>
        <shop><city>X</city><city>Y</city></shop>
    </catalog>"#,
        Limits::default(),
    );
    let books = found.iter().find(|t| t.path == "/catalog/book").expect("the books");
    assert_eq!(names(books), ["@id", "title", "price"]);
    assert_eq!(books.columns[2].kind, ColumnKind::Number);
    assert_eq!(books.number(0, 2), Some(10.5), "a decimal comma");
    let cities = found.iter().find(|t| t.path == "/catalog/shop/city").expect("the cities");
    assert_eq!(names(cities), ["value"]);
}

/// HTML: each table, its header from `<th>`, a `colspan` repeated, tags and entities read,
/// a table inside a cell its own; a page that doesn't close what it opens still read.
#[test]
fn html_tables_are_read_leniently() {
    let page = r#"<html><body><p>x</p>
        <table><tr><th>Name</th><th colspan=2>Score</th></tr>
        <tr><td><b>Ana</b> &amp; co</td><td>1</td><td>2</td></tr>
        <tr><td>Dan<table><tr><td>inner</td></tr></table></td><td>3<td>4
        </table>
        <script>var t = "<table>";</script>"#;
    let found = tables("p.html", page, Limits::default());
    assert_eq!(found.len(), 2, "the outer one and the one in a cell");
    let outer = found.iter().find(|t| t.rows() == 2).expect("the outer table");
    assert_eq!(names(outer), ["Name", "Score", "Score"]);
    assert_eq!(outer.text(0, 0).as_deref(), Some("Ana & co"));
    assert_eq!(outer.number(1, 2), Some(4.0), "a cell not closed");
}

/// CSV: quotes holding the delimiter, a line break and a doubled quote; the header told.
#[test]
fn csv_follows_its_quotes() {
    let t = only(
        "d.csv",
        "name,note,amount\n\"Ana, M\",\"two\nlines\",\"1\"\nDan,\"say \"\"hi\"\"\",2\n",
    );
    assert_eq!(names(&t), ["name", "note", "amount"]);
    assert_eq!(t.rows(), 2);
    assert_eq!(t.text(0, 0).as_deref(), Some("Ana, M"));
    assert_eq!(t.text(0, 1).as_deref(), Some("two\nlines"));
    assert_eq!(t.text(1, 1).as_deref(), Some("say \"hi\""));
    assert_eq!(t.number(1, 2), Some(2.0));
}

/// A hostile document stops nowhere: deep nesting, too many rows and columns are bounded,
/// broken input gives nothing, never a panic.
#[test]
fn hostile_documents_are_bounded() {
    let deep = format!("{}1{}", "[".repeat(10_000), "]".repeat(10_000));
    let _ = tables("deep.json", &deep, Limits::default());
    let limits = Limits { rows: 10, columns: 3, ..Limits::default() };
    let many: String = std::iter::once("a,b,c,d,e\n".to_owned())
        .chain((0..100).map(|i| format!("{i},1,2,3,4\n")))
        .collect();
    let t = tables("m.csv", &many, limits).remove(0);
    assert_eq!((t.rows(), t.columns.len(), t.left_out), (10, 3, 90));
    assert!(tables("x.json", "{\"a\": [", Limits::default()).is_empty());
    assert!(tables("x.xml", "<a><b></a>", Limits::default()).is_empty());
    let _ = tables("x.html", "<table><tr><td colspan=99999999>&#xFFFFFFFF;", Limits::default());
    let _ = tables("x.yaml", "- [&a [*a]]", Limits::default());
}

/// Numbers as people write them: thousands apart, a percent, a currency, a decimal comma
/// only where no value writes a lone dot.
#[test]
fn numbers_as_people_write_them() {
    use infer::Marks;
    assert_eq!(infer::number("1 204,5", Marks::DecimalComma), Some(1204.5));
    assert_eq!(infer::number("12%", Marks::Plain), Some(12.0));
    assert_eq!(infer::number("$3.5", Marks::Plain), Some(3.5));
    assert_eq!(infer::number("abc", Marks::Plain), None);
    let column = |values: &[&str]| {
        let rows: Vec<Vec<Cell>> = values.iter().map(|v| vec![Cell::text(v)]).collect();
        let (kinds, marks) = infer::kinds(1, &rows);
        (kinds[0], marks[0])
    };
    assert_eq!(column(&["1,5", "2.5"]), (ColumnKind::Text, Marks::Plain), "mixed marks: text");
    // Commas between thousands, read as such (never 1.204).
    assert_eq!(column(&["1,204", "2,500", "-12,000.75"]), (ColumnKind::Number, Marks::Thousands));
    assert_eq!(infer::number("1,204", Marks::Thousands), Some(1204.0));
    assert_eq!(infer::number("-12,000.75", Marks::Thousands), Some(-12000.75));
    assert_eq!(infer::number("1,5", Marks::Thousands), None);
    // A comma with other than three digits after proves a decimal comma.
    assert_eq!(column(&["1,204", "1,5"]), (ColumnKind::Number, Marks::DecimalComma));
}
