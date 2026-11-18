//! The console's statements and decisions, without a server.

use super::*;

#[test]
fn a_statement_is_sent_without_its_comments_and_semicolon() {
    let text = "/* top */\n  SELECT 'a;b' AS x # trailing\n;\n";
    let read = statement(text);
    assert_eq!(read.body, "SELECT 'a;b' AS x");
    assert_eq!(read.leading, "/* top */\n  ".chars().count());
    assert_eq!((read.first.as_str(), read.kind), ("SELECT", StatementKind::Query));
    assert!(statement("SELECT * FROM t FOR UPDATE").locks);
    assert!(statement("SELECT * FROM t LOCK IN SHARE MODE").locks);
    assert!(!statement("SELECT 1").locks);
    assert!(statement("SELECT 1; SELECT 2").several);
    // A trigger's body is one statement.
    let trigger = "CREATE TRIGGER t BEFORE INSERT ON x FOR EACH ROW BEGIN SET NEW.a = 1; END";
    assert!(!statement(trigger).several);
}

#[test]
fn transactions_begin_with_their_isolation_set_first() {
    assert_eq!(begin_statements(&TxOptions::default()).unwrap(), ["START TRANSACTION"]);
    let serializable = TxOptions { isolation: Some(Isolation::Serializable), read_only: true };
    assert_eq!(
        begin_statements(&serializable).unwrap(),
        ["SET TRANSACTION ISOLATION LEVEL SERIALIZABLE", "START TRANSACTION READ ONLY"]
    );
    let sqlite = TxOptions { isolation: Some(Isolation::Immediate), read_only: false };
    assert!(begin_statements(&sqlite).is_err());
}

#[test]
fn an_errors_place_is_found_from_mysqls_words() {
    let text = "SELECT *\nFROMM customers WHERE id = 1";
    let message = "You have an error in your SQL syntax; check the manual that corresponds to \
                   your MySQL server version for the right syntax to use near 'FROMM customers \
                   WHERE id = 1' at line 2";
    let at = near_position(text, message).unwrap();
    assert_eq!(text.chars().skip(at - 1).take(5).collect::<String>(), "FROMM");
    // Near the end: after the last character.
    let end = near_position("SELECT * FROM", "… near '' at line 1").unwrap();
    assert_eq!(end, "SELECT * FROM".len() + 1);
    // A line before the place: the same text earlier isn't it.
    let twice = "SELECT x\nFROM t WHERE x\n";
    let at = near_position(twice, "… near 'x' at line 2").unwrap();
    assert_eq!(at, twice.rfind('x').unwrap() + 1, "on line 2's own: {at}");
    assert_eq!(near_position("SELECT 1", "Unknown column 'x' in 'field list'"), None);
    // Taken back through a wrapper (`EXPLAIN …`), the comments before counted.
    let error = DbError::sql(sql_error(1064, "42000", message, Some(text)));
    let shifted = shifted(error, 0, 3);
    assert!(matches!(
        shifted.sql_error().and_then(|e| e.position),
        Some(sql::EnginePosition::Character(_))
    ));
}

#[test]
fn warnings_become_notices() {
    let n = warning("Warning", "1292", "Truncated incorrect INTEGER value: '12x'");
    assert_eq!((n.severity.as_str(), n.detail.as_deref()), ("WARNING", Some("error 1292")));
}

#[test]
fn what_the_console_can_do_by_version() {
    let old = caps(ServerVersion::mysql(50744));
    assert!(!old.cursors && old.explain.plan && !old.explain.analyze && !old.databases);
    assert_eq!(old.switch_schema, SchemaSwitch::Statement);
    assert!(
        old.isolation.contains(Isolation::RepeatableRead)
            && !old.isolation.contains(Isolation::Deferred)
    );
    assert!(caps(ServerVersion::mysql(80036)).explain.analyze);
    assert!(caps(ServerVersion::mariadb(101106)).explain.analyze);
    assert_eq!(CONSOLE, old);
    assert!(READ_ONLY_SESSION.ends_with("READ ONLY"));
}
