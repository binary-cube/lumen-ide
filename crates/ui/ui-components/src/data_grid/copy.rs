//! Rows as text, for the clipboard and the exports: through a data extractor ([`extractors`]:
//! SQL `INSERT`s / `UPDATE`s / a `WHERE` clause, separated values, JSON, Markdown, HTML,
//! XML, one row, an aligned table, the user's templates) — exactly, whatever the values hold
//! (quotes, separators, new lines, `NULL`). The grid's columns and cells are given to it as
//! they are ([`column_of`], [`cell_of`]); the SQL is the source's dialect's.

use std::sync::Arc;

use extractors::{Cell, Column, Extraction, Input, Kind, SqlStyle};
pub use extractors::{Extractor, Group, Options};

use super::source::{CellClass, CellRef, GridColumn};

/// A grid's column as the extractors read it (its key, computed and generated marks kept).
pub fn column_of(column: &GridColumn) -> Column {
    Column {
        name: column.name.clone(),
        type_name: column.type_name.clone(),
        kind: kind_of(column.class),
        key: column.key,
        computed: column.computed,
        generated: column.generated,
    }
}

pub fn kind_of(class: CellClass) -> Kind {
    match class {
        CellClass::Text => Kind::Text,
        CellClass::Integer => Kind::Integer,
        CellClass::Decimal => Kind::Decimal,
        CellClass::Bool => Kind::Bool,
        CellClass::DateTime => Kind::DateTime,
        CellClass::Binary => Kind::Binary,
        CellClass::Json => Kind::Json,
        CellClass::Other => Kind::Other,
    }
}

/// A cell as the extractors read it.
pub fn cell_of<'a>(value: CellRef<'a>) -> Cell<'a> {
    match value {
        CellRef::Null => Cell::Null,
        CellRef::Bool(b) => Cell::Bool(b),
        CellRef::Int(i) => Cell::Int(i),
        CellRef::Float(f) => Cell::Float(f),
        CellRef::Text(t) => Cell::Text(t),
        CellRef::Bytes(b) => Cell::Bytes(b),
        CellRef::Json(t) => Cell::Json(t),
        CellRef::Other(t) => Cell::Other(t),
    }
}

/// `rows` (each with one value per column of `columns`) as `extractor` writes them; `table`
/// names the table of SQL statements, `sql` how they're written (`None`: standard SQL).
pub fn copy_text(
    extractor: &dyn Extractor,
    table: &str,
    columns: &[GridColumn],
    sql: Option<Arc<dyn SqlStyle>>,
    options: Options,
    rows: &[Vec<CellRef<'_>>],
) -> String {
    let mut stream = CopyStream::new(extractor, table, columns, sql, options);
    let mut out = stream.begin();
    out.push_str(&stream.rows(rows));
    out.push_str(&stream.finish());
    out
}

/// The same text as [`copy_text`], written a piece at a time (an export of millions of
/// rows: a block of them at a time, never all at once): what [`Self::begin`],
/// each [`Self::rows`] and [`Self::finish`] give, joined, is exactly [`copy_text`] of all
/// the rows.
pub struct CopyStream {
    extraction: Extraction,
}

impl std::fmt::Debug for CopyStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopyStream").field("written", &self.written()).finish()
    }
}

impl CopyStream {
    pub fn new(
        extractor: &dyn Extractor,
        table: &str,
        columns: &[GridColumn],
        sql: Option<Arc<dyn SqlStyle>>,
        options: Options,
    ) -> Self {
        let mut input = Input::new(table, columns.iter().map(column_of).collect());
        if let Some(sql) = sql {
            input = input.with_sql(sql);
        }
        Self { extraction: Extraction::new(extractor, input, options) }
    }

    /// Rows written so far.
    pub fn written(&self) -> u64 {
        self.extraction.written()
    }

    /// What comes before the rows (a header).
    pub fn begin(&mut self) -> String {
        self.extraction.begin()
    }

    /// The next rows.
    pub fn rows(&mut self, rows: &[Vec<CellRef<'_>>]) -> String {
        let cells: Vec<Vec<Cell<'_>>> =
            rows.iter().map(|row| row.iter().map(|v| cell_of(*v)).collect()).collect();
        self.extraction.rows(&cells)
    }

    /// What comes after the rows (a JSON array closed).
    pub fn finish(&mut self) -> String {
        self.extraction.finish()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/copy.rs"]
mod tests;
