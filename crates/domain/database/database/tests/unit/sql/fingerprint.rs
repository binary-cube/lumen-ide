use super::*;

fn fp(text: &str) -> String {
    fingerprint(text, Flavor::Postgres { version: None }).text
}

#[test]
fn values_and_parameters_are_taken_out() {
    assert_eq!(
        fp("SELECT * FROM users WHERE id = 42 AND name = 'bob'"),
        "select * from users where id = ? and name = ?"
    );
    assert_eq!(fp("SELECT * FROM users WHERE id = $1"), fp("select *  from users\nwhere id = 7"));
    assert_eq!(fp("SELECT 1 WHERE x = -5"), "select ? where x = ?", "a sign is the value's");
    assert_eq!(fp("SELECT a - 5 FROM t"), "select a - ? from t", "a subtraction stays");
    assert_eq!(fp("SELECT TRUE, NULL"), "select ?, null", "NULL isn't a value: IS NULL differs");
    assert_eq!(fp("SELECT $$body$$, E'x\\n'"), "select ?, ?");
    assert_eq!(fp("SELECT '1'::int"), "select ?::int");
}

#[test]
fn spaces_comments_case_and_the_last_semicolon_dont_count() {
    let a = fingerprint("SELECT  id\n  FROM \"Users\" -- who\nWHERE id = 1;", Flavor::Sqlite);
    let b = fingerprint("select id from \"Users\" /* x */ where id = 2", Flavor::Sqlite);
    assert_eq!(a, b);
    assert_eq!(a.text, "select id from \"Users\" where id = ?");
    // A quoted name keeps its case: another table.
    assert_ne!(a, fingerprint("select id from \"users\" where id = 2", Flavor::Sqlite));
}

#[test]
fn lists_of_values_of_any_length_are_one() {
    assert_eq!(fp("SELECT * FROM t WHERE id IN (1, 2, 3)"), fp("SELECT * FROM t WHERE id IN (9)"));
    assert_eq!(fp("SELECT * FROM t WHERE id IN (1, 2, 3)"), "select * from t where id in(?)");
    assert_eq!(fp("INSERT INTO t VALUES (1, 'a')"), "insert into t values(?)");
    // A list of columns isn't a list of values.
    assert_eq!(fp("SELECT f(a, b) FROM t"), "select f(a, b) from t");
}

#[test]
fn the_hash_is_stable_and_differs_for_other_queries() {
    let a = fingerprint("SELECT * FROM t WHERE id = 1", Flavor::Generic);
    assert_eq!(a.hash, fingerprint("select * from t where id = 99", Flavor::Generic).hash);
    assert_ne!(a.hash, fingerprint("SELECT * FROM t WHERE name = 1", Flavor::Generic).hash);
    // FNV-1a of the text: the same on every machine and run.
    assert_eq!(fingerprint("", Flavor::Generic).hash, 0xcbf2_9ce4_8422_2325);
}
