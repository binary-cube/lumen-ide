//! Whose value is written where: compared, set, inserted (by the column list or the
//! table's order), in a list, in an open literal; an enum's values read from MySQL's type.

use super::*;
use crate::sql::{Flavor, lex};

/// The column whose value goes at `‸` of `sql`.
fn at(sql: &str) -> Option<ValueOf> {
    let offset = sql.find('‸').expect("a cursor");
    let text = sql.replace('‸', "");
    let tokens = lex(&text, Flavor::MySql);
    value_of(&text, &tokens, offset)
}

fn column(qualifier: &[&str], name: &str) -> Option<ValueOf> {
    Some(ValueOf::Column {
        qualifier: qualifier.iter().map(|q| (*q).to_owned()).collect(),
        name: name.into(),
    })
}

#[test]
fn a_compared_or_set_column_is_found() {
    assert_eq!(at("SELECT * FROM orders WHERE status = ‸"), column(&[], "status"));
    assert_eq!(at("SELECT * FROM orders o WHERE o.status <> ‸"), column(&["o"], "status"));
    assert_eq!(at("SELECT * FROM orders WHERE `status` != ‸"), column(&[], "status"));
    assert_eq!(at("UPDATE orders SET status = ‸"), column(&[], "status"));
    assert_eq!(at("SELECT 1 FROM t WHERE a = 1 AND size = ‸"), column(&[], "size"));
    // Not a column's value: a product, a function's argument, a column list.
    assert_eq!(at("SELECT * FROM orders WHERE total > ‸"), None);
    assert_eq!(at("SELECT coalesce(status, ‸) FROM orders"), None);
    assert_eq!(at("SELECT ‸ FROM orders"), None);
}

#[test]
fn a_list_s_column_is_found() {
    assert_eq!(at("SELECT * FROM t WHERE size IN (‸"), column(&[], "size"));
    assert_eq!(at("SELECT * FROM t WHERE t.size NOT IN ('S', ‸)"), column(&["t"], "size"));
    // An INSERT's values: by its column list, or by the table's order.
    assert_eq!(at("INSERT INTO t (id, size) VALUES (1, ‸"), column(&[], "size"));
    assert_eq!(at("INSERT INTO t (id, size) VALUES (1, 'S'), (2, ‸"), column(&[], "size"));
    assert_eq!(
        at("INSERT INTO shop.t VALUES (1, ‸"),
        Some(ValueOf::Position { table: vec!["shop".into(), "t".into()], position: 1 })
    );
    assert_eq!(at("INSERT INTO t (id) VALUES (1, ‸"), None, "more values than columns");
}

#[test]
fn an_open_literal_is_found_only_when_it_s_a_plain_string() {
    let text = "SELECT * FROM t WHERE size = 'M";
    let tokens = lex(text, Flavor::MySql);
    let literal = open_literal(text, &tokens, text.len()).expect("in it");
    assert_eq!(literal_range(&literal, text.len()), text.len() - 2..text.len());
    let closed = "SELECT * FROM t WHERE size = 'M'";
    let tokens = lex(closed, Flavor::MySql);
    let literal = open_literal(closed, &tokens, closed.len() - 1).expect("in it");
    assert_eq!(literal_range(&literal, closed.len() - 1), closed.len() - 3..closed.len());
    assert!(open_literal(closed, &tokens, closed.len()).is_none(), "after it");
}

#[test]
fn mysql_s_enum_types_give_their_values() {
    assert_eq!(
        enum_values("enum('S','M', 'it''s','a\\'b')"),
        Some(vec!["S".into(), "M".into(), "it's".into(), "a'b".into()])
    );
    assert_eq!(enum_values("ENUM('x')"), Some(vec!["x".into()]));
    assert_eq!(enum_values("set('a','b')"), None);
    assert_eq!(enum_values("varchar(20)"), None);
    assert_eq!(literal("it's"), "'it''s'");
}
