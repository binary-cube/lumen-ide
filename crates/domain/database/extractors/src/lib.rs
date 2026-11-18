//! Data extractors: what a grid's rows become when they're
//! copied (⌘C, "Copy As ›") or exported — the same extractor for all three, the one a
//! project chose in the grid's toolbar.
//!
//! An [`Extractor`] writes the rows of an [`Input`] (the table, the [`Column`]s, how SQL is
//! written: [`SqlStyle`], the source's dialect) a block at a time ([`Extraction`]), so an
//! export of millions of rows never holds them all. The built-in ones: SQL `INSERT`s (a row
//! or many a statement), `UPDATE`s by the key and a `WHERE` clause ([`sql_writers`], with
//! "Skip Computed / Skip Generated Columns": [`Options`]); separated values in the formats
//! configured ([`delimited`]: CSV, TSV, pipes, semicolons, the user's own); JSON, Markdown,
//! HTML, XML, one row and an aligned text table ([`formats`]). The [`catalog`] lists those
//! offered — the modules' own and a project's templates too ([`template`]: the user's own
//! extractors in a small, safe template language) — and keeps each project's choice. Pure:
//! nothing here reads a file, a clock or the network.

pub mod catalog;
pub mod cell;
pub mod delimited;
pub mod extractor;
pub mod formats;
pub mod sql;
pub mod sql_writers;
pub mod template;

pub use catalog::{Catalog, DEFAULT_ID, builtin, catalog};
pub use cell::{Cell, Column, Kind, hex};
pub use delimited::{CsvFormat, Delimited, LineEnd};
pub use extractor::{Extraction, Extractor, Group, Input, Lines, Options, Stream, extract};
pub use formats::{Html, Json, Markdown, OneRow, Pretty, Xml};
pub use sql::{AnsiSql, SqlStyle, ansi_literal};
pub use sql_writers::{SqlInserts, SqlUpdates, WhereClause};
pub use template::{TemplateError, TemplateExtractor};

/// The commands of the extractors' module, by their ids (a grid's popover runs them; the
/// module owns them): the project's choice and the options saved, "Configure CSV Formats…",
/// "Go to Extractors Directory".
pub const SAVE_COMMAND: &str = "extractors.save";
pub const CONFIGURE_CSV_COMMAND: &str = "extractors.configure_csv";
pub const OPEN_DIRECTORY_COMMAND: &str = "extractors.open_directory";

#[cfg(test)]
#[path = "../tests/unit/common.rs"]
mod test_support;
