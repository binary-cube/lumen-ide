//! The console's statements and decisions, without a server.

use super::*;

const VERSIONS: [u32; 9] = [90200, 90600, 100000, 110000, 120000, 140000, 160000, 170000, 180000];

#[test]
fn queries_read_from_a_cursor_and_nothing_else_does() {
    for version in VERSIONS {
        let yes = |sql: &str| statement(sql, version).cursorable();
        assert!(yes("SELECT * FROM users"));
        assert!(yes("  -- the newest\nWITH n AS (SELECT 1) SELECT * FROM n;"));
        assert!(yes("VALUES (1), (2)"));
        assert!(yes("TABLE users"));
        assert!(!yes("SHOW search_path"), "not a query a DECLARE takes");
        assert!(!yes("EXPLAIN SELECT 1"));
        assert!(!yes("SELECT 1; SELECT 2"), "two statements");
        assert!(!yes("UPDATE users SET a = 1 RETURNING id"));
        assert!(!yes("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"), "a write");
        assert!(!yes("SELECT * FROM users FOR UPDATE"), "locks rows: not read-only");
        assert!(!yes("SELECT * FROM users u FOR NO KEY UPDATE OF u"));
        assert!(!yes("SELECT * INTO t2 FROM users"), "makes a table");
        assert!(!yes(""));
    }
}

#[test]
fn a_statement_is_sent_without_its_comments_and_semicolon() {
    let text = "/* top */\n  SELECT 'a;b' AS x -- trailing\n;\n";
    let read = statement(text, 170000);
    assert_eq!(read.body, "SELECT 'a;b' AS x");
    assert_eq!(read.leading, "/* top */\n  ".chars().count());
    assert_eq!(read.first, "SELECT");
    let (declare, prefix) = declare(&cursor_name(3), read.body);
    assert_eq!(declare, "DECLARE console_cursor_3 NO SCROLL CURSOR FOR SELECT 'a;b' AS x");
    assert_eq!(&declare[prefix..], read.body);
    assert_eq!(fetch("console_cursor_3", 501), "FETCH FORWARD 501 FROM console_cursor_3");
    assert_eq!(close("console_cursor_3"), "CLOSE console_cursor_3");
}

#[test]
fn an_errors_place_is_taken_back_to_what_the_user_wrote() {
    // `SELECT nope FROM t` declared: the server points at `nope` in the DECLARE.
    let text = "-- c\nSELECT nope FROM t";
    let read = statement(text, 170000);
    let (declare, prefix) = declare("console_cursor_1", read.body);
    let at = declare.find("nope").unwrap() + 1;
    let error = DbError::sql(sql_error(
        "column \"nope\" does not exist",
        "42703",
        Some(&ErrorPosition::Original(at as u32)),
        None,
        Some("Perhaps you meant to reference the column \"t.name\"."),
    ));
    let back = shifted(error, prefix, read.leading);
    let Some(sql::EnginePosition::Character(n)) = back.sql_error().and_then(|e| e.position) else {
        panic!("{back:?}")
    };
    assert_eq!(text.chars().nth(n - 1), Some('n'), "on `nope` in the text: {n}");
    assert_eq!(back.sql_error().and_then(|e| e.sqlstate.as_deref()), Some("42703"));
    assert!(back.sql_error().and_then(|e| e.hint.as_deref()).is_some_and(|h| h.contains("t.name")));
    // A place inside the wrapper is none; a query of the server's own is no place either.
    let early =
        DbError::sql(sql_error("x", "42601", Some(&ErrorPosition::Original(3)), None, None));
    assert_eq!(shifted(early, prefix, 0).sql_error().and_then(|e| e.position), None);
    let internal = ErrorPosition::Internal { position: 4, query: "SELECT 1".into() };
    assert_eq!(sql_error("x", "42601", Some(&internal), None, None).position, None);
    assert_eq!(sql_error("x", "", None, None, None).sqlstate, None);
    // Other errors pass as they are.
    assert_eq!(shifted(DbError::Cancelled, prefix, 0), DbError::Cancelled);
}

#[test]
fn the_schema_goes_first_in_the_search_path() {
    assert_eq!(search_path("public", "public"), "SET search_path TO public");
    assert_eq!(search_path("Sales", "\"Sales\""), "SET search_path TO \"Sales\", public");
    assert!(READ_ONLY_SESSION.ends_with("READ ONLY"));
}

#[test]
fn what_the_console_can_do_is_the_same_on_every_version() {
    const { assert!(CONSOLE.cursors && CONSOLE.explain.analyze && CONSOLE.notices && CONSOLE.read_only) };
    assert!(CONSOLE.isolation.contains(Isolation::Serializable));
    assert!(!CONSOLE.isolation.contains(Isolation::Immediate));
    assert_eq!(CONSOLE.switch_schema, SchemaSwitch::Statement);
    const { assert!(CONSOLE.databases && CONSOLE.error_positions && !CONSOLE.limited) };
    // Its profiles: the oldest version's before it connects (a session says its own).
    assert_eq!(CONSOLE.profile, crate::profile::caps(database::postgres::OLDEST));
    const { assert!(CONSOLE.profile.actual && CONSOLE.profile.undo_writes) };
}
