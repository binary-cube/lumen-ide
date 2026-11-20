use super::*;
use ui_components::data_grid::filter::{Compare, GridFilter};

const LINE: &str = r#"{"ts":"2026-11-20T08:35:21.388Z","level":"WARN","target":"database_explorer::worker","thread":"db-explorer","run":"abc","msg":"can't open","fields":{"path":"/a.db","tries":2},"span":[{"name":"project"},{"name":"source"}]}"#;

/// A line's cells: its time in the user's zone, its level, module, thread, message, and its
/// fields with the spans it ran in.
#[test]
fn a_line_becomes_its_cells() {
    let e = parse(LINE);
    assert!(e.cell(1).starts_with("2026-11-2"), "{}", e.cell(1));
    assert!(e.cell(1).ends_with(".388"), "{}", e.cell(1));
    assert_eq!(
        (e.cell(LEVEL), e.cell(2), e.cell(3), e.cell(MSG)),
        ("WARN", "database_explorer::worker", "db-explorer", "can't open")
    );
    assert_eq!(e.cell(5), "path=/a.db  tries=2  in=project › source");
    // Not the log's JSON (the file of before): shown whole as the message.
    let old = parse("2026-11-19T19:11:28Z INFO lumen: window closed");
    assert_eq!(old.cell(MSG), "2026-11-19T19:11:28Z INFO lumen: window closed");
    assert_eq!(local_time("not a time"), "not a time");
}

fn compare(column: &str, op: Compare, value: &str) -> GridFilter {
    GridFilter::Compare { column: column.into(), op, value: CellValue::Text(value.into()) }
}

/// The grid's WHERE over a line's cells: equal in any case, `LIKE`'s "holds", the
/// combinations; a column the log hasn't passes nothing.
#[test]
fn a_line_passes_the_filter_by_its_cells() {
    let e = parse(LINE);
    assert!(passes(&e, None));
    assert!(passes(&e, Some(&compare("level", Compare::Eq, "warn"))));
    assert!(!passes(&e, Some(&compare("level", Compare::Eq, "ERROR"))));
    assert!(passes(&e, Some(&compare("level", Compare::NotEq, "ERROR"))));
    let holds = GridFilter::Contains { column: "msg".into(), text: "OPEN".into() };
    assert!(passes(&e, Some(&holds)));
    let both = GridFilter::And(vec![holds.clone(), compare("target", Compare::Eq, "nothing")]);
    assert!(!passes(&e, Some(&both)));
    assert!(passes(&e, Some(&GridFilter::Or(vec![holds.clone(), compare("x", Compare::Eq, "y")]))));
    assert!(!passes(&e, Some(&GridFilter::Not(Box::new(holds)))));
    assert!(!passes(&e, Some(&compare("nowhere", Compare::Eq, "x"))));
    assert!(passes(&e, Some(&GridFilter::NotNull("fields".into()))));
    assert!(passes(&e, Some(&compare("ts", Compare::Greater, "2000"))));
}

/// The grid's cells cut at CELL_MAX characters (at a character, with `…`), the line kept
/// whole for its details.
#[test]
fn a_long_cell_is_cut_and_the_line_kept_whole() {
    let long = "ă".repeat(CELL_MAX + 50);
    let line = format!(r#"{{"level":"INFO","msg":"{long}"}}"#);
    let e = parse(&line).shortened();
    assert_eq!(e.cell(MSG).chars().count(), CELL_MAX + 1);
    assert!(e.cell(MSG).ends_with('…'));
    assert_eq!(e.raw, line);
    assert_eq!(parse(LINE).shortened().cell(MSG), "can't open", "a short one as it was");
}

/// A line's details: its message whole as the heading, what it is, each field on its own,
/// the spans it ran in; a line that isn't JSON, itself.
#[test]
fn a_line_s_details_hold_it_all() {
    let blocks = details(LINE);
    assert_eq!((blocks[0].kind, blocks[0].text.as_str()), (BlockKind::Heading, "can't open"));
    assert!(
        blocks[1].text.contains("WARN")
            && blocks[1].text.contains("db-explorer")
            && blocks[1].text.contains("run abc")
    );
    let points: Vec<&str> = blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Point { .. }))
        .map(|b| b.text.as_str())
        .collect();
    assert_eq!(points, ["path: /a.db", "tries: 2", "in project", "in source"]);
    assert_eq!(details("not json")[0].text, "not json");
}
