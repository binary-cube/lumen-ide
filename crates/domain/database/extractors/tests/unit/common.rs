//! What the unit tests share: three columns (the first the key) and two rows.
use crate::cell::{Cell, Column, Kind};

pub fn cols() -> Vec<Column> {
    vec![
        Column::new("id", Kind::Integer).keyed(),
        Column::new("name", Kind::Text),
        Column::new("price", Kind::Decimal),
    ]
}

pub fn rows() -> Vec<Vec<Cell<'static>>> {
    vec![
        vec![Cell::Int(1), Cell::Text("Ann"), Cell::Float(2.5)],
        vec![Cell::Int(2), Cell::Text("O'Hara, \"Jo\"\nline"), Cell::Null],
    ]
}
