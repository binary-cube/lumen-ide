use super::*;
use crate::{Flow, RowSink};

#[test]
fn rows_come_from_what_a_sink_collected() {
    let mut collect = Collect::default();
    collect.columns(&[Column { name: "id".into(), type_name: "INTEGER".into() }]);
    collect.row_key(RowKey::RowId(1));
    assert_eq!(collect.row(vec![Value::Int(1)]), Flow::Continue);
    let rows = Rows::from(collect);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.keys, vec![RowKey::RowId(1)]);
    assert_eq!(rows.columns[0].name, "id");
}

#[test]
fn a_pages_weight_grows_with_its_values() {
    let small = Rows { rows: vec![vec![Value::Text("a".into())]], ..Rows::default() };
    let large = Rows { rows: vec![vec![Value::Text("a".repeat(10_000))]], ..Rows::default() };
    assert!(Rows::default().bytes() < small.bytes());
    assert!(large.bytes() >= small.bytes() + 9_999);
    let blob = Rows { rows: vec![vec![Value::Bytes(vec![0; 4096])]], ..Rows::default() };
    assert!(blob.bytes() >= 4096);
}

#[test]
fn a_fetch_says_what_it_holds() {
    assert_eq!(Fetch::Ready(3).ready(), Some(3));
    assert_eq!(Fetch::<u8>::Pending.ready(), None);
    assert!(Fetch::<u8>::Pending.is_pending());
    assert!(!Fetch::<u8>::Failed(DbError::Cancelled).is_pending());
}
