use super::*;

type Row = Box<[Value]>;

fn lent() -> Lent<Row> {
    let first: Vec<Row> = vec![
        vec![Value::Text("RO".into()), Value::Int(10), Value::Other("2025-11-19 09:00:00".into())]
            .into(),
    ];
    let second: Vec<Row> =
        vec![vec![Value::Text("DE".into()), Value::Other("12.5".into()), Value::Null].into()];
    let columns = vec![
        Column::new("country", ColumnKind::Text),
        Column::new("amount", ColumnKind::Number),
        Column::new("at", ColumnKind::Time),
    ];
    Lent::chained(columns, vec![first.into(), second.into()])
}

/// Cells read across chunks as numbers (a decimal written as text too), moments as seconds,
/// text as it is; an empty cell is nothing.
#[test]
fn cells_are_read_as_numbers_moments_or_text() {
    let rows = lent();
    assert_eq!(rows.rows(), 2);
    assert_eq!(rows.number(0, 1), Some(10.0));
    assert_eq!(rows.number(1, 1), Some(12.5), "in the second chunk");
    assert_eq!(rows.number(0, 2), charts_model::moment("2025-11-19 09:00:00"));
    assert_eq!(rows.number(1, 2), None);
    assert_eq!(rows.text(1, 0).as_deref(), Some("DE"));
    assert_eq!(rows.text(1, 2), None);
    assert_eq!(rows.column("amount"), Some(1));
    assert_eq!(rows.number(5, 0), None, "past the end");
}
