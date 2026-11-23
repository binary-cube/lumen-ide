//! Tables found in any data (plan 94h): what a document is ([`detect`]: by its name, else by
//! its content) and every table in it ([`tables`]) — a JSON or YAML list of records
//! ([`tree`]), an XML element repeated under one parent ([`xml`]), an HTML `<table>`
//! ([`html`]), delimited lines ([`csv`]) — each with where it was found and its columns'
//! kinds inferred from their values ([`infer`]). A table is read as charts read rows
//! (`charts_model::Tabular`), so whatever holds data can be drawn.
//!
//! Pure work for a worker: no I/O. A document is hostile: its depth, its tables, rows and
//! columns are bounded ([`Limits`]); what's past them is left out and said.

pub mod csv;
pub mod detect;
pub mod html;
pub mod infer;
pub mod tree;
pub mod xml;

use std::borrow::Cow;

use charts_model::{Column, ColumnKind, Tabular};

pub use detect::{Format, detect};

/// What a document may hold before the rest is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How deep a tree is walked.
    pub depth: usize,
    /// Tables found in one document.
    pub tables: usize,
    /// Rows of one table.
    pub rows: usize,
    /// Columns of one table.
    pub columns: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { depth: 64, tables: 64, rows: 500_000, columns: 256 }
    }
}

/// A cell as read.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    Number(f64),
    Text(String),
}

impl Cell {
    /// A text as a cell: empty when it's only blanks.
    pub fn text(s: &str) -> Self {
        let t = s.trim();
        if t.is_empty() { Cell::Empty } else { Cell::Text(t.to_owned()) }
    }

    fn as_text(&self) -> Option<Cow<'_, str>> {
        match self {
            Cell::Empty => None,
            Cell::Number(n) => Some(Cow::Owned(charts_model::scale::number_label(*n, 1e-9))),
            Cell::Text(t) => Some(Cow::Borrowed(t)),
        }
    }
}

/// A table found in a document.
#[derive(Debug, Clone, PartialEq)]
pub struct DataTable {
    /// Where it was found (`$.orders`, `/catalog/book`, `table 2`).
    pub path: String,
    pub columns: Vec<Column>,
    /// How each column writes its numbers' marks (commas between thousands, a decimal comma).
    comma: Vec<infer::Marks>,
    pub rows: Vec<Vec<Cell>>,
    /// Rows or columns past the limits, left out.
    pub left_out: usize,
}

impl DataTable {
    /// A table at `path` of `names` and `rows` (as many cells each as names, or fewer),
    /// its columns' kinds inferred.
    pub fn new(path: impl Into<String>, names: Vec<String>, rows: Vec<Vec<Cell>>) -> Self {
        let (kinds, comma) = infer::kinds(names.len(), &rows);
        let columns = names.into_iter().zip(kinds).map(|(n, k)| Column::new(n, k)).collect();
        Self { path: path.into(), columns, comma, rows, left_out: 0 }
    }

    fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.rows.get(row)?.get(col)
    }
}

impl Tabular for DataTable {
    fn columns(&self) -> &[Column] {
        &self.columns
    }

    fn rows(&self) -> usize {
        self.rows.len()
    }

    fn number(&self, row: usize, col: usize) -> Option<f64> {
        match self.cell(row, col)? {
            Cell::Number(n) => Some(*n),
            Cell::Text(t) => match self.columns.get(col).map(|c| c.kind) {
                Some(ColumnKind::Time) => charts_model::moment(t),
                _ => infer::number(t, self.comma.get(col).copied().unwrap_or_default()),
            },
            Cell::Empty => None,
        }
    }

    fn text(&self, row: usize, col: usize) -> Option<Cow<'_, str>> {
        self.cell(row, col)?.as_text()
    }
}

/// Every table in `text`, a document named `name` (its extension says what it is; else
/// its content does), within `limits`.
pub fn tables(name: &str, text: &str, limits: Limits) -> Vec<DataTable> {
    let mut found = match detect(name, text) {
        Format::Json => tree::json(text, limits),
        Format::Yaml => tree::yaml(text, limits),
        Format::Xml => xml::tables(text, limits),
        Format::Html => html::tables(text, limits),
        Format::Csv(delimiter) => csv::table(text, delimiter, limits).into_iter().collect(),
        Format::Unknown => Vec::new(),
    };
    found.retain(|t| !t.rows.is_empty() && !t.columns.is_empty());
    found.truncate(limits.tables);
    found
}

/// A column's name when the data gives none (its place, from 1), in the user's language.
pub(crate) fn column_named(i: usize) -> String {
    i18n::tr!("charts.generated_column", n = i + 1)
}

/// A table's name when the page gives none (its place, from 1), in the user's language.
pub(crate) fn table_named(i: usize) -> String {
    i18n::tr!("charts.generated_table", n = i + 1)
}

/// The column of a record's keys, and of a plain value, in the user's language.
pub(crate) fn key_named() -> String {
    i18n::t("charts.generated_key")
}

pub(crate) fn value_named() -> String {
    i18n::t("charts.generated_value")
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
