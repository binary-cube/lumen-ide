use super::*;
use crate::sql::{TableSource, lex};

const LITE: Flavor = Flavor::Sqlite;
const PG: Flavor = Flavor::Postgres { version: Some(170_000) };

fn names(sql: &str, flavor: Flavor) -> Resolution {
    let tokens = lex(sql, flavor);
    resolve(sql, &tokens, flavor)
}

/// Where `word` is written as a whole word the `nth` time (from 0).
fn word_at(sql: &str, word: &str, nth: usize) -> usize {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    sql.match_indices(word)
        .filter(|(at, _)| {
            let before = sql[..*at].chars().next_back();
            let after = sql[at + word.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        })
        .nth(nth)
        .unwrap_or_else(|| panic!("{word} #{nth} written"))
        .0
}

/// The role of the name written as `word` (its `nth` time, from 0).
fn role_of<'a>(res: &'a Resolution, sql: &str, word: &str, nth: usize) -> &'a NameRole {
    let at = word_at(sql, word, nth);
    &res.names.iter().find(|n| n.range.start == at).unwrap_or_else(|| panic!("{word} read")).role
}

fn read(res: &Resolution, sql: &str, word: &str, nth: usize) -> bool {
    let at = word_at(sql, word, nth);
    res.names.iter().any(|n| n.range.start == at)
}

#[test]
fn a_join_s_tables_aliases_qualifiers_and_columns() {
    let sql =
        "SELECT u.name, total FROM users u JOIN orders o ON o.user_id = u.id WHERE o.total > 1";
    let res = names(sql, LITE);
    assert_eq!(role_of(&res, sql, "users", 0), &NameRole::Table { table: 0 });
    assert_eq!(role_of(&res, sql, "orders", 0), &NameRole::Table { table: 1 });
    assert_eq!(role_of(&res, sql, "u", 1), &NameRole::Alias { table: 0 });
    assert_eq!(role_of(&res, sql, "u", 0), &NameRole::Qualifier { parts: vec!["u".into()] });
    assert_eq!(
        role_of(&res, sql, "name", 0),
        &NameRole::Column { qualifier: vec!["u".into()], certain: true }
    );
    assert_eq!(
        role_of(&res, sql, "total", 0),
        &NameRole::Column { qualifier: vec![], certain: true }
    );
    assert_eq!(
        role_of(&res, sql, "user_id", 0),
        &NameRole::Column { qualifier: vec!["o".into()], certain: true }
    );
    // The words of the statement aren't names.
    assert!(!read(&res, sql, "SELECT", 0) && !read(&res, sql, "WHERE", 0));
    assert!(!read(&res, sql, "JOIN", 0) && !read(&res, sql, "ON", 0));
    let scope = &res.scopes[0];
    assert_eq!(scope.own, 2);
    assert_eq!(res.qualified(0, &["o".into()]), [1]);
    assert_eq!(res.alias_of(0, 1).map(|n| n.text.as_str()), Some("o"));
}

#[test]
fn the_space_before_the_alias_finds_it() {
    let sql = "SELECT 1 FROM users AS x";
    let res = names(sql, PG);
    assert_eq!(role_of(&res, sql, "x", 0), &NameRole::Alias { table: 0 });
}

#[test]
fn a_schema_before_a_table_is_its_schema() {
    let sql = "SELECT * FROM billing.invoices i";
    let res = names(sql, PG);
    assert_eq!(role_of(&res, sql, "billing", 0), &NameRole::Schema { table: 0 });
    assert_eq!(role_of(&res, sql, "invoices", 0), &NameRole::Table { table: 0 });
    let res = names("SELECT * FROM \"Odd Name\"", PG);
    assert_eq!(res.names[0].text, "Odd Name");
    assert!(res.names[0].quoted);
}

#[test]
fn ctes_are_defined_and_used() {
    let sql = "WITH recent AS (SELECT id, total FROM orders) SELECT r.total FROM recent r";
    let res = names(sql, PG);
    assert_eq!(role_of(&res, sql, "recent", 0), &NameRole::CteName);
    assert_eq!(role_of(&res, sql, "recent", 1), &NameRole::Table { table: 0 });
    // In the main query's scope, `recent` is the CTE.
    let main = res.names.iter().find(|n| n.text == "recent" && n.role != NameRole::CteName);
    let level = main.map(|n| n.level).unwrap();
    assert!(matches!(res.scopes[level].tables[0].source, TableSource::Cte { .. }));
    // Inside its query, `orders` is a table.
    let orders = res.names.iter().find(|n| n.text == "orders").unwrap();
    assert!(matches!(res.scopes[orders.level].tables[0].source, TableSource::Table(_)));
}

#[test]
fn a_derived_table_and_a_function_s_rows() {
    let sql = "SELECT d.n, g FROM (SELECT count(*) AS n FROM users) d, generate_series(1, 3) g";
    let res = names(sql, PG);
    assert_eq!(role_of(&res, sql, "d", 1), &NameRole::Alias { table: 0 });
    let scope = &res.scopes[0];
    assert!(matches!(scope.tables[0].source, TableSource::Derived { .. }));
    assert!(matches!(scope.tables[1].source, TableSource::Function(_)));
    assert_eq!(role_of(&res, sql, "count", 0), &NameRole::Function { qualifier: vec![] });
    // `AS n`: a new name, not a column.
    assert!(!read(&res, sql, "n", 1));
}

#[test]
fn a_statement_s_target_columns() {
    let sql = "INSERT INTO users (id, name) VALUES (1, 'a')";
    let res = names(sql, LITE);
    assert_eq!(role_of(&res, sql, "id", 0), &NameRole::TargetColumn { table: 0 });
    assert_eq!(role_of(&res, sql, "name", 0), &NameRole::TargetColumn { table: 0 });
    let sql = "UPDATE users SET name = email, email = 'x' WHERE id = 1";
    let res = names(sql, LITE);
    assert_eq!(role_of(&res, sql, "name", 0), &NameRole::TargetColumn { table: 0 });
    assert_eq!(
        role_of(&res, sql, "email", 0),
        &NameRole::Column { qualifier: vec![], certain: true }
    );
    assert_eq!(role_of(&res, sql, "email", 1), &NameRole::TargetColumn { table: 0 });
    let sql = "INSERT INTO t (a) VALUES (1) ON CONFLICT (a) DO UPDATE SET b = excluded.b";
    let res = names(sql, PG);
    assert_eq!(role_of(&res, sql, "a", 1), &NameRole::TargetColumn { table: 0 });
    assert_eq!(role_of(&res, sql, "b", 0), &NameRole::TargetColumn { table: 0 });
    assert_eq!(
        role_of(&res, sql, "excluded", 0),
        &NameRole::Qualifier { parts: vec!["excluded".into()] }
    );
}

#[test]
fn bare_names_are_certain_only_where_an_operand_goes() {
    let sql = "SELECT a b, x::date, CAST(y AS text), extract(year FROM d), DATE '2020-01-01' \
               FROM t WHERE z = 1 AND NOT w";
    let res = names(sql, PG);
    let certain = |word: &str, nth| {
        let at = word_at(sql, word, nth);
        res.names
            .iter()
            .find(|n| n.range.start == at)
            .is_some_and(|n| matches!(n.role, NameRole::Column { certain: true, .. }))
    };
    assert!(certain("a", 0), "after SELECT");
    assert!(!certain("b", 0), "an alias given");
    assert!(!certain("date", 0), "a type after ::");
    assert!(!certain("text", 0), "a type after AS");
    assert!(!certain("year", 0), "a call's syntax");
    assert!(!certain("DATE", 0), "a typed literal");
    assert!(certain("z", 0) && certain("w", 0));
}

#[test]
fn ddl_names_and_using_joins_are_marked() {
    let res = names("DROP TABLE IF EXISTS gone", PG);
    let gone = res.names.iter().find(|n| n.text == "gone").unwrap();
    assert!(gone.ddl);
    let res = names("SELECT id FROM a JOIN b USING (id)", PG);
    assert!(res.scopes[0].using);
    let res = names("SELECT id FROM a NATURAL JOIN b", PG);
    assert!(res.scopes[0].using);
    let res = names("SELECT id FROM a JOIN b ON a.id = b.id", PG);
    assert!(!res.scopes[0].using);
}

#[test]
fn nested_queries_see_outer_tables_and_branches_their_own() {
    let sql = "SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id)";
    let res = names(sql, PG);
    let inner = res.names.iter().find(|n| n.text == "user_id").unwrap();
    assert_eq!(res.qualified(inner.level, &["u".into()]).len(), 1);
    assert_eq!(res.qualified(inner.level, &["o".into()]).len(), 1);
    let outer = res.names.iter().find(|n| n.text == "users").unwrap();
    assert!(res.qualified(outer.level, &["o".into()]).is_empty(), "inner aliases stay in");
    let sql = "SELECT a.x FROM a UNION SELECT b.y FROM b";
    let res = names(sql, PG);
    let y = res.names.iter().find(|n| n.text == "y").unwrap();
    assert!(res.qualified(y.level, &["a".into()]).is_empty());
    assert_eq!(res.qualified(y.level, &["b".into()]).len(), 1);
}

#[test]
fn the_statement_at_the_cursor_is_read() {
    let text = "SELECT * FROM users;\nSELECT o.id FROM orders o;";
    let tokens = lex(text, LITE);
    let at = text.find("o.id").unwrap();
    let (res, range) = resolve_at(text, &tokens, at, LITE);
    assert_eq!(&text[range], "SELECT o.id FROM orders o");
    assert_eq!(res.name_at(at).map(|n| n.text.as_str()), Some("o"));
    // Right after a name, it's that name.
    let end = text.find("users").unwrap() + "users".len();
    let (res, _) = resolve_at(text, &tokens, end, LITE);
    assert_eq!(res.name_at(end).map(|n| n.text.as_str()), Some("users"));
}

#[test]
fn created_tables_are_found_anywhere_in_a_script() {
    let text = "CREATE TEMP TABLE IF NOT EXISTS scratch (a int); \
                CREATE OR REPLACE VIEW v AS SELECT 1; CREATE SEQUENCE s; \
                ALTER TABLE old RENAME TO fresh; SELECT 1 INTO kept FROM x; \
                CREATE INDEX i ON t (a)";
    let found = crate::sql::created_tables(text, &lex(text, PG), PG);
    let names: Vec<&str> = found.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["scratch", "v", "s", "fresh", "kept"]);
}

#[test]
fn reading_names_never_panics_on_unfinished_text() {
    for text in [
        "SELECT u. FROM users u",
        "SELECT FROM",
        "SELECT (",
        "WITH x AS (",
        "INSERT INTO t (",
        "UPDATE t SET",
        "SELECT * FROM a JOIN",
        ")))",
        "SELECT \"unterminated",
    ] {
        for flavor in [LITE, PG, Flavor::Generic] {
            let _ = names(text, flavor);
        }
    }
}
