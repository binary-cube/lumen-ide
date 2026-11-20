//! The contract: an [`Extractor`] (its id, name, group, the extension of a file it writes)
//! opens a [`Stream`] for what's extracted ([`Input`]: the table, the columns, the SQL
//! style), which writes a block of rows at a time — so an export of millions of rows never
//! holds them all. [`Extraction`] is what callers use: the columns "Skip Computed / Skip
//! Generated Columns" leave out taken away ([`Options`]), the rows counted; what its
//! [`Extraction::begin`], every [`Extraction::rows`] and [`Extraction::finish`] give,
//! joined, is the whole text ([`extract`] at once).

use std::sync::Arc;

use crate::cell::{Cell, Column};
use crate::sql::{AnsiSql, SqlStyle};

/// Where an extractor is listed (the popover's sections, in this order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    /// SQL statements and clauses.
    Sql,
    /// Separated values (CSV, TSV and the user's formats).
    Delimited,
    /// JSON, Markdown, HTML, XML, one row, an aligned table (and the modules' own).
    Other,
    /// The user's templates (`*.extractor`).
    Scripted,
}

impl Group {
    pub const ALL: [Group; 4] = [Self::Sql, Self::Delimited, Self::Other, Self::Scripted];

    /// Its heading in the user's language.
    pub fn label(self) -> String {
        i18n::t(match self {
            Self::Sql => "extract.group_sql",
            Self::Delimited => "extract.group_delimited",
            Self::Other => "extract.group_other",
            Self::Scripted => "extract.group_scripted",
        })
    }
}

/// What's extracted: the table the rows are of (its name as SQL writes it into), their
/// columns (in the order the rows give their values), how SQL is written.
#[derive(Debug, Clone)]
pub struct Input {
    pub table: String,
    pub columns: Vec<Column>,
    pub sql: Arc<dyn SqlStyle>,
}

impl Input {
    /// `columns` of `table`, in standard SQL.
    pub fn new(table: impl Into<String>, columns: Vec<Column>) -> Self {
        Self { table: table.into(), columns, sql: Arc::new(AnsiSql) }
    }

    pub fn with_sql(mut self, sql: Arc<dyn SqlStyle>) -> Self {
        self.sql = sql;
        self
    }
}

/// The text an extractor writes, a piece at a time.
pub trait Stream: Send {
    /// What comes before the rows (a header).
    fn begin(&mut self, out: &mut String) {
        let _ = out;
    }

    /// The next rows (each with a value per column of the input).
    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String);

    /// What comes after them (an array, a table closed).
    fn finish(&mut self, out: &mut String) {
        let _ = out;
    }
}

/// A way of writing rows (see the module's doc).
pub trait Extractor: Send + Sync + std::fmt::Debug {
    /// Its identity (kept as the project's choice; a menu row's).
    fn id(&self) -> &str;

    /// Its name, as listed.
    fn name(&self) -> String;

    fn group(&self) -> Group;

    /// The extension of a file it writes (`csv`, `sql`, `json`).
    fn extension(&self) -> &str;

    /// The extensions of a file's name that ask for it when an export is named so (`md`,
    /// `markdown`): one format each (of several writing `.sql`, the plain inserts); none by
    /// default.
    fn names_files(&self) -> &[&str] {
        &[]
    }

    /// "Skip Computed / Skip Generated Columns" apply to it (the SQL that writes rows).
    fn skips_columns(&self) -> bool {
        false
    }

    /// Why it can't be used (a template with an error: its line and column), if it can't.
    fn problem(&self) -> Option<String> {
        None
    }

    /// The file it's read from (a template), if any.
    fn origin(&self) -> Option<&std::path::Path> {
        None
    }

    /// Its stream for `input` (already without the columns skipped).
    fn open(&self, input: &Input) -> Box<dyn Stream>;
}

/// What's left out of the SQL that writes rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Columns the engine computes (`GENERATED ALWAYS AS`, a query's expressions).
    pub skip_computed: bool,
    /// Columns the engine makes (auto-increments, identities).
    pub skip_generated: bool,
}

/// An extraction under way (see the module's doc).
pub struct Extraction {
    stream: Box<dyn Stream>,
    /// The input's columns kept, by place (`None`: all of them).
    keep: Option<Vec<usize>>,
    written: u64,
}

impl std::fmt::Debug for Extraction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extraction").field("written", &self.written).finish_non_exhaustive()
    }
}

impl Extraction {
    /// `extractor` writing `input`'s rows, `options`' columns left out (when it skips any:
    /// never all of them — then none is).
    pub fn new(extractor: &dyn Extractor, mut input: Input, options: Options) -> Self {
        let skip = |c: &Column| {
            (options.skip_computed && c.computed) || (options.skip_generated && c.generated)
        };
        let keep = (extractor.skips_columns() && input.columns.iter().any(skip))
            .then(|| (0..input.columns.len()).filter(|&i| !skip(&input.columns[i])).collect())
            .filter(|kept: &Vec<usize>| !kept.is_empty());
        if let Some(kept) = &keep {
            input.columns = kept.iter().map(|&i| input.columns[i].clone()).collect();
        }
        Self { stream: extractor.open(&input), keep, written: 0 }
    }

    /// Rows written so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn begin(&mut self) -> String {
        let mut out = String::new();
        self.stream.begin(&mut out);
        out
    }

    /// The next rows (each with a value per column of the input given).
    pub fn rows(&mut self, rows: &[Vec<Cell<'_>>]) -> String {
        let mut out = String::new();
        self.write_rows(rows, &mut out);
        out
    }

    /// The same, added to `out`.
    pub fn write_rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        match &self.keep {
            None => self.stream.rows(rows, out),
            Some(keep) => {
                let kept: Vec<Vec<Cell<'_>>> = rows
                    .iter()
                    .map(|row| {
                        keep.iter().map(|&i| row.get(i).copied().unwrap_or(Cell::Null)).collect()
                    })
                    .collect();
                self.stream.rows(&kept, out);
            }
        }
        self.written += rows.len() as u64;
    }

    pub fn finish(&mut self) -> String {
        let mut out = String::new();
        self.stream.finish(&mut out);
        out
    }
}

/// `rows` of `input` as `extractor` writes them, at once.
pub fn extract(
    extractor: &dyn Extractor,
    input: Input,
    options: Options,
    rows: &[Vec<Cell<'_>>],
) -> String {
    let mut extraction = Extraction::new(extractor, input, options);
    let mut out = extraction.begin();
    extraction.write_rows(rows, &mut out);
    out.push_str(&extraction.finish());
    out
}

/// Lines joined by a line ending (none after the last: a file's end is the caller's).
#[derive(Debug, Clone)]
pub struct Lines {
    pub eol: &'static str,
    started: bool,
}

impl Lines {
    pub fn new(eol: &'static str) -> Self {
        Self { eol, started: false }
    }

    /// `line` added to `out`, after a line ending if something came before.
    pub fn push(&mut self, out: &mut String, line: &str) {
        if std::mem::replace(&mut self.started, true) {
            out.push_str(self.eol);
        }
        out.push_str(line);
    }

    pub fn started(&self) -> bool {
        self.started
    }
}

#[cfg(test)]
#[path = "../tests/unit/extractor.rs"]
mod tests;
