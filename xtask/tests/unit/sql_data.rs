use super::*;

fn row(name: &str, kind: &str, arity: i64) -> FunctionRow {
    FunctionRow { name: name.into(), kind: kind.into(), arity }
}

#[test]
fn merges_a_functions_forms_into_one_sorted_line() {
    let text = format(
        "3.50.0",
        &[
            row("substr", "s", 3),
            row("SUBSTR", "s", 2),
            row("max", "w", 1),
            row("max", "s", -1),
            row("max", "a", 1),
            row("abs", "s", 1),
            row("bad name", "s", 1),
            row("", "s", 1),
        ],
    );
    let body: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(body, ["ABS\ts\t1", "MAX\ta,s,w\t-1,1", "SUBSTR\ts\t2,3"]);
    assert!(text.lines().next().is_some_and(|l| l.starts_with('#')));
    assert!(text.contains("SQLite 3.50.0"));
}

#[test]
fn reads_the_bundled_sqlite() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    let mut statement = db.prepare("SELECT name, type, narg FROM pragma_function_list").unwrap();
    let rows: Vec<FunctionRow> = statement
        .query_map([], |r| Ok(FunctionRow { name: r.get(0)?, kind: r.get(1)?, arity: r.get(2)? }))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let text = format("x", &rows);
    for name in ["COUNT\t", "SUBSTR\t", "COALESCE\t", "JSON_EXTRACT\t", "ROW_NUMBER\t"] {
        assert!(text.lines().any(|l| l.starts_with(name)), "{name}");
    }
}
