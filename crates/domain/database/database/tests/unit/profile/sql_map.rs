use super::*;
use crate::profile::node::ObjectKind;

const PG: Flavor = Flavor::Postgres { version: None };

fn object(name: &str, alias: Option<&str>) -> ObjectRef {
    ObjectRef {
        kind: ObjectKind::Table,
        schema: None,
        name: name.into(),
        alias: alias.map(Into::into),
    }
}

fn at(map: &SqlMap, range: Option<Range<usize>>) -> Option<&str> {
    range.map(|r| &map.text()[r])
}

#[test]
fn tables_with_their_aliases() {
    let text = "SELECT * FROM orders o JOIN customers AS c ON c.id = o.customer_id \
                WHERE o.status = 'paid'";
    let map = SqlMap::new(text, PG);
    let names: Vec<(String, Option<&str>)> =
        map.tables().iter().map(|t| (t.table.to_string(), t.alias.as_deref())).collect();
    assert_eq!(names, [("orders".to_owned(), Some("o")), ("customers".to_owned(), Some("c")),]);
    let map = SqlMap::new("SELECT * FROM a, public.b x WHERE a.id = x.id", PG);
    assert_eq!(map.tables().len(), 2);
    assert_eq!(map.tables()[1].table, TableRef::in_schema("public", "b"));
    assert_eq!(map.resolve("x").unwrap().table.name, "b");
    assert_eq!(map.resolve("a").unwrap().table.name, "a");
    assert!(map.resolve("nope").is_none());
    // A function in FROM isn't a table; a subquery's tables are.
    let map = SqlMap::new("SELECT * FROM generate_series(1, 3) g, (SELECT * FROM t) s", PG);
    assert_eq!(map.tables().len(), 1);
    assert_eq!(map.tables()[0].table.name, "t");
}

#[test]
fn a_nodes_table_is_found_once_or_not_at_all() {
    let text = "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id";
    let map = SqlMap::new(text, PG);
    assert_eq!(at(&map, map.table_range(&object("orders", Some("o")))), Some("orders o"));
    assert_eq!(at(&map, map.table_range(&object("customers", None))), Some("customers c"));
    // The same table twice: ambiguous without the alias, found with it.
    let text = "SELECT * FROM t a JOIN t b ON a.id = b.parent";
    let map = SqlMap::new(text, PG);
    assert!(map.table_range(&object("t", None)).is_none(), "ambiguous");
    assert_eq!(at(&map, map.table_range(&object("t", Some("b")))), Some("t b"));
    assert!(map.resolve("t").is_some(), "both are the same table");
}

#[test]
fn a_columns_comparison_in_where_and_on() {
    let text = "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id \
                WHERE o.status = 'paid' AND total BETWEEN 1 AND 5 AND (c.region = 'eu' OR x = 1)";
    let map = SqlMap::new(text, PG);
    assert_eq!(
        at(&map, map.predicate_range("status", &["o", "orders"])),
        Some("o.status = 'paid'")
    );
    assert_eq!(at(&map, map.predicate_range("total", &["o"])), Some("total BETWEEN 1 AND 5"));
    assert_eq!(at(&map, map.predicate_range("region", &["c"])), Some("c.region = 'eu'"));
    assert_eq!(at(&map, map.predicate_range("customer_id", &["o"])), Some("c.id = o.customer_id"));
    // Another table's qualifier isn't this one's.
    assert!(map.predicate_range("status", &["c"]).is_none());
    // Compared twice: no single place.
    let map = SqlMap::new("SELECT * FROM t WHERE a = 1 OR a = 2", PG);
    assert!(map.predicate_range("a", &["t"]).is_none());
    // In the select list only: not a condition.
    let map = SqlMap::new("SELECT status FROM t", PG);
    assert!(map.predicate_range("status", &["t"]).is_none());
}

#[test]
fn places_after_values_written_in() {
    // The statement as sent (a parameter's value written in) is what's read.
    let sent = "SELECT * FROM users WHERE email = 'a@b.ro' AND id > 10";
    let map = SqlMap::new(sent, PG);
    let r = map.predicate_range("email", &["users"]).unwrap();
    assert_eq!(&sent[r.clone()], "email = 'a@b.ro'");
    // Taken back to the document, where the value was `:email`.
    let document = "-- users\nSELECT * FROM users WHERE email = :email AND id > 10";
    let start = document.find("SELECT").unwrap();
    let value = sent.find("'a@b.ro'").unwrap();
    let place = document.find(":email").unwrap() - start;
    let bind = crate::sql::BindMap {
        start,
        replaced: vec![(value..value + "'a@b.ro'".len(), place..place + ":email".len())],
    };
    let back = bind.to_document(r.start)..bind.to_document(r.end);
    assert_eq!(&document[back], "email = :email");
}

#[test]
fn the_clauses_rules_read() {
    let map =
        SqlMap::new("SELECT * FROM t WHERE a = 1 ORDER BY b DESC, t.c LIMIT 10 OFFSET 20000", PG);
    assert_eq!(
        map.sort_columns(ClauseKind::OrderBy, &["t"]).unwrap(),
        [("b".into(), true), ("c".into(), false)]
    );
    assert_eq!(map.offset().map(|(n, _)| n), Some(20_000));
    assert!(map.limit_without_order().is_none());
    assert_eq!(at(&map, map.select_star()), Some("*"));
    assert_eq!(&map.text()[map.top(ClauseKind::Where).unwrap().range.clone()], "a = 1");
    // An expression in ORDER BY: no index in its order.
    let map = SqlMap::new("SELECT a FROM t ORDER BY lower(a)", PG);
    assert!(map.sort_columns(ClauseKind::OrderBy, &["t"]).is_none());
    assert!(map.select_star().is_none());
    // LIMIT without ORDER BY at its level; a subquery's ORDER BY isn't the outer's.
    let map = SqlMap::new("SELECT * FROM (SELECT * FROM t ORDER BY a) s LIMIT 5", PG);
    assert_eq!(at(&map, map.limit_without_order()), Some("LIMIT"));
    let map = SqlMap::new("SELECT * FROM t WHERE id IN (SELECT id FROM u LIMIT 1) ORDER BY id", PG);
    assert_eq!(at(&map, map.limit_without_order()), Some("LIMIT"), "the subquery's own");
    // SQLite's `LIMIT offset, count`; a window's ORDER BY isn't the query's.
    let map =
        SqlMap::new("SELECT row_number() OVER (ORDER BY a) FROM t LIMIT 15000, 10", Flavor::Sqlite);
    assert_eq!(map.offset().map(|(n, _)| n), Some(15_000));
    assert!(map.limit_without_order().is_some());
    // `count(*)` isn't `SELECT *`.
    assert!(SqlMap::new("SELECT count(*) FROM t", PG).select_star().is_none());
    assert!(SqlMap::new("SELECT t.* FROM t", PG).select_star().is_some());
}
