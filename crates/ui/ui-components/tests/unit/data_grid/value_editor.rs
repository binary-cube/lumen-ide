//! The value editors: which one a column gets (types named over classes, priorities, the
//! registered ones over the built-ins), and what the built-ins read (JSON, dates, times,
//! choices).

use std::sync::Arc;

use super::*;

fn column(name: &str, type_name: &str, class: CellClass) -> GridColumn {
    GridColumn::new(name, type_name, class)
}

fn builtin() -> Vec<Arc<dyn ValueEditorProvider>> {
    builtins().into_iter().map(|s| Arc::new(s) as Arc<dyn ValueEditorProvider>).collect()
}

fn chosen(providers: &[Arc<dyn ValueEditorProvider>], c: &GridColumn) -> String {
    choose(providers, c).map(|p| p.id().to_owned()).unwrap_or_default()
}

#[test]
fn each_class_gets_its_builtin() {
    let all = builtin();
    assert_eq!(chosen(&all, &column("a", "TEXT", CellClass::Text)), "text");
    assert_eq!(chosen(&all, &column("a", "INTEGER", CellClass::Integer)), "integer");
    assert_eq!(chosen(&all, &column("a", "REAL", CellClass::Decimal)), "decimal");
    assert_eq!(chosen(&all, &column("a", "BOOLEAN", CellClass::Bool)), "bool");
    assert_eq!(chosen(&all, &column("a", "JSON", CellClass::Json)), "json");
    assert_eq!(chosen(&all, &column("a", "UUID", CellClass::Other)), "raw");
    // A type named `date` / `time` alone is checked as such; the others as both.
    assert_eq!(chosen(&all, &column("a", "DATE", CellClass::DateTime)), "date");
    assert_eq!(chosen(&all, &column("a", "time(3)", CellClass::DateTime)), "time");
    assert_eq!(chosen(&all, &column("a", "DATETIME", CellClass::DateTime)), "datetime");
    assert_eq!(chosen(&all, &column("a", "TIMESTAMP", CellClass::DateTime)), "datetime");
    // Binary values are never edited: nothing fits.
    assert_eq!(chosen(&all, &column("a", "BLOB", CellClass::Binary)), "");
}

#[test]
fn a_type_named_wins_over_a_class_and_priority_over_both() {
    let mut all = builtin();
    let array = ValueEditorSpec::new("pg.array", ValueFormat::Raw).for_types(&["_int4", "int[]"]);
    all.push(Arc::new(array));
    let ints = column("tags", "_int4", CellClass::Other);
    assert_eq!(chosen(&all, &ints), "pg.array");
    // A class alone, with a priority, over the built-in of that class.
    let money = ValueEditorSpec::new("money", ValueFormat::Decimal)
        .for_classes(&[CellClass::Decimal])
        .priority(5);
    all.push(Arc::new(money));
    assert_eq!(chosen(&all, &column("a", "REAL", CellClass::Decimal)), "money");
    // Equal fits: the first given wins (the registered ones come first).
    let first = ValueEditorSpec::new("first", ValueFormat::Text).for_classes(&[CellClass::Text]);
    let mut ordered: Vec<Arc<dyn ValueEditorProvider>> = vec![Arc::new(first)];
    ordered.extend(builtin());
    assert_eq!(chosen(&ordered, &column("a", "TEXT", CellClass::Text)), "first");
    // A spec naming types never fits another type, whatever its class.
    let only = ValueEditorSpec::new("only", ValueFormat::Text)
        .for_classes(&[CellClass::Text])
        .for_types(&["citext"])
        .priority(9);
    let list: Vec<Arc<dyn ValueEditorProvider>> = vec![Arc::new(only)];
    assert!(choose(&list, &column("a", "TEXT", CellClass::Text)).is_none());
}

#[test]
fn a_registered_editor_is_found_for_its_column() {
    let spec = ValueEditorSpec::new("test.mood", ValueFormat::Choice(vec!["happy".into()]))
        .for_types(&["mood_test_type"]);
    register(Arc::new(spec));
    let c = column("m", "mood_test_type", CellClass::Text);
    let editor = editor_for(&c);
    assert_eq!(editor.id(), "test.mood");
    assert_eq!(editor.parse("HAPPY", &c), Ok(CellValue::Text("happy".into())));
    assert!(editor.parse("sad", &c).unwrap_err().contains("happy"));
    // Text columns keep the built-in.
    assert_eq!(editor_for(&column("t", "TEXT", CellClass::Text)).id(), "text");
}

#[test]
fn json_is_checked_shown_pretty_and_kept_compact() {
    let spec = ValueEditorSpec::new("json", ValueFormat::Json).in_large_editor();
    let c = column("doc", "JSON", CellClass::Json);
    let raw = r#"{"b": [1, 2.5e3, -0], "a": {"x": null, "y": "q\"\u00e9"}, "e": [], "o": {}}"#;
    assert_eq!(
        spec.text(&CellRef::Json(raw), &c),
        "{\n  \"b\": [\n    1,\n    2.5e3,\n    -0\n  ],\n  \"a\": {\n    \"x\": null,\n    \"y\": \"q\\\"\\u00e9\"\n  },\n  \"e\": [],\n  \"o\": {}\n}"
    );
    let pretty = spec.text(&CellRef::Json(raw), &c);
    let compact = r#"{"b":[1,2.5e3,-0],"a":{"x":null,"y":"q\"\u00e9"},"e":[],"o":{}}"#;
    assert_eq!(spec.parse(&pretty, &c), Ok(CellValue::Json(compact.into())));
    // The same JSON written otherwise is no change.
    assert!(spec.same(&CellRef::Json(raw), &CellValue::Json(compact.into())));
    assert!(!spec.same(&CellRef::Json(raw), &CellValue::Json("{}".into())));
    assert!(spec.large(&c));
    // Not JSON: where it stops being it (1-based in the message).
    for bad in ["{", "{\"a\" 1}", "[1,]", "01", "tru", "\"x", "{} x", "[1 2]", "{a:1}"] {
        assert!(spec.parse(bad, &c).is_err(), "{bad}");
    }
    assert_eq!(json::format("[1, x]", false), Err(4));
    // Blank in a nullable column: NULL.
    assert_eq!(spec.parse("  ", &c), Ok(CellValue::Null));
    // Something that isn't JSON is shown as it is.
    assert_eq!(spec.text(&CellRef::Text("not json"), &c), "not json");
}

#[test]
fn dates_and_times_are_checked() {
    let date = ValueFormat::Date;
    let time = ValueFormat::Time;
    let both = ValueFormat::DateTime;
    for ok in ["2024-02-29", "2023-12-31", "05.03.2024", "03/05/2024", "2024/3/5"] {
        assert!(read(&date, ok, false).is_ok(), "{ok}");
    }
    for bad in ["2023-02-29", "2024-13-01", "2024-1-32", "24-01-01", "yesterday", "2024-01"] {
        assert!(read(&date, bad, false).is_err(), "{bad}");
    }
    for ok in ["10:30", "23:59:59", "23:59:60", "08:00:00.123", "10:00:00Z", "10:00:00+02:00"] {
        assert!(read(&time, ok, false).is_ok(), "{ok}");
    }
    for bad in ["24:00", "10:60", "10", "10:00.5", "10:00:00+25:00", "1:00"] {
        assert!(read(&time, bad, false).is_err(), "{bad}");
    }
    for ok in ["2024-03-05 10:00:00", "2024-03-05T10:00:00.5Z", "2024-03-05", "10:00"] {
        assert_eq!(read(&both, ok, false), Ok(CellValue::Text(ok.into())), "{ok}");
    }
    assert!(read(&both, "2024-03-05 25:00", false).is_err());
    assert_eq!(read(&both, "", true), Ok(CellValue::Null));
    assert!(read(&both, "", false).is_err());
    // The message names the value and the format expected.
    let why = read(&date, "soon", false).unwrap_err();
    assert!(why.contains("soon"), "{why}");
}

#[test]
fn text_stays_as_typed_and_raw_is_unchecked() {
    assert_eq!(read(&ValueFormat::Text, "", true), Ok(CellValue::Text(String::new())));
    assert_eq!(read(&ValueFormat::Raw, " 1.50 ", false), Ok(CellValue::Other(" 1.50 ".into())));
    assert_eq!(read(&ValueFormat::Integer, " -3 ", false), Ok(CellValue::Int(-3)));
}
