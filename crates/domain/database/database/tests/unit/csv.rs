use super::*;
use TypeClass::*;

fn rows(text: &str, delimiter: u8) -> Vec<Vec<String>> {
    parse(text, delimiter, b'"', usize::MAX, true)
}

#[test]
fn records_follow_rfc_4180() {
    let text = "a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",,\"\"\n\nlast,x,y";
    assert_eq!(
        rows(text, b','),
        vec![vec!["a", "b,c", "say \"hi\""], vec!["multi\nline", "", ""], vec!["last", "x", "y"],]
    );
    // A sample cut mid-record leaves that record out; the limit stops early.
    assert_eq!(parse("a,b\nc,d\ne,", b',', b'"', 10, false).len(), 2);
    assert_eq!(parse("a,b\nc,d\ne,f\n", b',', b'"', 2, true).len(), 2);
    assert_eq!(
        parse("a,\"open\n", b',', b'"', 10, true),
        vec![] as Vec<Vec<String>>,
        "an unclosed quote"
    );
}

#[test]
fn the_delimiter_is_the_one_splitting_records_alike() {
    assert_eq!(sniff_delimiter("a,b,c\n1,2,3\n4,5,6\n", b'"', true), b',');
    assert_eq!(sniff_delimiter("a;b;c\n1,5;2;3\n4;5,1;6\n", b'"', true), b';', "decimal commas");
    assert_eq!(sniff_delimiter("a\tb\n1\t2\n", b'"', true), b'\t');
    assert_eq!(sniff_delimiter("a|b|c\n1|2|3\n", b'"', true), b'|');
    // Commas inside quotes don't count.
    assert_eq!(sniff_delimiter("\"x,y\";\"z,w\"\n\"1,2\";\"3,4\"\n", b'"', true), b';');
    assert_eq!(sniff_delimiter("one column\nonly\n", b'"', true), b',', "none: the comma");
}

#[test]
fn cells_infer_their_columns_class() {
    let body: Vec<Vec<String>> = [
        ["1", "1.5", "true", "2024-03-05", "007", "x", "", "14:30"],
        ["-20", "2", "No", "2024-03-05T14:30:00.5Z", "008", "1", "", "23:59:59"],
        ["+3", "-.5e3", "YES", "2024-03-05 14:30:00+02:00", "010", "y", "", "00:00:00.125"],
    ]
    .iter()
    .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
    .collect();
    assert_eq!(
        infer_types(&body, b','),
        vec![Integer, Decimal, Bool, DateTime, Text, Text, Text, DateTime]
    );
    // Dates in the other orders; impossible ones aren't.
    for date in ["05.03.2024", "03/05/2024", "31/12/2024", "2024/12/31"] {
        assert_eq!(infer_types(&[vec![date.to_owned()]], b','), vec![DateTime], "{date}");
    }
    for text in ["2024-13-01", "32.01.2024", "24:00", "12:60", "2024-03-05T", "1.2.3"] {
        assert_eq!(infer_types(&[vec![text.to_owned()]], b','), vec![Text], "{text}");
    }
    // A decimal comma only when the comma isn't the delimiter; too large for an integer.
    assert_eq!(infer_types(&[vec!["3,14".into()]], b';'), vec![Decimal]);
    assert_eq!(infer_types(&[vec!["3,14".into()]], b','), vec![Text]);
    assert_eq!(infer_types(&[vec!["99999999999999999999".into()]], b','), vec![Decimal]);
    // Short records: their missing cells are empty.
    assert_eq!(
        infer_types(&[vec!["1".into()], vec!["2".into(), "x".into()]], b','),
        vec![Integer, Text]
    );
}

#[test]
fn a_header_is_guessed_from_how_it_differs() {
    let csv = |t: &str| rows(t, b',');
    assert!(guess_header(&csv("id,name,price\n1,Ann,2.5\n2,Bo,3\n"), b','));
    assert!(!guess_header(&csv("1,Ann,2.5\n2,Bo,3\n"), b','));
    // All text: codes of one length under a longer name.
    assert!(guess_header(&csv("country\nRO\nDE\nFR\n"), b','));
    assert!(!guess_header(&csv("RO\nDE\nFR\n"), b','));
    // Nothing to tell: named when its cells are filled, different, and not values.
    assert!(guess_header(&csv("first,last\nAnn Marie,Smith\nBo,Li Wong\n"), b','));
    assert!(!guess_header(&csv("x,x\nAnn Marie,Smith\nBo,Li Wong\n"), b','));
    assert!(!guess_header(&[], b','));
}

#[test]
fn a_sample_gives_the_dialect_and_the_columns() {
    let mut sample =
        b"\xEF\xBB\xBFid;price;when;note\n1;2,50;2024-01-02;\"a;b\"\n2;3;2024-01-03;x\n".to_vec();
    let guess = super::guess(&sample, 100, true);
    assert_eq!(guess.dialect, CsvDialect { delimiter: b';', quote: b'"', header: true, bom: true });
    let columns: Vec<(Option<&str>, TypeClass)> =
        guess.columns.iter().map(|c| (c.name.as_deref(), c.class)).collect();
    assert_eq!(
        columns,
        vec![
            (Some("id"), Integer),
            (Some("price"), Decimal),
            (Some("when"), DateTime),
            (Some("note"), Text)
        ]
    );
    // Without a header the columns are unnamed (the UI numbers them).
    let plain = super::guess(b"1\t2\n3\t4\n", 100, true);
    assert_eq!(plain.dialect.delimiter, b'\t');
    assert!(!plain.dialect.header);
    assert!(plain.columns.iter().all(|c| c.name.is_none() && c.class == Integer));
    // A sample cut in the middle of a character and of a record.
    sample.extend_from_slice("3;4;2024-01-04;ș".as_bytes());
    sample.pop();
    let cut = super::guess(&sample, 100, false);
    assert_eq!(cut.columns.len(), 4);
    assert_eq!(cut.columns[0].class, Integer);
    assert_eq!(strip_bom(b"abc"), (&b"abc"[..], false));
}

/// Where `scanner` ends records in `text` fed `piece` bytes at a time.
fn ends(text: &[u8], piece: usize) -> Vec<usize> {
    let mut scanner = RecordScanner::new(b',', b'"');
    let mut out = Vec::new();
    let mut base = 0;
    for chunk in text.chunks(piece) {
        let mut at = 0;
        while let Some(n) = scanner.end(&chunk[at..]) {
            out.push(base + at + n);
            at += n + 1;
        }
        base += chunk.len();
    }
    out
}

#[test]
fn records_end_outside_quotes_whatever_the_pieces() {
    let text = b"a,\"b\nc\",d\r\n\"say \"\"x\"\"\n\",e\nf\"g\nh,\"\"\n";
    let whole = ends(text, text.len());
    assert_eq!(whole, vec![10, 25, 29, 34]);
    // Fed a byte at a time (a quote and its double split apart), the same ends.
    for piece in 1..text.len() {
        assert_eq!(ends(text, piece), whole, "pieces of {piece}");
    }
    // A quote not at a field's start is a character: `f"g` doesn't open one.
    let mut scanner = RecordScanner::new(b',', b'"');
    assert_eq!(scanner.end(b"\"open\nstill"), None);
    assert!(scanner.in_quotes());
}

#[test]
fn fields_are_split_as_written_and_read_as_parse_reads_them() {
    let record = b"a,\"b,c\",\"say \"\"hi\"\"\",,\"x\"y,\"\"";
    let fields = split_fields(record, b',', b'"');
    let raw: Vec<&[u8]> = fields.iter().map(|f| &record[f.clone()]).collect();
    assert_eq!(raw, vec![&b"a"[..], b"\"b,c\"", b"\"say \"\"hi\"\"\"", b"", b"\"x\"y", b"\"\""]);
    let texts: Vec<String> = raw.iter().map(|r| field_text(r, b'"')).collect();
    assert_eq!(texts, ["a", "b,c", "say \"hi\"", "", "xy", ""]);
    // The same as the whole-text parser.
    let text = std::str::from_utf8(record).unwrap();
    assert_eq!(rows(text, b','), vec![texts]);
    assert_eq!(trim_line_ending(b"a,b\r\n"), b"a,b");
    assert!(is_blank(b"\r\n") && is_blank(b"\n") && !is_blank(b"\"\"\n"));
}

#[test]
fn fields_are_quoted_only_when_they_must_be_or_the_file_quotes_all() {
    let dialect = CsvDialect { delimiter: b';', ..CsvDialect::default() };
    let plain = CsvStyle::default();
    assert_eq!(encode_field("abc", &dialect, &plain), "abc");
    assert_eq!(encode_field("a,b", &dialect, &plain), "a,b", "a comma isn't the delimiter");
    assert_eq!(encode_field("a;b", &dialect, &plain), "\"a;b\"");
    assert_eq!(encode_field("say \"hi\"", &dialect, &plain), "\"say \"\"hi\"\"\"");
    assert_eq!(encode_field("two\nlines", &dialect, &plain), "\"two\nlines\"");
    assert_eq!(encode_field(" padded", &dialect, &plain), "\" padded\"");
    let all = CsvStyle { quote_all: true, ..plain };
    assert_eq!(encode_field("abc", &dialect, &all), "\"abc\"");
    let fields = vec!["1".to_owned(), String::new(), "x;y".to_owned()];
    assert_eq!(encode_record(&fields, &dialect, &plain), "1;;\"x;y\"");
    assert_eq!(encode_record(&[String::new()], &dialect, &plain), "\"\"", "not a blank line");
}

#[test]
fn the_style_is_the_files_line_ending_and_quoting() {
    let comma = CsvDialect::default();
    assert_eq!(
        guess_style(b"a,b\r\n1,2\r\n", &comma),
        CsvStyle { line_ending: LineEnding::CrLf, quote_all: false }
    );
    assert_eq!(
        guess_style(b"\"a\",\"b\"\n\"1\",\"x\ny\"\n", &comma),
        CsvStyle { line_ending: LineEnding::Lf, quote_all: true }
    );
    assert!(!guess_style(b"\"a\",b\n", &comma).quote_all);
    assert!(guess_style(b"\"only\"", &comma).quote_all, "a record without its end");
    assert_eq!(guess_style(b"", &comma), CsvStyle::default());
}

#[test]
fn cells_are_values_of_their_columns_class() {
    assert_eq!(cell_value("", Integer), Value::Null);
    assert_eq!(cell_value("42", Integer), Value::Int(42));
    assert_eq!(cell_value("+42", Integer), Value::Other("+42".into()), "kept as written");
    assert_eq!(cell_value("2,50", Decimal), Value::Other("2,50".into()));
    assert_eq!(cell_value("abc", Text), Value::Text("abc".into()));
    assert_eq!(cell_text(&Value::Int(7)), Some("7".into()));
    assert_eq!(cell_text(&Value::Null), Some(String::new()));
    assert_eq!(cell_text(&Value::Bytes(vec![1])), None);
    // A known delimiter is taken as given.
    let tsv = guess_with(b"a,b\tc\n1,2\t3\n", 10, true, Some(b'\t'));
    assert_eq!(tsv.dialect.delimiter, b'\t');
    assert_eq!(tsv.columns.len(), 2);
}
