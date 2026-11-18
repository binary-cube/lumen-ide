use super::*;

const LITE: Flavor = Flavor::Sqlite;
const PG: Flavor = Flavor::Postgres { version: None };

fn fmt(text: &str, flavor: Flavor) -> String {
    format(text, flavor, &FormatOptions::default()).text
}

fn with(text: &str, flavor: Flavor, options: FormatOptions) -> String {
    format(text, flavor, &options).text
}

/// The tokens that aren't whitespace, a word's case aside (what a statement says).
fn said(text: &str, flavor: Flavor) -> Vec<(TokenKind, String)> {
    lex(text, flavor)
        .iter()
        .filter(|t| t.kind != TokenKind::Whitespace)
        .map(|t| {
            let raw = t.text(text);
            let raw =
                if t.kind == TokenKind::Word { raw.to_ascii_lowercase() } else { raw.to_owned() };
            (t.kind, raw)
        })
        .collect()
}

/// `input` laid out is `expected`, says the same, and laid out again doesn't change.
#[track_caller]
fn check(flavor: Flavor, input: &str, expected: &str) {
    let out = format(input, flavor, &FormatOptions::default());
    assert_eq!(out.text, expected, "laid out");
    assert_eq!(out.left, 0, "every statement laid out");
    assert_eq!(said(input, flavor), said(&out.text, flavor), "the same tokens");
    assert_eq!(fmt(&out.text, flavor), out.text, "idempotent");
}

// ---- golden cases: SQLite ----

#[test]
fn lite_select_where() {
    check(
        LITE,
        r#"select id, name from users where active = 1 and age >= 18"#,
        r#"SELECT id, name
FROM users
WHERE active = 1 AND age >= 18"#,
    );
}

#[test]
fn lite_join() {
    check(
        LITE,
        r#"select u.name, o.total from users u inner join orders o on o.user_id = u.id left join items i on i.order_id = o.id where o.total > 10 order by o.total desc limit 20"#,
        r#"SELECT u.name, o.total
FROM users u
INNER JOIN orders o ON o.user_id = u.id
LEFT JOIN items i ON i.order_id = o.id
WHERE o.total > 10
ORDER BY o.total DESC
LIMIT 20"#,
    );
}

#[test]
fn lite_group() {
    check(
        LITE,
        r#"select country, count(*) as n from customers group by country having count(*) > 5 order by n desc"#,
        r#"SELECT country, count(*) AS n
FROM customers
GROUP BY country
HAVING count(*) > 5
ORDER BY n DESC"#,
    );
}

#[test]
fn lite_insert_rows() {
    check(
        LITE,
        r#"insert into t (a, b) values (1, 'one'), (2, 'two'), (3, 'three')"#,
        r#"INSERT INTO t (a, b)
VALUES (1, 'one'), (2, 'two'), (3, 'three')"#,
    );
}

#[test]
fn lite_insert_or_replace() {
    check(
        LITE,
        r#"insert or replace into kv(key, value) values (?1, ?2)"#,
        r#"INSERT OR REPLACE INTO kv (key, value)
VALUES (?1, ?2)"#,
    );
}

#[test]
fn lite_upsert() {
    check(
        LITE,
        r#"insert into kv (k, v) values (:k, :v) on conflict(k) do update set v = excluded.v where kv.v <> excluded.v"#,
        r#"INSERT INTO kv (k, v)
VALUES (:k, :v)
ON CONFLICT (k) DO UPDATE
SET v = excluded.v
WHERE kv.v <> excluded.v"#,
    );
}

#[test]
fn lite_update() {
    check(
        LITE,
        r#"update users set name = @name, updated = $now where id = ?"#,
        r#"UPDATE users
SET name = @name, updated = $now
WHERE id = ?"#,
    );
}

#[test]
fn lite_delete() {
    check(
        LITE,
        r#"delete from sessions where expires < strftime('%s', 'now')"#,
        r#"DELETE FROM sessions
WHERE expires < strftime('%s', 'now')"#,
    );
}

#[test]
fn lite_create_table() {
    check(
        LITE,
        r#"create table if not exists [order items] (id integer primary key autoincrement, "order" integer not null references orders(id) on delete cascade, qty integer check (qty > 0), unique ("order", id)) without rowid"#,
        r#"CREATE TABLE IF NOT EXISTS [order items] (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    "order" INTEGER NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    qty INTEGER CHECK (qty > 0),
    UNIQUE ("order", id)
) WITHOUT ROWID"#,
    );
}

#[test]
fn lite_trigger() {
    check(
        LITE,
        r#"create trigger log_insert after insert on orders for each row when new.total > 0 begin insert into audit(what, at) values ('insert', datetime('now')); update stats set n = n + 1; end;"#,
        r#"CREATE TRIGGER log_insert AFTER INSERT ON orders FOR EACH ROW WHEN new.total > 0
BEGIN
    INSERT INTO audit (what, at)
    VALUES ('insert', datetime('now'));
    UPDATE stats
    SET n = n + 1;
END;"#,
    );
}

#[test]
fn lite_view() {
    check(
        LITE,
        r#"create view big_orders as select * from orders where total > 1000"#,
        r#"CREATE VIEW big_orders AS
SELECT *
FROM orders
WHERE total > 1000"#,
    );
}

#[test]
fn lite_index() {
    check(
        LITE,
        r#"create unique index idx_users_email on users(email) where email is not null"#,
        r#"CREATE UNIQUE INDEX idx_users_email ON users (email) WHERE email IS NOT NULL"#,
    );
}

#[test]
fn lite_cte() {
    check(
        LITE,
        r#"with recent as (select * from orders where created > date('now', '-7 days')), totals as (select user_id, sum(total) as s from recent group by user_id) select u.name, t.s from users u join totals t on t.user_id = u.id"#,
        r#"WITH recent AS (
    SELECT *
    FROM orders
    WHERE created > date('now', '-7 days')
),
totals AS (
    SELECT user_id, sum(total) AS s
    FROM recent
    GROUP BY user_id
)
SELECT u.name, t.s
FROM users u
JOIN totals t ON t.user_id = u.id"#,
    );
}

#[test]
fn lite_case() {
    check(
        LITE,
        r#"select id, case when total > 100 then 'big' when total > 10 then 'medium' else 'small' end as size from orders"#,
        r#"SELECT id, CASE WHEN total > 100 THEN 'big' WHEN total > 10 THEN 'medium' ELSE 'small' END AS size
FROM orders"#,
    );
}

#[test]
fn lite_window() {
    check(
        LITE,
        r#"select id, row_number() over (partition by user_id order by created desc) as rn from orders"#,
        r#"SELECT id, row_number() OVER (PARTITION BY user_id ORDER BY created DESC) AS rn
FROM orders"#,
    );
}

#[test]
fn lite_pragma() {
    check(
        LITE,
        r#"pragma foreign_keys = on; pragma table_info('users'); attach database 'other.db' as other;"#,
        r#"PRAGMA foreign_keys = ON;
PRAGMA table_info('users');
ATTACH DATABASE 'other.db' AS other;"#,
    );
}

#[test]
fn lite_explain() {
    check(
        LITE,
        r#"explain query plan select * from users where email = 'a@b.c'"#,
        r#"EXPLAIN QUERY PLAN
SELECT *
FROM users
WHERE email = 'a@b.c'"#,
    );
}

#[test]
fn lite_subqueries() {
    check(
        LITE,
        r#"select * from users where id in (select user_id from orders where total > 100) and not exists (select 1 from bans b where b.user_id = users.id)"#,
        r#"SELECT *
FROM users
WHERE id IN (SELECT user_id FROM orders WHERE total > 100)
    AND NOT EXISTS (SELECT 1 FROM bans b WHERE b.user_id = users.id)"#,
    );
}

#[test]
fn lite_union() {
    check(
        LITE,
        r#"select id from a union all select id from b except select id from c"#,
        r#"SELECT id
FROM a
UNION ALL
SELECT id
FROM b
EXCEPT
SELECT id
FROM c"#,
    );
}

// ---- golden cases: PostgreSQL ----

#[test]
fn pg_function() {
    check(
        PG,
        r#"create or replace function add_one(x integer) returns integer language plpgsql as $body$
begin
    -- keep as is
    return   x + 1;
end
$body$;"#,
        r#"CREATE OR REPLACE FUNCTION add_one(x integer) RETURNS INTEGER language plpgsql AS $body$
begin
    -- keep as is
    return   x + 1;
end
$body$;"#,
    );
}

#[test]
fn pg_casts() {
    check(
        PG,
        r#"select id::text, created_at::date, payload->>'name' as name, tags @> array['a'] as tagged from events where payload ? 'name' and created_at > now() - interval '1 day'"#,
        r#"SELECT id::TEXT, created_at::DATE, payload ->> 'name' AS name, tags @> ARRAY['a'] AS tagged
FROM events
WHERE payload ? 'name' AND created_at > now() - INTERVAL '1 day'"#,
    );
}

#[test]
fn pg_distinct_on() {
    check(
        PG,
        r#"select distinct on (user_id) user_id, created_at from logins order by user_id, created_at desc"#,
        r#"SELECT DISTINCT ON (user_id) user_id, created_at
FROM logins
ORDER BY user_id, created_at DESC"#,
    );
}

#[test]
fn pg_returning() {
    check(
        PG,
        r#"insert into users (email) values ($1) on conflict (email) do nothing returning id"#,
        r#"INSERT INTO users (email)
VALUES ($1)
ON CONFLICT (email) DO NOTHING
RETURNING id"#,
    );
}

#[test]
fn pg_lock() {
    check(
        PG,
        r#"select * from jobs where status = 'queued' order by priority limit 1 for update skip locked"#,
        r#"SELECT *
FROM jobs
WHERE status = 'queued'
ORDER BY priority
LIMIT 1
FOR UPDATE SKIP LOCKED"#,
    );
}

#[test]
fn pg_lateral() {
    check(
        PG,
        r#"select u.id, l.* from users u cross join lateral (select * from logins where user_id = u.id order by at desc limit 3) l"#,
        r#"SELECT u.id, l.*
FROM users u
CROSS JOIN LATERAL (SELECT * FROM logins WHERE user_id = u.id ORDER BY at DESC LIMIT 3) l"#,
    );
}

#[test]
fn pg_filter() {
    check(
        PG,
        r#"select count(*) filter (where ok) as good, string_agg(name, ', ' order by name) from t"#,
        r#"SELECT count(*) FILTER (WHERE ok) AS good, string_agg(name, ', ' ORDER BY name)
FROM t"#,
    );
}

#[test]
fn pg_comments() {
    check(
        PG,
        r#"/* header /* nested */ still */ select 1 -- one"#,
        r#"/* header /* nested */ still */
SELECT 1 -- one"#,
    );
}

#[test]
fn pg_escape() {
    check(
        PG,
        r#"select E'it\'s', U&'\0041', $$a;b$$, $x$ $$ $x$ from t"#,
        r#"SELECT E'it\'s', U&'\0041', $$a;b$$, $x$ $$ $x$
FROM t"#,
    );
}

#[test]
fn pg_do() {
    check(PG, r#"do $$ begin perform 1; end $$;"#, r#"DO $$ begin perform 1; end $$;"#);
}

#[test]
fn pg_atomic() {
    check(
        PG,
        r#"create function f() returns int language sql begin atomic select 1; select 2; end"#,
        r#"CREATE FUNCTION f() RETURNS INT language sql
BEGIN ATOMIC
    SELECT 1;
    SELECT 2;
END"#,
    );
}

#[test]
fn pg_continuation() {
    check(
        PG,
        r#"select 'one'
    'two' as s"#,
        r#"SELECT
    'one'
    'two' AS s"#,
    );
}

#[test]
fn pg_merge() {
    check(
        PG,
        r#"merge into stock s using deliveries d on s.item = d.item when matched then update set qty = s.qty + d.qty when not matched then insert (item, qty) values (d.item, d.qty)"#,
        r#"MERGE INTO stock s
USING deliveries d ON s.item = d.item
WHEN MATCHED THEN UPDATE
SET qty = s.qty + d.qty
WHEN NOT MATCHED THEN INSERT (item, qty)
VALUES (d.item, d.qty)"#,
    );
}

#[test]
fn pg_explain() {
    check(
        PG,
        r#"explain (analyze, buffers) select * from t where a = 1"#,
        r#"EXPLAIN (ANALYZE, buffers)
SELECT *
FROM t
WHERE a = 1"#,
    );
}

#[test]
fn pg_select_into() {
    check(
        PG,
        r#"select * into temp t2 from t where false"#,
        r#"SELECT *
INTO TEMP t2
FROM t
WHERE FALSE"#,
    );
}

#[test]
fn pg_alter() {
    check(
        PG,
        r#"alter table users add column nickname varchar(40), alter column name type text, drop column legacy"#,
        r#"ALTER TABLE users ADD COLUMN nickname VARCHAR(40), ALTER COLUMN name TYPE TEXT, DROP COLUMN legacy"#,
    );
}

#[test]
fn pg_long_list() {
    check(
        PG,
        r#"select id, first_name, last_name, email, phone, street, city, postal_code, country, created_at, updated_at, deleted_at from customers"#,
        r#"SELECT id, first_name, last_name, email, phone, street, city, postal_code, country, created_at, updated_at, deleted_at
FROM customers"#,
    );
}

#[test]
fn pg_long_condition() {
    check(
        PG,
        r#"select * from orders where status = 'paid' and total between 100 and 1000 and (country = 'RO' or country = 'MD' or country = 'BG') and created_at >= '2024-01-01' and deleted_at is null"#,
        r#"SELECT *
FROM orders
WHERE status = 'paid'
    AND total BETWEEN 100 AND 1000
    AND (country = 'RO' OR country = 'MD' OR country = 'BG')
    AND created_at >= '2024-01-01'
    AND deleted_at IS NULL"#,
    );
}

// ---- options ----

#[test]
fn keywords_are_written_in_the_case_chosen() {
    let text = "SELECT Id FROM Users WHERE Name LIKE 'A%' ORDER BY Id";
    let lower = FormatOptions { keyword_case: KeywordCase::Lower, ..FormatOptions::default() };
    assert_eq!(with(text, LITE, lower), "select Id\nfrom Users\nwhere Name like 'A%'\norder by Id");
    let keep = FormatOptions { keyword_case: KeywordCase::Preserve, ..FormatOptions::default() };
    let mixed = "select a FROM t Where b = 1";
    assert_eq!(with(mixed, LITE, keep), "select a\nFROM t\nWhere b = 1");
    // Names never: after a dot, a quoted one, a function's, a column named like a word
    // that's only a keyword somewhere else.
    assert_eq!(
        fmt("select t.key, \"select\", left(name, 2), key, action from t", PG),
        "SELECT t.key, \"select\", left(name, 2), key, action\nFROM t"
    );
    assert_eq!(
        fmt("create table t (id int, primary key (id))", PG),
        "CREATE TABLE t (\n    id INT,\n    PRIMARY KEY (id)\n)"
    );
}

#[test]
fn the_indentation_and_the_width_are_chosen() {
    let text = "select alpha, beta, gamma, delta from t where a = 1 and b = 2";
    let narrow = FormatOptions { width: 30, indent: 2, ..FormatOptions::default() };
    assert_eq!(
        with(text, LITE, narrow),
        "SELECT\n  alpha,\n  beta,\n  gamma,\n  delta\nFROM t\nWHERE a = 1 AND b = 2"
    );
    let narrower = FormatOptions { width: 20, indent: 2, ..FormatOptions::default() };
    assert_eq!(
        with(text, LITE, narrower),
        "SELECT\n  alpha,\n  beta,\n  gamma,\n  delta\nFROM t\nWHERE a = 1\n  AND b = 2"
    );
}

#[test]
fn commas_lead_when_chosen() {
    let leading =
        FormatOptions { commas: CommaStyle::Leading, width: 20, ..FormatOptions::default() };
    assert_eq!(
        with("select alpha, beta, gamma from t", LITE, leading),
        "SELECT\n    alpha\n    , beta\n    , gamma\nFROM t"
    );
    assert_eq!(
        with("with a as (select 1), b as (select 2) select 3", LITE, leading),
        "WITH a AS (\n    SELECT 1\n)\n, b AS (\n    SELECT 2\n)\nSELECT 3"
    );
    assert_eq!(
        with("create table t (a int, b int)", LITE, leading),
        "CREATE TABLE t (\n    a INT\n    , b INT\n)"
    );
}

#[test]
fn lists_go_one_item_a_line_when_chosen() {
    let one = FormatOptions { lists: ListStyle::OnePerLine, ..FormatOptions::default() };
    assert_eq!(
        with("select a, b from t group by a, b", LITE, one),
        "SELECT\n    a,\n    b\nFROM t\nGROUP BY\n    a,\n    b"
    );
    // A single item stays on its clause's line.
    assert_eq!(with("select a from t", LITE, one), "SELECT a\nFROM t");
}

#[test]
fn a_long_values_list_breaks_one_row_a_line() {
    let rows: Vec<String> = (0..12).map(|i| format!("({i}, 'name number {i}')")).collect();
    let text = format!("insert into t (id, name) values {}", rows.join(", "));
    let out = fmt(&text, LITE);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "INSERT INTO t (id, name)");
    assert_eq!(lines[1], "VALUES");
    assert_eq!(lines[2], "    (0, 'name number 0'),");
    assert_eq!(lines.last(), Some(&"    (11, 'name number 11')"));
    assert_eq!(lines.len(), 14);
}

#[test]
fn a_long_subquery_and_case_break_indented() {
    let text = "select id, (select max(created_at) from orders o where o.user_id = u.id and o.status = 'paid' and o.total > 100) as last_paid, case when u.vip then 'gold' when u.points > 1000 then 'silver' else 'bronze' end as tier from users u";
    let out = fmt(text, PG);
    assert_eq!(
        out,
        "SELECT
    id,
    (SELECT max(created_at) FROM orders o WHERE o.user_id = u.id AND o.status = 'paid' AND o.total > 100) AS last_paid,
    CASE WHEN u.vip THEN 'gold' WHEN u.points > 1000 THEN 'silver' ELSE 'bronze' END AS tier
FROM users u"
    );
    let narrow = FormatOptions { width: 60, ..FormatOptions::default() };
    assert_eq!(
        with(text, PG, narrow),
        "SELECT
    id,
    (
        SELECT max(created_at)
        FROM orders o
        WHERE o.user_id = u.id
            AND o.status = 'paid'
            AND o.total > 100
    ) AS last_paid,
    CASE
        WHEN u.vip THEN 'gold'
        WHEN u.points > 1000 THEN 'silver'
        ELSE 'bronze'
    END AS tier
FROM users u"
    );
}

#[test]
fn a_join_s_condition_goes_under_it_when_too_long() {
    let narrow = FormatOptions { width: 50, ..FormatOptions::default() };
    assert_eq!(
        with(
            "select * from a join b on b.a_id = a.id and b.kind = 'x' and b.deleted is null",
            LITE,
            narrow
        ),
        "SELECT *\nFROM a\nJOIN b\n    ON b.a_id = a.id\n        AND b.kind = 'x'\n        AND b.deleted IS NULL"
    );
}

// ---- comments, blank lines, line ends ----

#[test]
fn comments_stay_where_they_are() {
    check(
        LITE,
        "select a, -- the a\n  b /* the b */\nfrom t -- the table\n-- why\nwhere x = 1",
        "SELECT\n    a, -- the a\n    b /* the b */\nFROM t -- the table\n-- why\nWHERE x = 1",
    );
    // After a line comment, the comma starts the next line.
    check(LITE, "select a -- one\n, b from t", "SELECT\n    a -- one\n    , b\nFROM t");
    // A comment before a `)`, inside a call, in a CTE.
    check(
        PG,
        "with x as ( -- the source\n select 1 ) select f(a, -- first\n b) from x",
        "WITH x AS ( -- the source\n    SELECT 1\n)\nSELECT\n    f(\n        a, -- first\n        b\n    )\nFROM x",
    );
}

#[test]
fn comments_between_statements_and_after_them_are_kept() {
    check(
        LITE,
        "-- first\nselect 1; -- after it\n\n\n\n/* block */\nselect 2;\n-- the end",
        "-- first\nSELECT 1; -- after it\n\n/* block */\nSELECT 2;\n-- the end",
    );
    // Nothing but comments.
    check(LITE, "-- a\n/* b */", "-- a\n/* b */");
    // A multi-line block comment byte for byte.
    check(PG, "select 1 /* a\n      b\n   */ from t", "SELECT\n    1 /* a\n      b\n   */\nFROM t");
}

#[test]
fn blank_lines_between_statements_stay_one_and_the_last_line_break_stays() {
    assert_eq!(fmt("select 1;\n\n\nselect 2;\n", LITE), "SELECT 1;\n\nSELECT 2;\n");
    assert_eq!(fmt("select 1; select 2", LITE), "SELECT 1;\nSELECT 2");
    assert_eq!(fmt("\n\n  select 1\n\n", LITE), "SELECT 1\n");
    // A stray `;` stays after its statement.
    assert_eq!(fmt("select 1;;\n;", LITE), "SELECT 1;;\n;");
}

#[test]
fn lines_end_as_the_text_s_do() {
    assert_eq!(fmt("select a\r\nfrom t\r\nwhere b\r\n", LITE), "SELECT a\r\nFROM t\r\nWHERE b\r\n");
    assert_eq!(fmt("select 1;\r\n\r\n\r\nselect 2", LITE), "SELECT 1;\r\n\r\nSELECT 2");
}

#[test]
fn whitespace_only_text_is_left_alone() {
    assert_eq!(fmt("", LITE), "");
    assert_eq!(fmt("  \n\t", LITE), "  \n\t");
}

// ---- what's never changed ----

#[test]
fn strings_bodies_and_quoted_names_come_out_byte_for_byte() {
    let text =
        "select 'a  b', \"Mixed  Name\", $tag$\n  select   1;\n$tag$, E'x\\n  y', x'00ff' from t";
    let out = fmt(text, PG);
    for piece in
        ["'a  b'", "\"Mixed  Name\"", "$tag$\n  select   1;\n$tag$", "E'x\\n  y'", "x'00ff'"]
    {
        assert!(out.contains(piece), "{piece} in {out}");
    }
    let lite = fmt("select [my  col], `other  col` from [my  table]", LITE);
    assert_eq!(lite, "SELECT [my  col], `other  col`\nFROM [my  table]");
}

#[test]
fn unfinished_statements_are_left_as_written() {
    let out = format("select  a from t where b = 'open", LITE, &FormatOptions::default());
    assert_eq!(out.text, "select  a from t where b = 'open");
    assert_eq!(out.left, 1);
    let out = format("select (a,  b from t;\nselect 2", LITE, &FormatOptions::default());
    assert_eq!(out.left, 1);
    assert!(out.text.starts_with("select (a,  b from t;"), "{}", out.text);
    // The finished ones around it are laid out.
    let out = format("select 1;\nselect  'x", LITE, &FormatOptions::default());
    assert_eq!((out.text.as_str(), out.left), ("SELECT 1;\nselect  'x", 1));
}

#[test]
fn what_could_change_meaning_is_left_as_written() {
    // A psql command ends at its line.
    let text = "select 1 \\gset\nselect  2";
    let out = format(text, PG, &FormatOptions::default());
    assert_eq!(out.left, 1);
    assert!(out.text.contains("select 1 \\gset"));
    // Too deep to lay out.
    let deep = format!("select {}1{}", "(".repeat(150), ")".repeat(150));
    let out = format(&deep, LITE, &FormatOptions::default());
    assert_eq!((out.text, out.left), (deep, 1));
}

#[test]
fn separator_lines_stay_on_their_lines() {
    check(LITE, "select 1\nGO\nselect  2\n/\n", "SELECT 1\nGO\nSELECT 2\n/\n");
    // A column named `go` never ends up alone on a line where it would separate.
    let narrow = FormatOptions { width: 20, ..FormatOptions::default() };
    let text = "select alpha, beta, go from t";
    let out = format(text, LITE, &narrow);
    assert!(same_meaning(text, &out.text, LITE));
    assert!(!out.text.lines().any(|l| l.trim().eq_ignore_ascii_case("go")), "{}", out.text);
}

#[test]
fn signs_casts_and_operators_keep_their_tokens() {
    check(
        PG,
        "select -1, - x, a - -1, +2, a-b, x::int, arr[1], j->'k', 1.5e-3 from t where a<>b and c>=d",
        "SELECT -1, - x, a - -1, +2, a - b, x::INT, arr[1], j -> 'k', 1.5e-3\nFROM t\nWHERE a <> b AND c >= d",
    );
    // Glued in the text, the lexer read one operator: it stays one.
    check(LITE, "select a=-1 from t", "SELECT a =- 1\nFROM t");
    // Never two dashes: they'd start a comment.
    check(LITE, "select 1 - - 1", "SELECT 1 - - 1");
}

#[test]
fn postgres_s_adjacent_strings_keep_their_line_break() {
    // `'a' 'b'` on one line is an error, on two lines the strings join.
    let out = fmt("select 'a'\n'b'", PG);
    assert!(out.contains("'a'\n") && out.trim_end().ends_with("'b'"), "{out}");
    assert_eq!(fmt("select 'a' || 'b'", PG), "SELECT 'a' || 'b'");
}

// ---- a part of the text ----

#[test]
fn the_statement_at_the_cursor_is_laid_out_alone() {
    let text = "select 1;\nselect   a,b from t;\nselect 3;";
    let at = text.find("a,b").unwrap();
    let edit = format_at(text, at, LITE, &FormatOptions::default()).unwrap();
    assert_eq!(&text[edit.range.clone()], "select   a,b from t;");
    assert_eq!(edit.text, "SELECT a, b\nFROM t;");
    // In the blank lines between statements far from both: nothing.
    let text = "select 1;\n\n\n\nselect 2;";
    assert!(format_at(text, 12, LITE, &FormatOptions::default()).is_none());
}

#[test]
fn a_selection_lays_out_the_statements_it_touches() {
    let text = "select 1;\nselect a from t where b;\nselect c from u;\nselect 4;";
    let from = text.find("where").unwrap();
    let to = text.find("from u").unwrap();
    let edit = format_range(text, from..to, LITE, &FormatOptions::default()).unwrap();
    assert_eq!(&text[edit.range.clone()], "select a from t where b;\nselect c from u;");
    assert_eq!(edit.text, "SELECT a\nFROM t\nWHERE b;\nSELECT c\nFROM u;");
}

#[test]
fn an_indented_statement_keeps_its_indentation() {
    let text = "-- in a block\n    select a from t where b";
    let edit = format_at(text, text.len(), LITE, &FormatOptions::default()).unwrap();
    assert_eq!(edit.text, "SELECT a\n    FROM t\n    WHERE b");
    let mut again = text.to_owned();
    again.replace_range(edit.range.clone(), &edit.text);
    let second = format_at(&again, again.len(), LITE, &FormatOptions::default()).unwrap();
    assert_eq!(second.text, edit.text, "idempotent in place");
}

// ---- properties ----

/// Random texts of SQL's pieces (words, names, strings, comments, bodies, parentheses,
/// separators, line breaks) on every flavor and with random options: laid out, they say
/// the same, and laying them out again changes nothing.
#[test]
fn laying_out_is_idempotent_and_keeps_every_token() {
    use proptest::prelude::*;
    let config = proptest::test_runner::Config {
        cases: 700,
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
    let pieces = prop::sample::select(vec![
        "SELECT",
        "select",
        "FROM",
        "WHERE",
        "AND",
        "OR",
        "JOIN",
        "LEFT",
        "ON",
        "USING",
        "GROUP",
        "BY",
        "ORDER",
        "HAVING",
        "LIMIT",
        "WITH",
        "AS",
        "INSERT",
        "INTO",
        "VALUES",
        "UPDATE",
        "SET",
        "DELETE",
        "UNION",
        "ALL",
        "CASE",
        "WHEN",
        "THEN",
        "ELSE",
        "END",
        "BETWEEN",
        "IN",
        "EXISTS",
        "NOT",
        "NULL",
        "CREATE",
        "TABLE",
        "TRIGGER",
        "BEGIN",
        "ATOMIC",
        "DISTINCT",
        "OVER",
        "PARTITION",
        "FILTER",
        "ON CONFLICT",
        "DO",
        "RETURNING",
        "a",
        "b",
        "t",
        "u.id",
        "count",
        "f",
        "go",
        "GO",
        "(",
        ")",
        "(",
        ")",
        ",",
        ",",
        ".",
        ";",
        "*",
        "=",
        "-",
        "+",
        "<>",
        "::",
        "1",
        "2.5",
        "'s'",
        "'it''s'",
        "\"Q\"",
        "[x]",
        "`y`",
        "?",
        "$1",
        ":p",
        "@v",
        "-- c\n",
        "/* b */",
        "/* m\n l */",
        "$$ x; $$",
        "$t$ $$ $t$",
        "E'\\''",
        "\n",
        "\n\n",
        " ",
        "\t",
        "\\",
        "[",
        "]",
        "'open",
        "/*open",
    ]);
    let options = (0..3usize, 1..9usize, 20..140usize, any::<bool>(), any::<bool>());
    runner
        .run(
            &(prop::collection::vec(pieces, 0..60), options, 0..3usize),
            |(parts, (case, indent, width, leading, one), flavor)| {
                let text = parts.join(" ");
                let flavor = [Flavor::Generic, LITE, PG][flavor];
                let options = FormatOptions {
                    keyword_case: [KeywordCase::Upper, KeywordCase::Lower, KeywordCase::Preserve]
                        [case],
                    indent,
                    width,
                    commas: if leading { CommaStyle::Leading } else { CommaStyle::Trailing },
                    lists: if one { ListStyle::OnePerLine } else { ListStyle::Fit },
                };
                let once = format(&text, flavor, &options);
                prop_assert_eq!(said(&text, flavor), said(&once.text, flavor));
                prop_assert!(same_meaning(&text, &once.text, flavor));
                let twice = format(&once.text, flavor, &options);
                prop_assert_eq!(&twice.text, &once.text, "idempotent on {:?}", text);
                // A part of it (at a random place) is laid out by the same rules.
                if let Some(edit) = format_at(&text, text.len() / 2, flavor, &options) {
                    let mut changed = text.clone();
                    changed.replace_range(edit.range.clone(), &edit.text);
                    prop_assert!(same_meaning(&text, &changed, flavor));
                }
                Ok(())
            },
        )
        .unwrap();
}

/// The layout doesn't depend on how the text was spaced: the same tokens spaced at random
/// (spaces, tabs, line breaks — not where a break means something: around a comment,
/// between two strings) give the same text.
#[test]
fn the_layout_does_not_depend_on_the_spacing() {
    use proptest::prelude::*;
    let texts: [(&str, Flavor); 10] = [
        (
            "select u.name, o.total from users u inner join orders o on o.user_id = u.id where o.total > 10 and u.active order by 2 desc limit 5",
            LITE,
        ),
        (
            "with recent as (select * from orders where created > date('now', '-7 days')) select count(*) from recent",
            LITE,
        ),
        (
            "insert into t (a, b) values (1, 'one'), (2, 'two') on conflict (a) do update set b = excluded.b",
            LITE,
        ),
        (
            "create table if not exists t (id integer primary key, name text not null, check (length(name) > 0)) without rowid",
            LITE,
        ),
        (
            "create trigger tr after insert on t for each row begin update u set n = n + 1 where id = new.id; delete from v; end",
            LITE,
        ),
        (
            "select id, case when total > 100 then 'big' when total > 10 then 'medium' else 'small' end as size, -1 from orders",
            LITE,
        ),
        (
            "select distinct on (a) a, b::text, c->>'k' from t where x between 1 and 2 or y is not distinct from z for update skip locked",
            PG,
        ),
        (
            "select * from a where exists (select 1 from b where b.a = a.id and b.k in (1, 2, 3)) union all select * from c",
            PG,
        ),
        (
            "create or replace function f(x int) returns int language plpgsql as $$ begin return x; end $$",
            PG,
        ),
        (
            "update accounts set balance = balance - 100, note = 'a long note to make it break', version = version + 1 where id = 42",
            PG,
        ),
    ];
    let config = proptest::test_runner::Config {
        cases: 200,
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
    let spaces = prop::collection::vec(
        prop::sample::select(vec![" ", "\n", "\t", "  \n    ", "\r\n "]),
        200,
    );
    let widths = prop::sample::select(vec![30usize, 60, 120]);
    runner
        .run(&(0..texts.len(), spaces, widths), |(which, spaces, width)| {
            let (text, flavor) = texts[which];
            let options = FormatOptions { width, ..FormatOptions::default() };
            let tokens = lex(text, flavor);
            let mut respaced = String::new();
            for (i, t) in tokens.iter().enumerate() {
                let fixed = t.kind != TokenKind::Whitespace
                    || tokens.get(i + 1).is_some_and(|n| n.is_comment())
                    || i.checked_sub(1).is_some_and(|p| tokens[p].is_comment());
                if fixed {
                    respaced.push_str(t.text(text));
                } else {
                    respaced.push_str(spaces[i % spaces.len()]);
                }
            }
            // `\r\n` among them: the text's lines end so.
            let expected = format(text, flavor, &options).text;
            let expected =
                if respaced.contains("\r\n") { expected.replace('\n', "\r\n") } else { expected };
            prop_assert_eq!(format(&respaced, flavor, &options).text, expected, "{:?}", respaced);
            Ok(())
        })
        .unwrap();
}

#[test]
fn the_golden_outputs_are_fixed_points_in_every_option() {
    let texts = [
        "select a, b from t where x = 1 and y in (select z from u) order by a",
        "with r as (select 1) insert into t (a) select * from r on conflict do nothing",
        "create table t (a int primary key, b text not null, check (a > 0))",
        "select case when a then b else c end, count(*) filter (where d) from t group by 1",
    ];
    for text in texts {
        for case in [KeywordCase::Upper, KeywordCase::Lower] {
            for commas in [CommaStyle::Trailing, CommaStyle::Leading] {
                for lists in [ListStyle::Fit, ListStyle::OnePerLine] {
                    for width in [20, 60, 120] {
                        let options =
                            FormatOptions { keyword_case: case, commas, lists, width, indent: 2 };
                        let once = with(text, PG, options);
                        assert_eq!(with(&once, PG, options), once, "{text} with {options:?}");
                        assert_eq!(said(text, PG), said(&once, PG));
                    }
                }
            }
        }
    }
}

// ---- budgets ----

/// Generous in a debug build; a release build is many times faster.
fn budget(ms: u64) -> std::time::Duration {
    let factor = if cfg!(debug_assertions) { 10 } else { 1 };
    std::time::Duration::from_millis(ms * factor)
}

#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn a_huge_insert_is_laid_out_in_time() {
    // Over 1 MB: 26 000 rows.
    let rows: Vec<String> =
        (0..26_000).map(|i| format!("({i}, 'customer number {i}', {}.5, null)", i % 97)).collect();
    let text =
        format!("insert into customers (id, name, balance, note) values {};", rows.join(","));
    assert!(text.len() > 1_000_000);
    let start = std::time::Instant::now();
    let out = format(&text, LITE, &FormatOptions::default());
    let took = start.elapsed();
    assert_eq!(out.left, 0);
    assert_eq!(out.text.lines().count(), 26_002);
    assert!(took < budget(400), "took {took:?}");
}

#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn a_very_wide_select_and_many_statements_are_laid_out_in_time() {
    let columns: Vec<String> = (0..5_000).map(|i| format!("t.column_{i} as c{i}")).collect();
    let text = format!("select {} from t where a = 1", columns.join(", "));
    let start = std::time::Instant::now();
    let out = format(&text, PG, &FormatOptions::default());
    assert_eq!(out.left, 0);
    assert_eq!(out.text.lines().count(), 5_003);
    let statements: String = (0..5_000)
        .map(|i| format!("update t set a = {i}, b = 'x' where id = {i} and c in (1, 2, 3);\n"))
        .collect();
    let out = format(&statements, PG, &FormatOptions::default());
    assert_eq!(out.left, 0);
    let took = start.elapsed();
    assert!(took < budget(400), "took {took:?}");
}

#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn deep_nesting_is_laid_out_in_time() {
    let text = format!(
        "select {}1{} from t where {}",
        "coalesce(".repeat(90),
        ")".repeat(90),
        (0..2_000).map(|i| format!("(a{i} = {i} or b{i} = {i})")).collect::<Vec<_>>().join(" and ")
    );
    let start = std::time::Instant::now();
    let out = format(&text, LITE, &FormatOptions::default());
    assert_eq!(out.left, 0);
    assert_eq!(fmt(&out.text, LITE), out.text);
    let took = start.elapsed();
    assert!(took < budget(300), "took {took:?}");
}

#[test]
fn the_word_lists_are_sorted_uppercase() {
    for list in words::lists().into_iter().chain([words::after_words().leak() as &[&str]]) {
        assert!(list.windows(2).all(|w| w[0] < w[1]), "{list:?}");
        assert!(list.iter().all(|w| w.bytes().all(|b| !b.is_ascii_lowercase())));
    }
}
