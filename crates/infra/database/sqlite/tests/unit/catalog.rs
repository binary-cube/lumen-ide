use super::*;

#[test]
fn checks_are_found_in_a_definition_outside_strings_and_comments() {
    let sql = "CREATE TABLE t (
        a INTEGER CHECK (a > 0) -- CHECK (in a comment)
        , b TEXT DEFAULT 'CHECK (no)' CONSTRAINT short check(length(b) < (10 + 2)),
        \"check\" TEXT, /* CHECK (x) */
        CHECK (a < 100 OR b = 'x)')
    )";
    assert_eq!(checks(sql), vec!["a > 0", "length(b) < (10 + 2)", "a < 100 OR b = 'x)'"]);
    assert!(checks("CREATE TABLE t (a, b)").is_empty());
}

#[test]
fn words_and_names_are_told_from_strings() {
    let sql =
        "CREATE TABLE t (id INTEGER PRIMARY KEY autoincrement, note TEXT DEFAULT 'AUTOINCREMENT')";
    assert!(has_word(sql, "AUTOINCREMENT"));
    assert!(!has_word("CREATE TABLE t (note TEXT DEFAULT 'AUTOINCREMENT')", "AUTOINCREMENT"));
    let view =
        "CREATE VIEW v AS SELECT * FROM \"my \"\"users\"\"\" JOIN [teams] JOIN `x` -- users2";
    let names: Vec<String> = names(view).collect();
    assert!(names.contains(&"my \"users\"".to_owned()), "{names:?}");
    assert!(names.contains(&"teams".to_owned()) && names.contains(&"x".to_owned()));
    assert!(!names.contains(&"users2".to_owned()), "comments aren't read");
}

#[test]
fn main_is_the_schema_of_a_table_named_without_one() {
    assert_eq!(table_ref("main", "users"), TableRef::new("users"));
    assert_eq!(table_ref("aux", "users"), TableRef::in_schema("aux", "users"));
    assert_eq!(schema_of(&TableRef::new("users")), "main");
}

#[test]
fn collations_and_generated_expressions_are_read_from_a_definition() {
    let sql = "CREATE TABLE t (
        id INTEGER PRIMARY KEY,
        \"Name\" TEXT COLLATE nocase NOT NULL CHECK (name <> 'x' COLLATE binary),
        price REAL DEFAULT (CAST(1 AS REAL)),
        total REAL GENERATED ALWAYS AS (price * (1 + 0.2)) STORED,
        [label] TEXT AS (upper(\"Name\")),
        CONSTRAINT pk UNIQUE (price),
        CHECK (price > 0)
    )";
    let clauses = column_clauses(sql);
    let names: Vec<&str> = clauses.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "Name", "price", "total", "label"], "table constraints left out");
    assert_eq!(clauses[1].collation.as_deref(), Some("nocase"));
    assert_eq!(clauses[1].generated, None, "a default's or a check's words aren't the column's");
    assert_eq!(clauses[2], ColumnClause { name: "price".into(), ..ColumnClause::default() });
    assert_eq!(clauses[3].generated.as_deref(), Some("price * (1 + 0.2)"));
    assert_eq!(clauses[4].generated.as_deref(), Some("upper(\"Name\")"));
    assert!(column_clauses("CREATE VIRTUAL TABLE f USING fts5").is_empty());
}
