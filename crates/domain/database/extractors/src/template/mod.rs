//! The user's own extractors: `*.extractor` templates in a small, safe language of ours
//! (instead of Groovy / JavaScript scripts: no interpreter — a template can't read
//! a file, run a program or reach the network; it only writes the rows it's given).
//!
//! ```text
//! ---
//! name: Python DataFrame
//! extension: py
//! ---
//! df = pd.DataFrame([
//! {{#rows}}
//!     { {{#columns}}{{column.name|py}}: {{value|py}}{{^last}}, {{/last}}{{/columns}} },
//! {{/rows}}
//! ])
//! ```
//!
//! - **The header** (optional, first): `name:` (as listed; else the file's name) and
//!   `extension:` (an export's file; else `txt`) between `---` lines.
//! - **Loops**: `{{#rows}}…{{/rows}}` (once, at the top: the rows, written as they come) and
//!   `{{#columns}}…{{/columns}}` (anywhere: in a row, its cells; outside, the columns alone —
//!   a header).
//! - **Conditions**: `{{#first}}`, `{{#last}}` (of the innermost loop), `{{#null}}`,
//!   `{{#number}}` (the cell's value), `{{#key}}` (the column), each inverted by `^`
//!   (`{{^last}}, {{/last}}`: a separator).
//! - **Fields**: `{{value}}` (a cell's, `NULL` empty), `{{column.name}}`, `{{column.type}}`,
//!   `{{column.kind}}`, `{{column.index}}`, `{{table}}`, `{{row.index}}` (from 0),
//!   `{{row.number}}` (from 1).
//! - **Filters**, in order: `|sql` (a value as the source's literal, a name quoted), `|json`,
//!   `|csv`, `|html`, `|xml`, `|md`, `|php` and `|py` (a literal of those languages: `null` /
//!   `None`, booleans, numbers, quoted texts), `|upper`, `|lower`, `|trim`, `|pad:N` (to N
//!   characters; `-N`: on the left).
//! - `{# a comment #}` (or `{{! a comment }}`); `\{{` is a `{{`, `\{#` a `{#`. A section's
//!   tag alone on its line takes the line with it.
//!
//! An error stops only its own template: it's listed with its line and column ([`TemplateError`])
//! and the others work.

mod complete;
mod parse;
mod render;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use complete::{Suggestion, SuggestionKind, complete};
pub use parse::{MAX_DEPTH, MAX_LEN, parse, place};

use crate::extractor::{Extractor, Group, Input, Stream};

/// The extension of a template's file.
pub const EXTENSION: &str = "extractor";

/// Where a template went wrong (1-based line and column, in characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

/// What a section is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Rows,
    Columns,
    First,
    Last,
    Null,
    Key,
    Number,
}

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rows => "rows",
            Self::Columns => "columns",
            Self::First => "first",
            Self::Last => "last",
            Self::Null => "null",
            Self::Key => "key",
            Self::Number => "number",
        }
    }
}

/// A field written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expr {
    Value,
    Table,
    ColumnName,
    ColumnType,
    ColumnKind,
    ColumnIndex,
    RowIndex,
    RowNumber,
}

/// What a field goes through before it's written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Sql,
    Json,
    Csv,
    Html,
    Xml,
    Md,
    Php,
    Py,
    Upper,
    Lower,
    Trim,
    /// To this many characters (negative: padded on the left).
    Pad(i32),
}

/// A part of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Text(String),
    Output { expr: Expr, filters: Vec<Filter> },
    Section { section: Section, inverted: bool, body: Vec<Node> },
}

/// A template read (see the module's doc): what's written before the rows, for each row,
/// after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub extension: String,
    pub before: Vec<Node>,
    /// The rows' section's body (`None`: the template has none — written once).
    pub row: Option<Vec<Node>>,
    pub after: Vec<Node>,
}

/// A template as an extractor (listed among the "Scripted" ones).
#[derive(Debug, Clone)]
pub struct TemplateExtractor {
    id: String,
    name: String,
    origin: PathBuf,
    read: Result<Arc<Template>, TemplateError>,
}

impl TemplateExtractor {
    /// The template `text` of file `origin`, known as `id`.
    pub fn new(id: impl Into<String>, origin: &Path, text: &str) -> Self {
        let stem = origin.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let read = parse(text, &stem).map(Arc::new);
        let name = match &read {
            Ok(template) => template.name.clone(),
            Err(_) => stem,
        };
        Self { id: id.into(), name, origin: origin.to_path_buf(), read }
    }

    /// The template, or where it went wrong.
    pub fn template(&self) -> Result<&Template, &TemplateError> {
        self.read.as_deref()
    }

    pub fn error(&self) -> Option<&TemplateError> {
        self.read.as_ref().err()
    }
}

impl Extractor for TemplateExtractor {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn group(&self) -> Group {
        Group::Scripted
    }

    fn extension(&self) -> &str {
        match &self.read {
            Ok(template) => &template.extension,
            Err(_) => "txt",
        }
    }

    fn problem(&self) -> Option<String> {
        let e = self.read.as_ref().err()?;
        Some(i18n::tr!(
            "extract.template_problem",
            line = e.line,
            column = e.column,
            message = e.message.as_str()
        ))
    }

    fn origin(&self) -> Option<&Path> {
        Some(&self.origin)
    }

    fn open(&self, input: &Input) -> Box<dyn Stream> {
        match &self.read {
            Ok(template) => {
                Box::new(render::TemplateStream::new(Arc::clone(template), input.clone()))
            }
            // Never offered (its problem is said): nothing written.
            Err(_) => Box::new(render::Nothing),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/template.rs"]
mod tests;
