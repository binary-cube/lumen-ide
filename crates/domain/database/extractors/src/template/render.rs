//! A template written for its rows, a block at a time: what's before the rows at the start,
//! each row as it comes — one kept back until the next arrives, so `{{#last}}` knows —, what's
//! after them at the end.

use std::sync::Arc;

use super::{Expr, Filter, Node, Section, Template};
use crate::cell::{Cell, Column};
use crate::delimited::CsvFormat;
use crate::extractor::{Input, Stream};
use crate::formats::{json_string, json_value, md_cell, xml_escape};

/// A cell kept (the row held back until the next comes).
#[derive(Debug, Clone, PartialEq)]
enum Kept {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Json(String),
    Other(String),
}

impl Kept {
    fn of(cell: &Cell<'_>) -> Self {
        match *cell {
            Cell::Null => Self::Null,
            Cell::Bool(b) => Self::Bool(b),
            Cell::Int(i) => Self::Int(i),
            Cell::Float(f) => Self::Float(f),
            Cell::Text(t) => Self::Text(t.to_owned()),
            Cell::Bytes(b) => Self::Bytes(b.to_vec()),
            Cell::Json(t) => Self::Json(t.to_owned()),
            Cell::Other(t) => Self::Other(t.to_owned()),
        }
    }

    fn cell(&self) -> Cell<'_> {
        match self {
            Self::Null => Cell::Null,
            Self::Bool(b) => Cell::Bool(*b),
            Self::Int(i) => Cell::Int(*i),
            Self::Float(f) => Cell::Float(*f),
            Self::Text(t) => Cell::Text(t),
            Self::Bytes(b) => Cell::Bytes(b),
            Self::Json(t) => Cell::Json(t),
            Self::Other(t) => Cell::Other(t),
        }
    }
}

/// The row being written.
#[derive(Clone, Copy)]
struct RowAt<'a> {
    index: u64,
    cells: &'a [Cell<'a>],
    first: bool,
    last: bool,
}

/// The column being written.
#[derive(Clone, Copy)]
struct ColumnAt<'a> {
    index: usize,
    column: &'a Column,
    first: bool,
    last: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Loop {
    Rows,
    Columns,
}

#[derive(Clone, Copy)]
struct Scope<'a> {
    input: &'a Input,
    row: Option<RowAt<'a>>,
    column: Option<ColumnAt<'a>>,
    innermost: Option<Loop>,
}

impl<'a> Scope<'a> {
    fn value(&self) -> Cell<'a> {
        match (self.row, self.column) {
            (Some(row), Some(column)) => row.cells.get(column.index).copied().unwrap_or(Cell::Null),
            _ => Cell::Null,
        }
    }

    fn holds(&self, section: Section) -> bool {
        let (first, last) = match self.innermost {
            Some(Loop::Columns) => self.column.map_or((false, false), |c| (c.first, c.last)),
            Some(Loop::Rows) => self.row.map_or((false, false), |r| (r.first, r.last)),
            None => (false, false),
        };
        match section {
            Section::First => first,
            Section::Last => last,
            Section::Null => self.value().is_null(),
            Section::Number => {
                let value = self.value();
                value.is_number()
                    || (self.column.is_some_and(|c| c.column.kind.is_number())
                        && matches!(value, Cell::Other(t) if t.trim().parse::<f64>().is_ok_and(f64::is_finite)))
            }
            Section::Key => self.column.is_some_and(|c| c.column.key),
            Section::Rows | Section::Columns => true,
        }
    }
}

/// What a field is before its filters: a cell, a name (quoted by `|sql`), plain text.
enum Val<'a> {
    Cell(Cell<'a>),
    Name(String),
    Plain(String),
}

impl Val<'_> {
    fn text(&self) -> String {
        match self {
            Self::Cell(c) => c.text().into_owned(),
            Self::Name(t) | Self::Plain(t) => t.clone(),
        }
    }

    fn numeric(&self) -> bool {
        matches!(self, Self::Plain(t) if t.parse::<f64>().is_ok())
    }
}

fn filtered(val: Val<'_>, filter: Filter, input: &Input) -> String {
    match filter {
        Filter::Sql => match val {
            Val::Cell(c) => input.sql.literal(c),
            Val::Name(n) => input.sql.name(&n),
            ref plain if plain.numeric() => plain.text(),
            plain => input.sql.literal(Cell::Text(&plain.text())),
        },
        Filter::Json => match val {
            Val::Cell(c) => json_value(c),
            ref plain if plain.numeric() => plain.text(),
            other => json_string(&other.text()),
        },
        Filter::Csv => {
            let mut out = String::new();
            if !matches!(val, Val::Cell(Cell::Null)) {
                CsvFormat::new("csv", "", ",", false).field(&val.text(), &mut out);
            }
            out
        }
        Filter::Html | Filter::Xml => xml_escape(&val.text()),
        Filter::Md => md_cell(&val.text()),
        Filter::Php => language_literal(&val, Language::Php),
        Filter::Py => language_literal(&val, Language::Py),
        Filter::Upper => val.text().to_uppercase(),
        Filter::Lower => val.text().to_lowercase(),
        Filter::Trim => val.text().trim().to_owned(),
        Filter::Pad(n) => {
            let text = val.text();
            let pad = (n.unsigned_abs() as usize).saturating_sub(text.chars().count());
            if n < 0 {
                format!("{}{text}", " ".repeat(pad))
            } else {
                format!("{text}{}", " ".repeat(pad))
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    Php,
    Py,
}

/// A text as a single-quoted literal of `language` (`\` and `'` escaped; Python's line breaks
/// too).
fn quoted(text: &str, language: Language) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' if language == Language::Py => out.push_str("\\n"),
            '\r' if language == Language::Py => out.push_str("\\r"),
            '\t' if language == Language::Py => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// A field as a literal of `language`: `null` / `None`, booleans, numbers, quoted texts.
fn language_literal(val: &Val<'_>, language: Language) -> String {
    let (null, yes, no) = match language {
        Language::Php => ("null", "true", "false"),
        Language::Py => ("None", "True", "False"),
    };
    match val {
        Val::Cell(Cell::Null) => null.to_owned(),
        Val::Cell(Cell::Bool(b)) => if *b { yes } else { no }.to_owned(),
        Val::Cell(Cell::Int(i)) => i.to_string(),
        Val::Cell(Cell::Float(f)) if f.is_finite() => f.to_string(),
        Val::Cell(Cell::Float(_)) => null.to_owned(),
        plain if plain.numeric() => plain.text(),
        other => quoted(&other.text(), language),
    }
}

fn output(expr: Expr, filters: &[Filter], scope: &Scope<'_>, out: &mut String) {
    let mut val = match expr {
        Expr::Value => Val::Cell(scope.value()),
        Expr::Table => Val::Name(scope.input.table.clone()),
        Expr::ColumnName => {
            Val::Name(scope.column.map(|c| c.column.name.clone()).unwrap_or_default())
        }
        Expr::ColumnType => {
            Val::Plain(scope.column.map(|c| c.column.type_name.clone()).unwrap_or_default())
        }
        Expr::ColumnKind => {
            Val::Plain(scope.column.map(|c| c.column.kind.name().to_owned()).unwrap_or_default())
        }
        Expr::ColumnIndex => {
            Val::Plain(scope.column.map(|c| c.index.to_string()).unwrap_or_default())
        }
        Expr::RowIndex => Val::Plain(scope.row.map(|r| r.index.to_string()).unwrap_or_default()),
        Expr::RowNumber => {
            Val::Plain(scope.row.map(|r| (r.index + 1).to_string()).unwrap_or_default())
        }
    };
    for filter in filters {
        val = Val::Plain(filtered(val, *filter, scope.input));
    }
    out.push_str(&val.text());
}

fn write(nodes: &[Node], scope: &Scope<'_>, out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Output { expr, filters } => output(*expr, filters, scope, out),
            Node::Section { section: Section::Columns, body, .. } => {
                let columns = &scope.input.columns;
                for (index, column) in columns.iter().enumerate() {
                    let at = ColumnAt {
                        index,
                        column,
                        first: index == 0,
                        last: index + 1 == columns.len(),
                    };
                    let inner =
                        Scope { column: Some(at), innermost: Some(Loop::Columns), ..*scope };
                    write(body, &inner, out);
                }
            }
            // The rows' section is only at the top (written by the stream).
            Node::Section { section: Section::Rows, .. } => {}
            Node::Section { section, inverted, body } => {
                if scope.holds(*section) != *inverted {
                    write(body, scope, out);
                }
            }
        }
    }
}

/// A template's stream (see the module's doc).
pub struct TemplateStream {
    template: Arc<Template>,
    input: Input,
    /// The row held back (the next one says whether it's the last), and its index.
    held: Option<Vec<Kept>>,
    index: u64,
}

impl TemplateStream {
    pub fn new(template: Arc<Template>, input: Input) -> Self {
        Self { template, input, held: None, index: 0 }
    }

    fn top(&self) -> Scope<'_> {
        Scope { input: &self.input, row: None, column: None, innermost: None }
    }

    /// The row held written (`last`: no other follows).
    fn write_held(&mut self, last: bool, out: &mut String) {
        let Some(held) = self.held.take() else { return };
        let Some(body) = self.template.row.clone() else { return };
        let cells: Vec<Cell<'_>> = held.iter().map(Kept::cell).collect();
        let row = RowAt { index: self.index, cells: &cells, first: self.index == 0, last };
        let scope =
            Scope { input: &self.input, row: Some(row), column: None, innermost: Some(Loop::Rows) };
        write(&body, &scope, out);
        self.index += 1;
    }
}

impl Stream for TemplateStream {
    fn begin(&mut self, out: &mut String) {
        let template = Arc::clone(&self.template);
        write(&template.before, &self.top(), out);
    }

    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        if self.template.row.is_none() {
            return;
        }
        for row in rows {
            self.write_held(false, out);
            self.held = Some(row.iter().map(Kept::of).collect());
        }
    }

    fn finish(&mut self, out: &mut String) {
        self.write_held(true, out);
        let template = Arc::clone(&self.template);
        write(&template.after, &self.top(), out);
    }
}

/// A template that can't be used: nothing written.
pub struct Nothing;

impl Stream for Nothing {
    fn rows(&mut self, _rows: &[Vec<Cell<'_>>], _out: &mut String) {}
}
