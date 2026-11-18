use super::*;
use crate::sql::lex;

const PG: Flavor = Flavor::Postgres { version: Some(170_000) };
const LITE: Flavor = Flavor::Sqlite;

/// The scope at `‸` in `sql`.
fn at(sql: &str, flavor: Flavor) -> QueryScope {
    let offset = sql.find('‸').expect("a cursor");
    let text = sql.replace('‸', "");
    let tokens = lex(&text, flavor);
    scope_in(&text, &tokens, offset, flavor)
}

fn both(sql: &str) -> [QueryScope; 2] {
    [at(sql, LITE), at(sql, PG)]
}

fn names(s: &QueryScope) -> Vec<String> {
    s.tables
        .iter()
        .map(|t| match &t.alias {
            Some(a) => format!("{}:{a}@{}", label(t), t.level),
            None => format!("{}@{}", label(t), t.level),
        })
        .collect()
}

fn label(t: &ScopeTable) -> String {
    match &t.source {
        TableSource::Table(r) => r.to_string(),
        TableSource::Cte { name, .. } => format!("cte {name}"),
        TableSource::Derived { .. } => "derived".into(),
        TableSource::Function(f) => format!("fn {f}"),
    }
}

fn column(q: &[&str]) -> Expect {
    Expect::Column { qualifier: Some(q.iter().map(|s| (*s).to_owned()).collect()) }
}

const ANY_COLUMN: Expect = Expect::Column { qualifier: None };

#[test]
fn an_alias_dot_offers_that_table_s_columns() {
    for s in both("select * from events e where e.‸") {
        assert_eq!(s.expect, column(&["e"]));
        assert_eq!(names(&s), ["events:e@0"]);
        assert_eq!(s.qualified(&["e".into()]), [0]);
        assert_eq!(s.clause, Clause::Where);
    }
    // The table's own name qualifies it too when it has no alias.
    let s = at("SELECT users.‸ FROM users", LITE);
    assert_eq!(s.expect, column(&["users"]));
    assert_eq!(s.qualified(&["users".into()]), [0]);
    // A prefix after the dot.
    let s = at("select e.na‸ from events e", PG);
    assert_eq!(s.expect, column(&["e"]));
    assert_eq!(s.prefix, "na");
    assert_eq!(s.replace, 9..11);
}

#[test]
fn unfinished_statements_are_read_as_far_as_they_go() {
    // The tables after the cursor count (the select list is written before the FROM).
    let s = at("SELECT u.‸ FROM users u", LITE);
    assert_eq!(s.expect, column(&["u"]));
    assert_eq!(names(&s), ["users:u@0"]);
    // Nothing between SELECT and FROM: a column.
    for s in both("SELECT ‸ FROM users") {
        assert_eq!(s.expect, ANY_COLUMN);
        assert_eq!(names(&s), ["users@0"]);
    }
    // A half typed name never becomes a table.
    let s = at("SELECT * FROM us‸", LITE);
    assert_eq!(s.expect, Expect::Table { schema: None, join: false });
    assert_eq!(s.prefix, "us");
    assert!(s.tables.is_empty());
    // Before the next clause: it isn't swallowed as the name.
    let s = at("SELECT * FROM public.‸ WHERE id = 1", PG);
    assert_eq!(s.expect, Expect::Table { schema: Some("public".into()), join: false });
    assert_eq!(s.clause, Clause::From);
    // An open parenthesis, an open string elsewhere.
    let s = at("SELECT count(‸ FROM orders", LITE);
    assert_eq!(s.expect, ANY_COLUMN);
    let s = at("SELECT * FROM t WHERE a = (‸", PG);
    assert_eq!(s.expect, ANY_COLUMN);
    // Empty: a statement's first word.
    assert_eq!(at("‸", LITE).expect, Expect::Keyword(KeywordPlace::Start));
    assert_eq!(at("sel‸", LITE).expect, Expect::Keyword(KeywordPlace::Start));
    assert_eq!(at("sel‸", LITE).prefix, "sel");
}

#[test]
fn strings_comments_numbers_and_parameters_offer_nothing() {
    for sql in [
        "SELECT 'abc‸' FROM t",
        "SELECT 'abc‸",
        "SELECT * FROM t -- a comm‸ent",
        "SELECT * FROM t -- a comment‸",
        "SELECT /* x‸ */ 1",
        "SELECT * FROM t LIMIT 10‸",
        "SELECT * FROM t WHERE id = :na‸",
        "SELECT * FROM t WHERE id = 1‸2",
    ] {
        assert_eq!(at(sql, LITE).expect, Expect::Nothing, "{sql}");
    }
    assert_eq!(at("SELECT $$ a‸ $$", PG).expect, Expect::Nothing);
    // After a closed string: a keyword again.
    assert_eq!(
        at("SELECT * FROM t WHERE a = 'x' ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterCondition(Clause::Where))
    );
}

#[test]
fn tables_are_expected_where_a_statement_names_one() {
    let table = Expect::Table { schema: None, join: false };
    for sql in [
        "SELECT * FROM ‸",
        "SELECT * FROM a, ‸",
        "INSERT INTO ‸",
        "UPDATE ‸",
        "DELETE FROM ‸",
        "MERGE INTO ‸",
        "DROP TABLE ‸",
        "DROP TABLE IF EXISTS ‸",
        "ALTER TABLE ‸",
        "TRUNCATE ‸",
        "TRUNCATE TABLE ‸",
        "TABLE ‸",
        "CREATE INDEX i ON ‸",
        "CREATE TABLE b (id integer REFERENCES ‸",
        "ALTER TABLE b ADD FOREIGN KEY (a_id) REFERENCES ‸",
        "DELETE FROM a USING ‸",
        "UPDATE a SET x = 1 FROM ‸",
    ] {
        assert_eq!(at(sql, PG).expect, table, "{sql}");
    }
    for sql in [
        "SELECT * FROM a JOIN ‸",
        "SELECT * FROM a LEFT JOIN ‸",
        "SELECT * FROM a LEFT OUTER JOIN ‸",
        "MERGE INTO t USING ‸",
    ] {
        assert_eq!(at(sql, PG).expect, Expect::Table { schema: None, join: true }, "{sql}");
    }
    // A new name: nothing.
    for sql in [
        "CREATE TABLE ‸",
        "SELECT * FROM t AS ‸",
        "WITH ‸",
        "SELECT a AS ‸ FROM t",
        "SELECT * INTO ‸ FROM t",
    ] {
        assert_eq!(at(sql, PG).expect, Expect::Nothing, "{sql}");
    }
}

#[test]
fn after_a_table_the_next_clauses_are_offered() {
    for sql in ["SELECT * FROM users ‸", "SELECT * FROM users u ‸", "SELECT * FROM users AS u ‸"]
    {
        assert_eq!(
            at(sql, LITE).expect,
            Expect::Keyword(KeywordPlace::AfterTable { join: false }),
            "{sql}"
        );
    }
    let s = at("SELECT * FROM users u wh‸", LITE);
    assert_eq!(s.expect, Expect::Keyword(KeywordPlace::AfterTable { join: false }));
    assert_eq!(names(&s), ["users:u@0"], "the word typed isn't the alias");
    assert_eq!(
        at("SELECT * FROM a JOIN b ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterTable { join: true })
    );
    assert_eq!(at("SELECT * FROM a LEFT ‸", LITE).expect, Expect::Keyword(KeywordPlace::JoinWord));
    assert_eq!(at("SELECT * FROM a GROUP ‸", LITE).expect, Expect::Keyword(KeywordPlace::By));
    assert_eq!(at("SELECT * FROM a ORDER ‸", LITE).expect, Expect::Keyword(KeywordPlace::By));
    assert_eq!(at("SELECT a ‸", LITE).expect, Expect::Keyword(KeywordPlace::AfterSelectItem));
    assert_eq!(at("SELECT * ‸", LITE).expect, Expect::Keyword(KeywordPlace::AfterSelectItem));
    assert_eq!(at("SELECT a * ‸ FROM t", LITE).expect, ANY_COLUMN, "a product's operand");
    assert_eq!(at("INSERT ‸", LITE).expect, Expect::Keyword(KeywordPlace::Into));
    assert_eq!(at("DELETE ‸", LITE).expect, Expect::Keyword(KeywordPlace::From));
    assert_eq!(
        at("INSERT INTO t ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterInsertTarget)
    );
    assert_eq!(
        at("INSERT INTO t (a, b) ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterInsertTarget)
    );
    assert_eq!(
        at("INSERT INTO t (a) VALUES (1) ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterValues)
    );
    assert_eq!(at("UPDATE t ‸", LITE).expect, Expect::Keyword(KeywordPlace::AfterUpdateTarget));
    assert_eq!(at("SELECT 1 UNION ‸", LITE).expect, Expect::Keyword(KeywordPlace::SetOperation));
    assert_eq!(
        at("SELECT * FROM t ORDER BY a ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterOrderItem)
    );
    assert_eq!(
        at("SELECT * FROM t GROUP BY a ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterGroupItem)
    );
    assert_eq!(at("WITH x AS (SELECT 1) ‸", LITE).expect, Expect::Keyword(KeywordPlace::AfterCte));
    assert_eq!(
        at("SELECT 1; ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::Start),
        "a new statement"
    );
    assert_eq!(at("SELECT 1;\nsel‸", LITE).expect, Expect::Keyword(KeywordPlace::Start));
}

#[test]
fn columns_are_offered_in_every_clause_holding_expressions() {
    for sql in [
        "SELECT ‸ FROM t",
        "SELECT a, ‸ FROM t",
        "SELECT DISTINCT ‸ FROM t",
        "SELECT * FROM t WHERE ‸",
        "SELECT * FROM t WHERE a = 1 AND ‸",
        "SELECT * FROM t WHERE a > ‸",
        "SELECT * FROM t WHERE NOT ‸",
        "SELECT * FROM t WHERE a IN (‸",
        "SELECT * FROM t WHERE a BETWEEN ‸",
        "SELECT * FROM t GROUP BY ‸",
        "SELECT * FROM t GROUP BY a, ‸",
        "SELECT * FROM t GROUP BY a HAVING ‸",
        "SELECT * FROM t ORDER BY ‸",
        "SELECT * FROM t ORDER BY a DESC, ‸",
        "SELECT * FROM t a JOIN u b ON a.id = ‸",
        "UPDATE t SET a = ‸",
        "UPDATE t SET a = 1 WHERE ‸",
        "DELETE FROM t WHERE ‸",
        "DELETE FROM t RETURNING ‸",
        "UPDATE t SET a = 1 RETURNING ‸",
        "INSERT INTO t (a) VALUES (1) RETURNING ‸",
        "SELECT CASE WHEN ‸ END FROM t",
        "SELECT CASE WHEN a THEN ‸ END FROM t",
        "SELECT max(‸) FROM t",
        "SELECT coalesce(a, ‸) FROM t",
        "SELECT row_number() OVER (ORDER BY ‸) FROM t",
        "SELECT * FROM t WHERE EXISTS (SELECT 1 FROM u WHERE ‸",
    ] {
        let s = at(sql, PG);
        assert_eq!(s.expect, ANY_COLUMN, "{sql}");
        assert!(!s.tables.is_empty(), "{sql}: its tables in scope");
    }
    // The clause it is.
    assert_eq!(at("SELECT * FROM t ORDER BY ‸", LITE).clause, Clause::OrderBy);
    assert_eq!(at("SELECT * FROM t GROUP BY a HAVING ‸", LITE).clause, Clause::Having);
    assert_eq!(at("UPDATE t SET a = 1 RETURNING ‸", PG).clause, Clause::Returning);
}

#[test]
fn join_on_expects_a_whole_condition() {
    for s in both("select * from orders o join users u on ‸") {
        assert_eq!(s.expect, Expect::JoinCondition { right: 1, left: vec![0] });
        assert_eq!(names(&s), ["orders:o@0", "users:u@0"]);
    }
    let s = at("SELECT * FROM a JOIN b ON b.a_id = a.id JOIN c ON ‸", LITE);
    assert_eq!(s.expect, Expect::JoinCondition { right: 2, left: vec![0, 1] });
    // Typing the condition's start.
    let s = at("SELECT * FROM a x JOIN b y ON y‸", LITE);
    assert_eq!(s.expect, Expect::JoinCondition { right: 1, left: vec![0] });
    assert_eq!(s.prefix, "y");
    // Its operands after.
    assert_eq!(at("SELECT * FROM a JOIN b ON b.‸", LITE).expect, column(&["b"]));
    assert_eq!(
        at("SELECT * FROM a JOIN b ON b.x = a.y ‸", LITE).expect,
        Expect::Keyword(KeywordPlace::AfterCondition(Clause::On))
    );
    // USING.
    let s = at("SELECT * FROM a JOIN b USING (‸", LITE);
    assert_eq!(s.expect, Expect::UsingColumns { right: 1, left: vec![0] });
    // After the condition, another JOIN still sees every table.
    let s = at("SELECT * FROM a JOIN b ON b.id = a.b_id LEFT JOIN ‸", PG);
    assert_eq!(s.expect, Expect::Table { schema: None, join: true });
    assert_eq!(names(&s), ["a@0", "b@0"]);
}

#[test]
fn insert_column_lists_and_update_targets() {
    let s = at("INSERT INTO users (‸", LITE);
    assert_eq!(s.expect, Expect::InsertColumns { table: 0 });
    assert_eq!(names(&s), ["users@0"]);
    let s = at("INSERT INTO public.users (id, ‸) VALUES (1, 2)", PG);
    assert_eq!(s.expect, Expect::InsertColumns { table: 0 });
    assert_eq!(names(&s), ["public.users@0"]);
    assert_eq!(at("INSERT INTO t (a) VALUES (‸", LITE).expect, Expect::Value);
    assert_eq!(at("INSERT INTO t (a) VALUES (1, ‸", LITE).expect, Expect::Value);
    // UPDATE … SET: the target's columns, left of `=`.
    for sql in [
        "UPDATE users SET ‸",
        "UPDATE users u SET a = 1, ‸",
        "UPDATE users AS u SET ‸ WHERE id = 1",
    ] {
        let s = at(sql, PG);
        assert_eq!(s.expect, Expect::TableColumns { table: 0 }, "{sql}");
    }
    // ON CONFLICT (…) DO UPDATE SET.
    let s = at("INSERT INTO t (a) VALUES (1) ON CONFLICT (‸", LITE);
    assert_eq!(s.expect, Expect::TableColumns { table: 0 });
    let s = at("INSERT INTO t (a) VALUES (1) ON CONFLICT (a) DO UPDATE SET ‸", PG);
    assert_eq!(s.expect, Expect::TableColumns { table: 0 });
    // INSERT … SELECT reads its own FROM.
    let s = at("INSERT INTO t (a) SELECT x.‸ FROM src x", LITE);
    assert_eq!(s.expect, column(&["x"]));
    assert_eq!(names(&s), ["t@0", "src:x@0"]);
    // A CREATE INDEX's columns.
    let s = at("CREATE INDEX i ON users (‸", LITE);
    assert_eq!(s.expect, Expect::TableColumns { table: 0 });
    // ALTER TABLE … DROP COLUMN.
    assert_eq!(at("ALTER TABLE users DROP COLUMN ‸", PG).expect, Expect::TableColumns { table: 0 });
    assert_eq!(
        at("ALTER TABLE users ADD COLUMN ‸", PG).expect,
        Expect::Keyword(KeywordPlace::Other)
    );
}

#[test]
fn ctes_are_tables_with_their_columns() {
    let s = at("WITH x AS (SELECT id, name AS n FROM users) SELECT x.‸ FROM x", LITE);
    assert_eq!(s.expect, column(&["x"]));
    assert_eq!(names(&s), ["cte x@0"]);
    assert_eq!(
        s.tables[0].source,
        TableSource::Cte {
            name: "x".into(),
            columns: vec![OutputColumn::Named("id".into()), OutputColumn::Named("n".into())]
        }
    );
    assert_eq!(s.ctes.len(), 1);
    // Its column list wins over its query's.
    let s = at("WITH x(a, b) AS (SELECT 1, 2) SELECT ‸ FROM x", PG);
    assert_eq!(
        s.tables[0].source,
        TableSource::Cte {
            name: "x".into(),
            columns: vec![OutputColumn::Named("a".into()), OutputColumn::Named("b".into())]
        }
    );
    // Recursive: the CTE sees itself.
    let sql = "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT t.‸ + 1 FROM t WHERE n < 9) SELECT n FROM t";
    let s = at(sql, PG);
    assert_eq!(s.expect, column(&["t"]));
    assert_eq!(names(&s), ["cte t@0"]);
    // Several, the later seeing the earlier; offered where a table goes.
    let s = at("WITH a AS (SELECT 1 AS x), b AS (SELECT * FROM ‸) SELECT 1", LITE);
    assert_eq!(s.expect, Expect::Table { schema: None, join: false });
    assert_eq!(s.ctes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    // A CTE's star means its tables.
    let s = at("WITH x AS (SELECT * FROM users u) SELECT ‸ FROM x", LITE);
    let TableSource::Cte { columns, .. } = &s.tables[0].source else { panic!("a cte") };
    assert_eq!(
        columns,
        &[OutputColumn::Star { qualifier: None, tables: vec![TableRef::new("users")] }]
    );
    // The main statement may be a write.
    let s = at("WITH old AS (SELECT id FROM t) DELETE FROM t WHERE id IN (SELECT ‸ FROM old)", PG);
    assert_eq!(names(&s), ["cte old@0", "t@1"]);
}

#[test]
fn derived_tables_have_their_select_list_s_columns() {
    let s = at("SELECT d.‸ FROM (SELECT id, total AS sum, count(*) n FROM orders) d", LITE);
    assert_eq!(s.expect, column(&["d"]));
    assert_eq!(names(&s), ["derived:d@0"]);
    assert_eq!(
        s.tables[0].source,
        TableSource::Derived {
            columns: ["id", "sum", "n"].map(|c| OutputColumn::Named(c.into())).to_vec()
        }
    );
    // Inside it, the outer tables aren't seen (unless LATERAL).
    let s = at("SELECT * FROM users u, (SELECT ‸ FROM orders o) d", PG);
    assert_eq!(names(&s), ["orders:o@0"]);
    let s = at("SELECT * FROM users u, LATERAL (SELECT ‸ FROM orders o) d", PG);
    assert_eq!(names(&s), ["orders:o@0", "users:u@1"]);
    // Joined: its condition.
    let s = at("SELECT * FROM users u JOIN (SELECT user_id FROM orders) o ON ‸", PG);
    assert_eq!(s.expect, Expect::JoinCondition { right: 1, left: vec![0] });
}

#[test]
fn subqueries_see_out_and_their_aliases_stay_in() {
    // Correlated: the inner query sees the outer's tables, nearer first.
    let sql = "SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.‸)";
    for s in both(sql) {
        assert_eq!(s.expect, column(&["u"]));
        assert_eq!(names(&s), ["orders:o@0", "users:u@1"]);
        assert_eq!(s.qualified(&["u".into()]), [1]);
    }
    let s = at("SELECT * FROM users u WHERE u.id IN (SELECT ‸ FROM orders o)", LITE);
    assert_eq!(names(&s), ["orders:o@0", "users:u@1"]);
    // In the select list.
    let s = at("SELECT u.name, (SELECT count(*) FROM orders o WHERE o.‸) FROM users u", LITE);
    assert_eq!(s.expect, column(&["o"]));
    assert_eq!(names(&s), ["orders:o@0", "users:u@1"]);
    // Outside, the inner aliases don't leak.
    let s = at("SELECT ‸ FROM users u WHERE u.id IN (SELECT user_id FROM orders o)", LITE);
    assert_eq!(names(&s), ["users:u@0"]);
    let s = at("SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o) AND ‸", PG);
    assert_eq!(names(&s), ["users:u@0"]);
    // An inner alias hiding an outer one of the same name.
    let s = at("SELECT * FROM a x WHERE EXISTS (SELECT 1 FROM b x WHERE x.‸)", LITE);
    assert_eq!(s.qualified(&["x".into()]), [0]);
    // Two levels deep.
    let s = at(
        "SELECT * FROM a WHERE EXISTS (SELECT 1 FROM b WHERE EXISTS (SELECT 1 FROM c WHERE ‸))",
        LITE,
    );
    assert_eq!(names(&s), ["c@0", "b@1", "a@2"]);
}

#[test]
fn each_branch_of_a_union_has_its_own_from() {
    let s = at("SELECT a FROM x UNION SELECT ‸ FROM y", LITE);
    assert_eq!(names(&s), ["y@0"]);
    let s = at("SELECT ‸ FROM x UNION ALL SELECT b FROM y", PG);
    assert_eq!(names(&s), ["x@0"]);
}

#[test]
fn the_select_list_s_aliases_where_the_dialect_allows_them() {
    let sql =
        |clause: &str| format!("SELECT count(*) AS total, name n FROM t GROUP BY name {clause}");
    let order = at(&format!("{} ‸", sql("ORDER BY")), PG);
    assert_eq!(order.aliases, ["total", "n"]);
    let order = at(&format!("{} ‸", sql("ORDER BY")), LITE);
    assert_eq!(order.aliases, ["total", "n"]);
    // GROUP BY: PostgreSQL and SQLite; HAVING: SQLite only.
    let group = at("SELECT count(*) AS total FROM t GROUP BY ‸", PG);
    assert_eq!(group.aliases, ["total"]);
    let having = at("SELECT count(*) AS total FROM t GROUP BY a HAVING ‸", PG);
    assert!(having.aliases.is_empty());
    let having = at("SELECT count(*) AS total FROM t GROUP BY a HAVING ‸", LITE);
    assert_eq!(having.aliases, ["total"]);
    let generic = at("SELECT count(*) AS total FROM t GROUP BY ‸", Flavor::Generic);
    assert!(generic.aliases.is_empty());
    // Not in WHERE.
    assert!(at("SELECT a AS b FROM t WHERE ‸", LITE).aliases.is_empty());
}

#[test]
fn a_star_right_before_the_cursor_can_be_expanded() {
    let s = at("SELECT *‸ FROM users u JOIN orders o ON o.user_id = u.id", LITE);
    assert_eq!(s.star, Some(Star { range: 7..8, qualifier: None }));
    let s = at("SELECT o.*‸ FROM orders o", PG);
    assert_eq!(s.star, Some(Star { range: 7..10, qualifier: Some("o".into()) }));
    assert_eq!(at("SELECT a *‸ FROM t", LITE).star, None, "a product");
    assert_eq!(at("SELECT * ‸FROM t", LITE).star, None, "not right before");
    assert_eq!(at("SELECT count(*‸) FROM t", LITE).star, None, "an argument");
}

#[test]
fn tables_named_with_schemas_quotes_and_functions() {
    let s = at("SELECT * FROM public.users u JOIN \"Order Lines\" ol ON ‸", PG);
    assert_eq!(names(&s), ["public.users:u@0", "Order Lines:ol@0"]);
    let s = at("SELECT * FROM main.[my table] t WHERE t.‸", LITE);
    assert_eq!(names(&s), ["main.my table:t@0"]);
    let s = at("SELECT g.‸ FROM generate_series(1, 10) g", PG);
    assert_eq!(names(&s), ["fn generate_series:g@0"]);
    let s = at("SELECT * FROM json_each(x) AS j, users WHERE ‸", LITE);
    assert_eq!(names(&s), ["fn json_each:j@0", "users@0"]);
    // A quoted alias, a quoted name being typed.
    let s = at("SELECT \"T\".‸ FROM t AS \"T\"", PG);
    assert_eq!(s.qualified(&["T".into()]), [0]);
    let s = at("SELECT * FROM \"Us‸", PG);
    assert_eq!(s.prefix, "Us");
    assert!(s.quoted);
    assert_eq!(s.expect, Expect::Table { schema: None, join: false });
    // `schema.table.` qualifies a column.
    let s = at("SELECT public.users.‸ FROM public.users", PG);
    assert_eq!(s.expect, column(&["public", "users"]));
    assert_eq!(s.qualified(&["public".into(), "users".into()]), [0]);
}

#[test]
fn every_table_named_is_listed_for_its_columns() {
    let s = at(
        "WITH c AS (SELECT * FROM a) SELECT * FROM c JOIN b ON ‸ WHERE x IN (SELECT y FROM d)",
        LITE,
    );
    assert_eq!(s.named, [TableRef::new("a"), TableRef::new("b"), TableRef::new("d")]);
}

#[test]
fn merge_and_its_branches() {
    let sql = "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET ‸";
    let s = at(sql, PG);
    assert_eq!(s.expect, Expect::TableColumns { table: 0 });
    assert_eq!(names(&s), ["t@0", "s@0"]);
    let sql = "MERGE INTO t USING s ON ‸";
    assert_eq!(at(sql, PG).expect, Expect::JoinCondition { right: 1, left: vec![0] });
    let sql = "MERGE INTO t USING s ON t.id = s.id WHEN NOT MATCHED THEN INSERT (‸";
    assert_eq!(at(sql, PG).expect, Expect::InsertColumns { table: 0 });
}

#[test]
fn search_path_offers_schemas() {
    assert_eq!(at("SET search_path TO ‸", PG).expect, Expect::Schema);
}

#[test]
fn the_statement_under_the_cursor_is_the_one_read() {
    let sql = "SELECT * FROM a;\nSELECT ‸ FROM b;\nSELECT * FROM c";
    assert_eq!(names(&at(sql, LITE)), ["b@0"]);
    // Right after a statement without `;`, still it.
    let sql = "SELECT * FROM a x WHERE x.‸\n\nSELECT 1";
    assert_eq!(names(&at(sql, LITE)), ["a:x@0"]);
}

#[test]
fn nothing_panics_on_any_text() {
    use proptest::prelude::*;
    let config = proptest::test_runner::Config {
        cases: 600,
        rng_algorithm: proptest::test_runner::RngAlgorithm::ChaCha,
        ..Default::default()
    };
    let mut runner = proptest::test_runner::TestRunner::new_with_rng(
        config,
        proptest::test_runner::TestRng::from_seed(
            proptest::test_runner::RngAlgorithm::ChaCha,
            &[39; 32],
        ),
    );
    let words = prop::sample::select(vec![
        "SELECT", "FROM", "WHERE", "JOIN", "ON", "(", ")", ",", ".", "a", "b", "x", "WITH", "AS",
        "INSERT", "INTO", "VALUES", "UPDATE", "SET", "=", "*", "'s", "\"q", "--c\n", "UNION",
        "GROUP", "BY", "ORDER", "USING", "LATERAL", "DELETE", "CASE", "END", ";", "1", "$$",
    ]);
    runner
        .run(&(prop::collection::vec(words, 0..30), any::<prop::sample::Index>()), |(parts, at)| {
            let text = parts.join(" ");
            let offset = at.index(text.len() + 1);
            let offset = (0..=offset).rev().find(|&o| text.is_char_boundary(o)).unwrap_or(0);
            for flavor in [LITE, PG] {
                let tokens = lex(&text, flavor);
                let s = scope_in(&text, &tokens, offset, flavor);
                // Its indices point into its tables.
                match &s.expect {
                    Expect::JoinCondition { right, left }
                    | Expect::UsingColumns { right, left } => {
                        prop_assert!(*right < s.tables.len().max(1) || s.tables.is_empty());
                        prop_assert!(left.iter().all(|l| l < right));
                    }
                    Expect::InsertColumns { table } | Expect::TableColumns { table } => {
                        prop_assert!(*table < s.tables.len());
                    }
                    _ => {}
                }
                prop_assert!(s.replace.end <= text.len() && s.replace.start <= s.replace.end);
            }
            Ok(())
        })
        .unwrap();
}
