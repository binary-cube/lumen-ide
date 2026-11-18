use super::*;
use crate::TableInfo;
use crate::relations::VirtualKey;
use crate::schema_index::IndexParts;
use crate::schema_index::tests::{big_index, shop, shop_loaded};
use crate::sql::{lex, scope_in};

const LITE: Flavor = Flavor::Sqlite;
const PG: Flavor = Flavor::Postgres { version: Some(170_000) };

/// The suggestions at `‸` in `sql`.
fn suggest_with(
    sql: &str,
    index: Option<&SchemaIndex>,
    flavor: Flavor,
    schema: Option<&str>,
) -> Completion {
    let offset = sql.find('‸').expect("a cursor");
    let text = sql.replace('‸', "");
    let scope = scope_in(&text, &lex(&text, flavor), offset, flavor);
    complete(&CompletionRequest { scope: &scope, index, flavor, schema, limit: LIMIT })
}

fn suggest(sql: &str) -> Completion {
    let index = SchemaIndex::build(shop(true));
    suggest_with(sql, Some(&index), LITE, None)
}

fn labels(c: &Completion) -> Vec<&str> {
    c.items.iter().map(|i| i.label.as_str()).collect()
}

fn inserts(c: &Completion) -> Vec<&str> {
    c.items.iter().map(|i| i.insert.as_str()).collect()
}

fn of_kind(c: &Completion, kind: SuggestionKind) -> Vec<&str> {
    c.items.iter().filter(|i| i.kind == kind).map(|i| i.label.as_str()).collect()
}

#[test]
fn an_alias_dot_lists_its_table_s_columns_in_order() {
    let c = suggest("select * from events e where e.‸");
    assert_eq!(labels(&c), ["id", "kind", "at", "user_id"]);
    assert!(c.items.iter().all(|i| i.kind == SuggestionKind::Column));
    assert_eq!(c.items[1].detail.as_deref(), Some("events e · TEXT"));
    // What's typed filters them, the prefix first.
    let c = suggest("select * from events e where e.k‸");
    assert_eq!(labels(&c)[0], "kind");
    // The select list, before its FROM.
    let c = suggest("SELECT o.‸ FROM orders o");
    assert_eq!(labels(&c), ["id", "user_id", "total", "created_at"]);
    // A table not in scope, by its name.
    let c = suggest("SELECT users.‸");
    assert_eq!(labels(&c), ["id", "name", "email"]);
}

#[test]
fn unqualified_columns_come_from_every_table_in_scope_the_ambiguous_qualified() {
    let c = suggest("SELECT ‸ FROM orders o JOIN users u ON u.id = o.user_id");
    let columns: Vec<(&str, &str)> = c
        .items
        .iter()
        .filter(|i| i.kind == SuggestionKind::Column)
        .map(|i| (i.label.as_str(), i.insert.as_str()))
        .collect();
    assert_eq!(
        columns,
        [
            ("o.id", "o.id"),
            ("user_id", "user_id"),
            ("total", "total"),
            ("created_at", "created_at"),
            ("u.id", "u.id"),
            ("name", "name"),
            ("email", "email"),
        ]
    );
    // The tables' aliases too (to qualify with), and the expression's words, after.
    assert!(of_kind(&c, SuggestionKind::Alias).contains(&"o"));
    assert!(of_kind(&c, SuggestionKind::Keyword).contains(&"CASE"));
    let first_keyword = c.items.iter().position(|i| i.kind == SuggestionKind::Keyword).unwrap();
    let last_column = c.items.iter().rposition(|i| i.kind == SuggestionKind::Column).unwrap();
    assert!(last_column < first_keyword);
    // Typed: the matches, prefixes first.
    let c = suggest("SELECT * FROM orders o JOIN users u ON u.id = o.user_id WHERE to‸");
    assert_eq!(c.items[0].label, "total");
    // The functions too, once something is typed.
    let c = suggest("SELECT COU‸ FROM orders");
    assert_eq!(c.items[0].label, "COUNT");
    assert_eq!(c.items[0].insert, "COUNT()");
    assert_eq!(c.items[0].cursor_back, 1);
    assert_eq!(c.items[0].detail.as_deref(), Some("count(expression) → integer"));
    let c = suggest("SELECT STRF‸ FROM orders");
    assert_eq!(c.items[0].label, "STRFTIME");
    assert_eq!(c.items[0].detail.as_deref(), Some("strftime(format, time, modifier, …) → text"));
    let c = suggest("select cou‸ from orders");
    assert_eq!(c.items[0].label, "count", "as the word typed is written");
}

#[test]
fn the_outer_query_s_columns_are_seen_in_a_subquery() {
    let c =
        suggest("SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = ‸)");
    let columns = of_kind(&c, SuggestionKind::Column);
    assert!(columns.contains(&"total"), "the inner table's");
    assert!(columns.contains(&"email"), "the outer table's");
    let total = columns.iter().position(|c| *c == "total").unwrap();
    let email = columns.iter().position(|c| *c == "email").unwrap();
    assert!(total < email, "the nearer first");
    // The inner's don't leak out.
    let c = suggest("SELECT ‸ FROM users u WHERE u.id IN (SELECT user_id FROM orders o)");
    assert!(!of_kind(&c, SuggestionKind::Column).contains(&"total"));
}

#[test]
fn join_offers_the_related_tables_first_with_their_condition() {
    let c = suggest("select * from orders o join ‸");
    let joins = of_kind(&c, SuggestionKind::Join);
    assert_eq!(joins[..2], ["users u ON u.id = o.user_id", "order_items oi ON oi.order_id = o.id"]);
    assert!(c.items[0].kind == SuggestionKind::Join && !c.items[0].inferred);
    assert_eq!(c.items[0].detail.as_deref(), Some("orders.user_id → users.id"));
    // Then every table, as after FROM.
    let tables = of_kind(&c, SuggestionKind::Table);
    assert!(tables.contains(&"products") && tables.contains(&"users"));
    let first_table = c.items.iter().position(|i| i.kind == SuggestionKind::Table).unwrap();
    let last_join = c.items.iter().rposition(|i| i.kind == SuggestionKind::Join).unwrap();
    assert!(last_join < first_table);
    // Typing filters by the table's name.
    let c = suggest("select * from orders o join us‸");
    assert_eq!(c.items[0].label, "users u ON u.id = o.user_id");
    // The alias isn't one already taken.
    let c = suggest("select * from orders u join ‸");
    assert_eq!(c.items[0].label, "users u1 ON u1.id = u.user_id");
}

#[test]
fn inferred_relations_come_last_and_marked() {
    // `events.user_id` points to users only by its name.
    let c = suggest("SELECT * FROM users u JOIN ‸");
    let joins: Vec<&Suggestion> =
        c.items.iter().filter(|i| i.kind == SuggestionKind::Join).collect();
    assert_eq!(joins[0].label, "orders o ON o.user_id = u.id");
    assert!(!joins[0].inferred);
    let events = joins.iter().find(|j| j.label.starts_with("events")).expect("inferred");
    assert_eq!(events.label, "events e ON e.user_id = u.id");
    assert!(events.inferred);
    assert!(joins.iter().position(|j| j.inferred).unwrap() > 0);
    // A tree: its parent.
    let c = suggest("SELECT * FROM categories c JOIN ‸");
    assert_eq!(c.items[0].label, "categories c1 ON c1.id = c.parent_id");
    assert!(c.items[0].inferred);
    // One accepted is a key; one set aside isn't offered.
    let mut parts = shop(true);
    parts.notes.accept(VirtualKey {
        table: TableRef::in_schema("main", "events"),
        columns: vec!["user_id".into()],
        target: TableRef::in_schema("main", "users"),
        references: vec!["id".into()],
    });
    parts.notes.ignore(VirtualKey {
        table: TableRef::in_schema("main", "order_items"),
        columns: vec!["product_id".into()],
        target: TableRef::in_schema("main", "products"),
        references: vec!["id".into()],
    });
    let index = SchemaIndex::build(parts);
    let c = suggest_with("SELECT * FROM users u JOIN ‸", Some(&index), LITE, None);
    let events = c.items.iter().find(|j| j.label.starts_with("events e ON")).unwrap();
    assert!(!events.inferred, "accepted");
    let c = suggest_with("SELECT * FROM order_items oi JOIN ‸", Some(&index), LITE, None);
    assert!(!c.items.iter().any(|j| j.label.starts_with("products p ON")), "set aside");
}

#[test]
fn join_on_offers_whole_conditions_then_columns() {
    let c = suggest("select * from orders o join users u on ‸");
    assert_eq!(c.items[0].label, "u.id = o.user_id");
    assert_eq!(c.items[0].kind, SuggestionKind::Join);
    assert!(of_kind(&c, SuggestionKind::Column).contains(&"email"), "the columns after");
    // Several tables before: each one's.
    let c =
        suggest("SELECT * FROM users u JOIN orders o ON o.user_id = u.id JOIN order_items i ON ‸");
    assert_eq!(c.items[0].label, "i.order_id = o.id");
    // A composite key: its parts joined.
    let parts = IndexParts {
        default_schema: Some("main".into()),
        tables: vec![
            TableInfo { table: TableRef::new("a"), kind: TableKind::Table, rows: None },
            TableInfo { table: TableRef::new("b"), kind: TableKind::Table, rows: None },
        ],
        columns: vec![
            (TableRef::new("a"), vec![brief("x", Some(1)), brief("y", Some(2))]),
            (TableRef::new("b"), vec![brief("ax", None), brief("ay", None)]),
        ],
        keys: vec![(
            TableRef::new("b"),
            vec![crate::ForeignKey {
                name: None,
                columns: vec!["ax".into(), "ay".into()],
                table: TableRef::new("a"),
                references: vec!["x".into(), "y".into()],
                on_update: crate::FkAction::NoAction,
                on_delete: crate::FkAction::NoAction,
            }],
        )],
        complete: true,
        ..IndexParts::default()
    };
    let index = SchemaIndex::build(parts);
    let c = suggest_with("SELECT * FROM a JOIN b ON ‸", Some(&index), LITE, None);
    assert_eq!(c.items[0].label, "b.ax = a.x AND b.ay = a.y");
}

fn brief(name: &str, key: Option<u16>) -> crate::ColumnBrief {
    crate::ColumnBrief {
        name: name.into(),
        type_name: "INTEGER".into(),
        key_position: key,
        ..Default::default()
    }
}

#[test]
fn tables_of_the_current_schema_come_first_then_the_others_qualified() {
    let parts = IndexParts {
        default_schema: Some("public".into()),
        schemas: vec!["billing".into(), "public".into()],
        tables: vec![
            TableInfo {
                table: TableRef::in_schema("billing", "invoices"),
                kind: TableKind::Table,
                rows: None,
            },
            TableInfo {
                table: TableRef::in_schema("public", "orders"),
                kind: TableKind::Table,
                rows: None,
            },
            TableInfo {
                table: TableRef::in_schema("public", "Order Lines"),
                kind: TableKind::Table,
                rows: None,
            },
            TableInfo {
                table: TableRef::in_schema("public", "recent"),
                kind: TableKind::View,
                rows: None,
            },
        ],
        complete: true,
        ..IndexParts::default()
    };
    let index = SchemaIndex::build(parts);
    let c = suggest_with("SELECT * FROM ‸", Some(&index), PG, None);
    assert_eq!(
        labels(&c),
        ["Order Lines", "orders", "recent", "billing", "billing.invoices"],
        "the current schema's, the other schemas, their tables"
    );
    assert_eq!(c.items[0].insert, "\"Order Lines\"", "quoted where it must be");
    assert_eq!(c.items[0].detail.as_deref(), Some("public"));
    assert_eq!(c.items[2].kind, SuggestionKind::View);
    assert_eq!(c.items[3].kind, SuggestionKind::Schema);
    assert_eq!(c.items[3].insert, "billing.");
    assert_eq!(c.items[4].insert, "billing.invoices");
    // From another schema chosen: that one first.
    let c = suggest_with("SELECT * FROM ‸", Some(&index), PG, Some("billing"));
    assert_eq!(c.items[0].label, "invoices");
    assert_eq!(c.items[0].insert, "invoices");
    // `schema.|`: its tables, unqualified.
    let c = suggest_with("SELECT * FROM billing.‸", Some(&index), PG, None);
    assert_eq!(labels(&c), ["invoices"]);
    assert_eq!(inserts(&c), ["invoices"]);
    // The statement's CTEs first.
    let c = suggest_with("WITH mine AS (SELECT 1) SELECT * FROM ‸", Some(&index), PG, None);
    assert_eq!(c.items[0].label, "mine");
    assert_eq!(c.items[0].kind, SuggestionKind::Cte);
}

#[test]
fn keywords_fit_the_place_and_the_case_typed() {
    let c = suggest("SELECT * FROM users u ‸");
    let words = of_kind(&c, SuggestionKind::Keyword);
    assert_eq!(words[..3], ["WHERE", "JOIN", "INNER JOIN"]);
    assert_eq!(c.items[0].insert, "WHERE ");
    let c = suggest("select * from users u wh‸");
    assert_eq!(c.items[0].label, "where");
    assert_eq!(c.items[0].insert, "where ");
    let c = suggest("SELECT * FROM users u JOIN orders o ‸");
    assert_eq!(c.items[0].label, "ON");
    let c = suggest("‸");
    assert_eq!(c.items[0].label, "SELECT");
    // A dialect's own words only.
    let pg = suggest_with("SELECT * FROM t WHERE a ‸", None, PG, None);
    assert!(of_kind(&pg, SuggestionKind::Keyword).contains(&"ILIKE"));
    assert!(!of_kind(&pg, SuggestionKind::Keyword).contains(&"GLOB"));
    let lite = suggest_with("SELECT * FROM t WHERE a ‸", None, LITE, None);
    assert!(of_kind(&lite, SuggestionKind::Keyword).contains(&"GLOB"));
    assert!(!of_kind(&lite, SuggestionKind::Keyword).contains(&"ILIKE"));
    assert!(
        !of_kind(&suggest_with("‸", None, PG, None), SuggestionKind::Keyword).contains(&"PRAGMA")
    );
    // Nothing in a string.
    assert!(suggest("SELECT 'a‸'").items.is_empty());
}

#[test]
fn an_insert_s_columns_all_of_them_first() {
    let c = suggest("INSERT INTO users (‸");
    assert_eq!(labels(&c), ["id, name, email", "id", "name", "email"]);
    assert_eq!(c.items[0].kind, SuggestionKind::Columns);
    // First whatever is typed.
    let c = suggest("INSERT INTO users (na‸");
    assert_eq!(c.items[0].kind, SuggestionKind::Columns);
    assert_eq!(c.items[1].label, "name");
    // UPDATE … SET: its target's columns.
    let c = suggest("UPDATE orders SET ‸");
    assert_eq!(labels(&c), ["id", "user_id", "total", "created_at"]);
}

#[test]
fn select_star_is_expanded_to_its_columns() {
    let c = suggest("SELECT *‸ FROM users");
    assert_eq!(labels(&c), ["id, name, email"]);
    assert!(c.items[0].expansion);
    assert_eq!(c.replace, Some(7..8));
    let c = suggest("SELECT *‸ FROM users u JOIN orders o ON o.user_id = u.id");
    assert_eq!(c.items[0].insert, "u.id, u.name, u.email, o.id, o.user_id, o.total, o.created_at");
    let c = suggest("SELECT o.*‸ FROM users u JOIN orders o ON o.user_id = u.id");
    assert_eq!(c.items[0].insert, "o.id, o.user_id, o.total, o.created_at");
    assert_eq!(c.replace, Some(7..10));
}

#[test]
fn ctes_and_derived_tables_give_their_columns() {
    let c = suggest("WITH x AS (SELECT id, name AS who FROM users) SELECT x.‸ FROM x");
    assert_eq!(labels(&c), ["id", "who"]);
    let c = suggest("WITH x AS (SELECT * FROM users) SELECT x.‸ FROM x");
    assert_eq!(labels(&c), ["id", "name", "email"], "its star: its table's");
    let c = suggest(
        "SELECT d.‸ FROM (SELECT user_id, sum(total) AS spent FROM orders GROUP BY user_id) d",
    );
    assert_eq!(labels(&c), ["user_id", "spent"]);
}

#[test]
fn order_by_offers_the_select_list_s_aliases_first() {
    let c = suggest("SELECT user_id, sum(total) AS spent FROM orders GROUP BY user_id ORDER BY ‸");
    assert_eq!(c.items[0].label, "spent");
    assert_eq!(c.items[0].kind, SuggestionKind::Alias);
    let c = suggest("SELECT user_id, sum(total) AS spent FROM orders WHERE ‸");
    assert!(!labels(&c).contains(&"spent"));
}

#[test]
fn a_huge_schema_asks_for_the_columns_it_needs() {
    let index = SchemaIndex::build(shop(false));
    let c = suggest_with(
        "SELECT e.‸ FROM events e JOIN users u ON u.id = e.user_id",
        Some(&index),
        LITE,
        None,
    );
    assert_eq!(c.missing, [TableRef::new("events")]);
    assert_eq!(c.items.last().map(|i| i.kind), Some(SuggestionKind::Loading));
    let c = suggest_with(
        "SELECT ‸ FROM events e JOIN users u ON u.id = e.user_id",
        Some(&index),
        LITE,
        None,
    );
    assert_eq!(c.missing, [TableRef::new("events"), TableRef::new("users")]);
    // Once read: the list made again has them.
    let index = index.with_columns(shop_loaded(&["events", "users"]), 2);
    let c = suggest_with("SELECT e.‸ FROM events e", Some(&index), LITE, None);
    assert!(c.missing.is_empty());
    assert_eq!(labels(&c), ["id", "kind", "at", "user_id"]);
}

#[test]
fn without_a_schema_the_statement_s_own_names_and_the_words() {
    let c = suggest_with("WITH x AS (SELECT 1 AS a) SELECT x.‸ FROM x", None, LITE, None);
    assert_eq!(labels(&c), ["a"]);
    let c = suggest_with("SELECT * FROM ‸", None, LITE, None);
    assert!(c.items.is_empty());
    let c = suggest_with("SELECT Cou‸", None, PG, None);
    assert_eq!(c.items[0].label, "COUNT");
}

#[test]
fn names_are_quoted_as_the_engine_needs() {
    assert_eq!(quote("users", PG), "users");
    assert_eq!(quote("Users", PG), "\"Users\"", "PostgreSQL folds capitals");
    assert_eq!(quote("Users", LITE), "Users");
    assert_eq!(quote("order", LITE), "\"order\"");
    assert_eq!(quote("my table", LITE), "\"my table\"");
    assert_eq!(quote("a\"b", PG), "\"a\"\"b\"");
    assert_eq!(quote("1st", LITE), "\"1st\"");
    assert_eq!(alias_for("order_items", &[]), "oi");
    assert_eq!(alias_for("OrderItems", &[]), "oi");
    assert_eq!(alias_for("users", &["u".into()]), "u1");
    assert_eq!(alias_for("on_hold", &[]), "oh");
    assert_eq!(alias_for("orders", &[]), "o");
    assert_eq!(alias_for("as_x", &[]), "ax");
    assert_eq!(alias_for("a_s", &[]), "as1", "not a keyword");
}

#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn five_thousand_tables_complete_within_budget() {
    let index = big_index();
    let budget = if cfg!(debug_assertions) { 60.0 } else { 2.0 };
    for sql in [
        "SELECT * FROM ‸",
        "SELECT * FROM table_04‸",
        "SELECT * FROM tbl‸",
        "SELECT * FROM table_00010 t JOIN ‸",
        "SELECT t.‸ FROM table_00010 t",
        "SELECT col‸ FROM table_00010 t JOIN table_00011 u ON u.table_00010_id = t.id",
    ] {
        let offset = sql.find('‸').unwrap();
        let text = sql.replace('‸', "");
        let tokens = lex(&text, PG);
        let started = std::time::Instant::now();
        let mut c = Completion::default();
        for _ in 0..5 {
            let scope = scope_in(&text, &tokens, offset, PG);
            c = complete(&CompletionRequest {
                scope: &scope,
                index: Some(index),
                flavor: PG,
                schema: None,
                limit: LIMIT,
            });
        }
        let ms = started.elapsed().as_secs_f64() * 1000.0 / 5.0;
        assert!(ms < budget, "{sql}: {ms:.2} ms");
        assert!(!c.items.is_empty(), "{sql}");
        assert!(c.items.len() <= LIMIT);
    }
}

/// A shop whose `orders.status` is a MySQL enum and `orders.mood` a PostgreSQL one.
fn enums() -> SchemaIndex {
    let column = |name: &str, type_name: &str, values: Option<&[&str]>| crate::ColumnBrief {
        name: name.into(),
        type_name: type_name.into(),
        values: values.map(|v| v.iter().map(|s| (*s).to_owned()).collect()),
        ..Default::default()
    };
    SchemaIndex::build(IndexParts {
        tables: vec![TableInfo {
            table: TableRef::new("orders"),
            kind: TableKind::Table,
            rows: None,
        }],
        columns: vec![(
            TableRef::new("orders"),
            vec![
                column("id", "int", None),
                column("status", "enum('new','paid','shipped')", None),
                column("mood", "mood", Some(&["sad", "ok", "happy"])),
            ],
        )],
        complete: true,
        ..IndexParts::default()
    })
}

#[test]
fn an_enum_column_s_value_offers_its_literals_first() {
    let index = enums();
    let my = Flavor::MySql;
    let c = suggest_with("SELECT * FROM orders WHERE status = ‸", Some(&index), my, None);
    assert_eq!(of_kind(&c, SuggestionKind::Value), ["new", "paid", "shipped"]);
    assert_eq!(&inserts(&c)[..3], ["'new'", "'paid'", "'shipped'"]);
    assert_eq!(c.items[0].detail.as_deref(), Some("orders.status"));
    // PostgreSQL's labels, in their order; typed: filtered.
    let c = suggest_with("UPDATE orders SET mood = h‸", Some(&index), PG, None);
    assert_eq!(c.items[0].label, "happy");
    // In a list, in an INSERT's values (by the list or the table's order).
    let c = suggest_with("SELECT * FROM orders WHERE mood IN ('sad', ‸", Some(&index), PG, None);
    assert_eq!(of_kind(&c, SuggestionKind::Value).len(), 3);
    let c = suggest_with("INSERT INTO orders VALUES (1, ‸", Some(&index), my, None);
    assert_eq!(of_kind(&c, SuggestionKind::Value), ["new", "paid", "shipped"]);
    // In an open literal: only the literals, the literal replaced whole.
    let sql = "SELECT * FROM orders o WHERE o.status = 'pa‸";
    let c = suggest_with(sql, Some(&index), my, None);
    assert_eq!(labels(&c), ["paid"]);
    assert_eq!(inserts(&c), ["'paid'"]);
    // Not an enum: nothing of the sort.
    let c = suggest_with("SELECT * FROM orders WHERE id = ‸", Some(&index), my, None);
    assert!(of_kind(&c, SuggestionKind::Value).is_empty());
    let c = suggest_with("SELECT * FROM orders WHERE id = '‸", Some(&index), my, None);
    assert!(c.items.is_empty());
}

#[test]
fn an_open_literal_s_scope_replaces_it_whole() {
    let sql = "SELECT * FROM orders WHERE status = 'pa‸id'";
    let offset = sql.find('‸').unwrap();
    let text = sql.replace('‸', "");
    let scope = scope_in(&text, &lex(&text, Flavor::MySql), offset, Flavor::MySql);
    assert_eq!(scope.expect, Expect::Literal);
    assert_eq!(&text[scope.replace.clone()], "'paid'");
    assert_eq!(scope.prefix, "pa");
}

#[test]
fn a_function_s_signature_is_the_schema_s_else_one_known() {
    let index = SchemaIndex::build(shop(true));
    assert_eq!(builtin_signature("SUBSTR", LITE), Some("substr(text, start, length) → text"));
    assert_eq!(
        builtin_signature("date_format", Flavor::MySql),
        Some("date_format(date, format) → text")
    );
    assert_eq!(builtin_signature("date_format", LITE), None, "not SQLite's");
    assert_eq!(
        signature("count", LITE, Some(&index)).as_deref(),
        Some("count(expression) → integer")
    );
    assert_eq!(signature("nothing_like_it", LITE, Some(&index)), None);
    // A routine of the schema with its arguments: as the engine says.
    let parts = IndexParts {
        routines: vec![crate::Routine {
            name: "net_total".into(),
            arguments: "order_id integer, rate numeric".into(),
            result: Some("numeric".into()),
            ..Default::default()
        }],
        complete: true,
        ..IndexParts::default()
    };
    let index = SchemaIndex::build(parts);
    assert_eq!(
        signature("NET_TOTAL", PG, Some(&index)).as_deref(),
        Some("net_total(order_id integer, rate numeric) → numeric")
    );
}
