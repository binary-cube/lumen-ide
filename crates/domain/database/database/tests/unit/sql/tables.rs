use super::*;
use crate::sql::lex;

const PG: Flavor = Flavor::Postgres { version: None };

fn table(sql: &str, flavor: Flavor) -> Option<TableRef> {
    query_table(sql, &lex(sql, flavor), flavor)
}

fn written(sql: &str, flavor: Flavor) -> Vec<(TableRef, bool)> {
    written_tables(sql, &lex(sql, flavor), flavor)
}

fn again(sql: &str) -> bool {
    rerunnable(sql, &lex(sql, Flavor::Sqlite), Flavor::Sqlite)
}

#[test]
fn a_query_of_one_table_is_named_after_it() {
    let one = |s: &str| table(s, Flavor::Sqlite);
    assert_eq!(one("SELECT * FROM users"), Some(TableRef::new("users")));
    assert_eq!(one("select id from users u where u.id > 1"), Some(TableRef::new("users")));
    assert_eq!(one("SELECT * FROM users AS u ORDER BY 1"), Some(TableRef::new("users")));
    assert_eq!(one("SELECT * FROM main.users LIMIT 3"), Some(TableRef::in_schema("main", "users")));
    assert_eq!(one("SELECT * FROM \"Order Lines\""), Some(TableRef::new("Order Lines")));
    assert_eq!(
        one("SELECT count(*) FROM orders GROUP BY user_id"),
        Some(TableRef::new("orders")),
        "an aggregate still reads one table"
    );
    assert_eq!(
        one("SELECT * FROM users WHERE id IN (SELECT user_id FROM orders)"),
        Some(TableRef::new("users")),
        "a subquery elsewhere doesn't count"
    );
    assert_eq!(
        table("SELECT * FROM acme.public.Doctrine_Migration_Versions", PG),
        Some(TableRef::in_schema("public", "doctrine_migration_versions")),
        "PostgreSQL folds unquoted names"
    );
    assert_eq!(table("SELECT * FROM \"Users\"", PG), Some(TableRef::new("Users")));
}

#[test]
fn anything_else_has_no_table_name() {
    for sql in [
        "SELECT 1",
        "SELECT * FROM users u JOIN orders o ON o.user_id = u.id",
        "SELECT * FROM users, orders",
        "SELECT * FROM users LEFT JOIN orders USING (id)",
        "SELECT * FROM (SELECT 1) t",
        "SELECT * FROM generate_series(1, 3)",
        "SELECT * FROM users UNION SELECT * FROM admins",
        "WITH x AS (SELECT 1) SELECT * FROM x",
        "VALUES (1), (2)",
        "UPDATE users SET name = 'x'",
        "",
    ] {
        assert_eq!(table(sql, Flavor::Sqlite), None, "{sql}");
    }
}

#[test]
fn the_tables_a_statement_changes() {
    let t = TableRef::new;
    assert_eq!(
        written("INSERT INTO users (name) VALUES ('a')", Flavor::Sqlite),
        [(t("users"), false)]
    );
    assert_eq!(
        written("INSERT OR REPLACE INTO users VALUES (1)", Flavor::Sqlite),
        [(t("users"), false)]
    );
    assert_eq!(written("REPLACE INTO users VALUES (1)", Flavor::MySql), [(t("users"), false)]);
    assert_eq!(written("UPDATE OR IGNORE users SET a = 1", Flavor::Sqlite), [(t("users"), false)]);
    assert_eq!(written("update ONLY Users set a = 1", PG), [(t("users"), false)]);
    assert_eq!(
        written("DELETE FROM main.orders WHERE id = 1", Flavor::Sqlite),
        [(TableRef::in_schema("main", "orders"), false)]
    );
    assert_eq!(written("MERGE INTO stock s USING x ON true", PG), [(t("stock"), false)]);
    assert_eq!(written("TRUNCATE TABLE a, b", PG), [(t("a"), false), (t("b"), false)]);
    assert_eq!(written("ALTER TABLE IF EXISTS users ADD c int", PG), [(t("users"), true)]);
    assert_eq!(written("DROP TABLE IF EXISTS a, b CASCADE", PG), [(t("a"), true), (t("b"), true)]);
    assert_eq!(
        written(
            "WITH gone AS (DELETE FROM orders RETURNING user_id) UPDATE users SET n = 0 FROM gone",
            PG
        ),
        [(t("orders"), false), (t("users"), false)]
    );
    assert_eq!(
        written("INSERT INTO log SELECT * FROM log", Flavor::Sqlite),
        [(t("log"), false)],
        "each once"
    );
    for sql in ["SELECT * FROM users", "CREATE TABLE x (a int)", "DELETE", "BEGIN", ""] {
        assert!(written(sql, Flavor::Sqlite).is_empty(), "{sql}");
    }
}

#[test]
fn only_pure_queries_are_run_again() {
    assert!(again("SELECT * FROM users"));
    assert!(again("with x as (select 1) select * from x"));
    assert!(again("VALUES (1)"));
    assert!(!again("SELECT * FROM users FOR UPDATE"));
    assert!(!again("PRAGMA table_info(users)"), "it reads, but can't be wrapped");
    assert!(!again("INSERT INTO users VALUES (1) RETURNING id"));
    assert!(!again("WITH x AS (DELETE FROM users RETURNING *) SELECT * FROM x"));
    assert!(!again(""));
}
