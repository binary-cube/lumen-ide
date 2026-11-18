//! The catalog's queries and how its answers are read, without a server.

use super::*;

#[test]
fn queries_name_their_schema_or_the_sessions_database() {
    let one = tables(Some("shop"));
    assert!(one.contains("TABLE_SCHEMA = 'shop'"), "{one}");
    let all = tables(None);
    assert!(all.contains("NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')"));
    assert!(columns(None, "orders").contains("TABLE_SCHEMA = DATABASE()"));
    // A name is a literal, never SQL.
    assert!(columns(Some("a'b"), "x").contains("'a''b'"));
    assert!(indexes(ServerVersion::mysql(80036), Some("s"), "t").contains("EXPRESSION"));
    assert!(!indexes(ServerVersion::mysql(50744), Some("s"), "t").contains("EXPRESSION"));
    assert!(!indexes(ServerVersion::mariadb(110404), Some("s"), "t").contains("EXPRESSION"));
    assert!(foreign_keys(Some("s"), Some("t")).contains("k.TABLE_NAME = 't'"));
    assert!(schemas().contains("SCHEMATA"));
}

#[test]
fn defaults_are_read_as_expressions_on_both_engines() {
    // MySQL writes literals bare.
    assert_eq!(default_of(Some("M"), "", "enum", false).as_deref(), Some("'M'"));
    assert_eq!(default_of(Some("0"), "", "tinyint", false).as_deref(), Some("0"));
    assert_eq!(default_of(Some(""), "", "varchar", false).as_deref(), Some("''"));
    assert_eq!(
        default_of(Some("CURRENT_TIMESTAMP"), "DEFAULT_GENERATED", "datetime", false).as_deref(),
        Some("CURRENT_TIMESTAMP")
    );
    assert_eq!(
        default_of(Some("uuid()"), "DEFAULT_GENERATED", "char", false).as_deref(),
        Some("uuid()")
    );
    assert_eq!(default_of(None, "", "int", false), None);
    // MariaDB quotes them itself, and says NULL for none.
    assert_eq!(default_of(Some("'M'"), "", "enum", true).as_deref(), Some("'M'"));
    assert_eq!(default_of(Some("NULL"), "", "text", true), None);
    assert_eq!(
        default_of(Some("current_timestamp()"), "", "datetime", true).as_deref(),
        Some("current_timestamp()")
    );
}

#[test]
fn extras_and_rules_are_read() {
    assert!(auto_increment("auto_increment"));
    assert_eq!(generated("STORED GENERATED"), Some(true));
    assert_eq!(generated("VIRTUAL GENERATED"), Some(false));
    assert_eq!(generated("DEFAULT_GENERATED on update CURRENT_TIMESTAMP"), None);
    assert_eq!(action("CASCADE"), FkAction::Cascade);
    assert_eq!(action("SET NULL"), FkAction::SetNull);
    assert_eq!(action("NO ACTION"), FkAction::NoAction);
    assert_eq!(table_kind("BASE TABLE"), TableKind::Table);
    assert_eq!(table_kind("SYSTEM VERSIONED"), TableKind::Table);
    assert_eq!(table_kind("VIEW"), TableKind::View);
}

#[test]
fn the_builtins_are_listed_once_each() {
    let names: Vec<&str> = BUILTINS.iter().map(|(n, _)| *n).collect();
    let mut once = names.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(names.len(), once.len());
    assert_eq!(builtin_kind("GROUP_CONCAT"), database::RoutineKind::Aggregate);
    assert_eq!(builtin_kind("ROW_NUMBER"), database::RoutineKind::Window);
    assert_eq!(builtin_kind("CONCAT"), database::RoutineKind::Function);
}
