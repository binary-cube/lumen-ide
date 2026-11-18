use super::*;
use crate::schema_index::IndexParts;
use crate::schema_index::tests::shop;
use crate::sql::lex;
use crate::{TableInfo, TableKind};

const LITE: Flavor = Flavor::Sqlite;
const PG: Flavor = Flavor::Postgres { version: Some(170_000) };

fn check_with(text: &str, flavor: Flavor, index: &SchemaIndex, known: &[TableRef]) -> SchemaCheck {
    let tokens = lex(text, flavor);
    check(&CheckRequest { text, tokens: &tokens, flavor, index, known, attached: false })
}

/// The problems of `text` on the shop's schema (SQLite), each with the text it's about.
fn problems(text: &str) -> Vec<(String, Problem)> {
    let index = SchemaIndex::build(shop(true));
    check_with(text, LITE, &index, &[])
        .problems
        .into_iter()
        .map(|p| (text[p.range].to_owned(), p.problem))
        .collect()
}

fn unknown(name: &str) -> Problem {
    Problem::UnknownTable { name: name.into() }
}

/// A PostgreSQL schema: `public` (orders) and `billing` (invoices), a partition and a
/// sequence known only as readable.
fn postgres() -> SchemaIndex {
    SchemaIndex::build(IndexParts {
        default_schema: Some("public".into()),
        schemas: vec!["public".into(), "billing".into()],
        tables: vec![
            TableInfo {
                table: TableRef::in_schema("public", "orders"),
                kind: TableKind::Table,
                rows: None,
            },
            TableInfo {
                table: TableRef::in_schema("billing", "invoices"),
                kind: TableKind::Table,
                rows: None,
            },
        ],
        readable: vec![
            TableRef::in_schema("public", "orders_2024"),
            TableRef::in_schema("public", "order_seq"),
        ],
        ..IndexParts::default()
    })
}

#[test]
fn an_unknown_table_is_told_where_it_s_named() {
    assert_eq!(problems("SELECT * FROM nope"), [("nope".into(), unknown("nope"))]);
    assert_eq!(
        problems("SELECT * FROM users u JOIN ordrs o ON o.user_id = u.id"),
        [("ordrs".into(), unknown("ordrs"))]
    );
    assert_eq!(problems("DELETE FROM gone WHERE 1"), [("gone".into(), unknown("gone"))]);
    assert_eq!(problems("SELECT * FROM main.nope"), [("nope".into(), unknown("main.nope"))]);
    // Known ones, in any case.
    assert!(problems("SELECT * FROM USERS JOIN Orders ON 1").is_empty());
    assert!(problems("SELECT * FROM big_orders").is_empty(), "a view");
}

#[test]
fn no_false_alarm_for_what_a_script_makes_or_can_t_be_known() {
    // Created anywhere in the text (a console runs its statements in any order).
    assert!(problems("SELECT * FROM scratch; CREATE TEMP TABLE scratch (a)").is_empty());
    assert!(
        problems("CREATE TABLE IF NOT EXISTS t2 (a); INSERT INTO t2 (a) VALUES (1)").is_empty()
    );
    assert!(problems("CREATE VIEW v AS SELECT 1; SELECT * FROM v").is_empty());
    // CTEs.
    assert!(problems("WITH recent AS (SELECT 1) SELECT * FROM recent").is_empty());
    assert!(
        problems("WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t) SELECT * FROM t")
            .is_empty()
    );
    // A schema not read: temp, an attached database.
    assert!(problems("SELECT * FROM temp.scratch").is_empty());
    assert!(problems("SELECT * FROM other.things").is_empty());
    assert!(problems("ATTACH DATABASE 'x.db' AS x; SELECT * FROM things").is_empty());
    // The engine's own.
    assert!(problems("SELECT * FROM sqlite_master").is_empty());
    assert!(problems("SELECT * FROM sqlite_schema").is_empty());
    // A function's rows; a schema change's own names; dynamic SQL in strings.
    assert!(problems("SELECT * FROM pragma_table_info('users')").is_empty());
    assert!(problems("SELECT * FROM json_each('[1]')").is_empty());
    assert!(problems("DROP TABLE IF EXISTS gone").is_empty());
    assert!(problems("ALTER TABLE gone RENAME TO other").is_empty());
    assert!(problems("SELECT 'SELECT * FROM nope'").is_empty());
    // An unfinished statement is left alone.
    assert!(problems("SELECT * FROM 'nope").is_empty());
    // A temporary table the console's session made.
    let index = SchemaIndex::build(shop(true));
    let known = [TableRef::new("scratch")];
    assert!(check_with("SELECT * FROM scratch", LITE, &index, &known).problems.is_empty());
}

#[test]
fn postgres_knows_its_partitions_sequences_and_unread_schemas() {
    let index = postgres();
    let told = |text: &str| {
        check_with(text, PG, &index, &[])
            .problems
            .into_iter()
            .map(|p| text[p.range].to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(told("SELECT * FROM nope"), ["nope"]);
    assert!(told("SELECT * FROM orders_2024").is_empty(), "a partition");
    assert!(told("SELECT nextval('x') FROM order_seq").is_empty(), "a sequence");
    assert!(told("SELECT * FROM invoices").is_empty(), "another schema's (on the search path?)");
    assert!(told("SELECT * FROM billing.invoices").is_empty());
    assert_eq!(told("SELECT * FROM billing.nope"), ["nope"]);
    assert!(told("SELECT * FROM audit.log").is_empty(), "a schema not read");
    assert!(told("SELECT * FROM pg_class").is_empty());
    assert!(told("SELECT * FROM information_schema.tables").is_empty());
    assert!(told("SELECT * FROM generate_series(1, 3) g").is_empty());
    // Dynamic SQL in a body.
    assert!(told("DO $$ BEGIN EXECUTE 'SELECT * FROM nope'; END $$").is_empty());
    assert!(told("SELECT 1 INTO TEMP kept; SELECT * FROM kept").is_empty());
}

#[test]
fn other_dialects_never_hear_of_unknown_tables() {
    let index = SchemaIndex::build(shop(true));
    let found = check_with("SELECT * FROM nope", Flavor::Generic, &index, &[]);
    assert!(found.problems.is_empty());
}

#[test]
fn an_unknown_column_needs_a_sure_table() {
    assert_eq!(
        problems("SELECT u.nope FROM users u"),
        [("nope".into(), Problem::UnknownColumn { column: "nope".into(), table: "users".into() })]
    );
    assert_eq!(
        problems("INSERT INTO users (id, nmae) VALUES (1, 'a')"),
        [("nmae".into(), Problem::UnknownColumn { column: "nmae".into(), table: "users".into() })]
    );
    assert_eq!(
        problems("UPDATE users SET nope = 1 WHERE id = 1"),
        [("nope".into(), Problem::UnknownColumn { column: "nope".into(), table: "users".into() })]
    );
    // In any case; the engine's own; unqualified ones are never called unknown.
    assert!(problems("SELECT u.EMAIL, u.rowid FROM users u").is_empty());
    assert!(problems("SELECT nope FROM users").is_empty());
    // A qualifier that isn't a table in scope (`excluded`, a schema's function).
    assert!(
        problems(
            "INSERT INTO users (id) VALUES (1) ON CONFLICT (id) DO UPDATE SET name = excluded.nope"
        )
        .is_empty()
    );
    // A CTE's or a derived table's columns aren't known surely.
    assert!(problems("WITH r AS (SELECT id + 1 FROM users) SELECT r.x FROM r").is_empty());
    assert!(problems("SELECT d.x FROM (SELECT 1) d").is_empty());
    // Columns not read: asked for, nothing told.
    let index = SchemaIndex::build(shop(false));
    let found = check_with("SELECT u.nope FROM users u", LITE, &index, &[]);
    assert!(found.problems.is_empty());
    assert_eq!(found.missing, [TableRef::new("users")]);
}

#[test]
fn an_unqualified_column_of_two_tables_is_a_warning() {
    let found = problems("SELECT id FROM users u JOIN orders o ON o.user_id = u.id");
    assert_eq!(
        found,
        [(
            "id".into(),
            Problem::Ambiguous { column: "id".into(), tables: vec!["u".into(), "o".into()] }
        )]
    );
    assert!(found[0].1.is_warning());
    // Not when only one has it, nor merged by USING / NATURAL, nor qualified.
    assert!(problems("SELECT email FROM users u JOIN orders o ON o.user_id = u.id").is_empty());
    assert!(problems("SELECT id FROM users JOIN orders USING (id)").is_empty());
    assert!(problems("SELECT id FROM users NATURAL JOIN orders").is_empty());
    assert!(problems("SELECT u.id FROM users u JOIN orders o ON o.user_id = u.id").is_empty());
    // An ORDER BY's output column, a select list's alias.
    assert!(problems("SELECT u.id FROM users u JOIN orders o ON 1 ORDER BY id").is_empty());
    assert!(problems("SELECT u.id AS id FROM users u JOIN orders o ON 1 GROUP BY id").is_empty());
    // A nearer level's table hides the outer ones.
    assert!(
        problems("SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE total > 1)")
            .is_empty()
    );
    // Words of the syntax, types, aliases given.
    assert!(problems("SELECT CAST(u.id AS id) FROM users u JOIN orders o ON 1").is_empty());
}

#[test]
fn a_whole_script_is_checked_statement_by_statement() {
    let found = problems("SELECT * FROM users;\nSELECT * FROM nope;\nSELECT o.x FROM orders o;");
    let texts: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(texts, ["nope", "x"]);
}
