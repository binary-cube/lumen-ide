use super::*;
use extractors::{Column, Input, Kind, Options, SqlInserts, extract};

use crate::postgres::Postgres;
use crate::sqlite::Sqlite;

fn insert(dialect: Arc<dyn Dialect>, row: Vec<Cell<'_>>) -> String {
    let columns = vec![Column::new("id", Kind::Integer), Column::new("on", Kind::Bool)];
    let input = Input::new("t", columns).with_sql(sql_style(dialect));
    extract(&SqlInserts { multirow: false }, input, Options::default(), &[row])
}

#[test]
fn each_dialect_writes_its_literals() {
    assert_eq!(
        insert(Arc::new(Sqlite), vec![Cell::Int(1), Cell::Bool(true)]),
        "INSERT INTO \"t\" (\"id\", \"on\") VALUES (1, 1);"
    );
    assert_eq!(
        insert(Arc::new(Postgres::new(160_000)), vec![Cell::Int(1), Cell::Bool(true)]),
        "INSERT INTO \"t\" (\"id\", \"on\") VALUES (1, TRUE);"
    );
}

#[test]
fn what_a_dialect_refuses_is_written_as_standard_sql() {
    let style = DialectSql(Arc::new(Sqlite));
    assert_eq!(style.literal(Cell::Float(f64::INFINITY)), "NULL");
    assert_eq!(style.literal(Cell::Text("a\0b")), "'a\0b'");
    assert_eq!(style.name(""), "\"\"");
    assert_eq!(value_of(cell_of(&Value::Bytes(vec![1]))), Value::Bytes(vec![1]));
}
