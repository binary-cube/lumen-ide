//! Separated values: a [`CsvFormat`] (its separator, its quote, a header or not, the text a
//! `NULL` is written as, the line ending) and the extractor writing it. The formats offered
//! by default are CSV, TSV (⌘C's: pasted into a spreadsheet as cells), TSV with its header,
//! pipe- and semicolon-separated; the user changes them or adds others ("Configure CSV
//! Formats…"), kept in the settings as [`CsvFormat::encode`] writes them.

use crate::cell::Cell;
use crate::extractor::{Extractor, Group, Input, Lines, Stream};

/// How a line ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnd {
    #[default]
    Lf,
    CrLf,
}

impl LineEnd {
    pub fn text(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::CrLf => "crlf",
        }
    }
}

/// A format of separated values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvFormat {
    /// Its identity (`csv`, `tsv`…, `csv-<uuid>` for one the user added).
    pub id: String,
    pub name: String,
    /// Between values (never empty).
    pub separator: String,
    /// Around a value holding the separator, the quote or a line break (doubled inside);
    /// `None`: values are written as they are.
    pub quote: Option<char>,
    /// The columns' names first.
    pub header: bool,
    /// What a `NULL` is written as (a value equal to it is quoted).
    pub null_text: String,
    pub line_end: LineEnd,
}

impl CsvFormat {
    /// A format separated by `separator`, quoted by `"`, with its header, `NULL` empty.
    pub fn new(id: &str, name: impl Into<String>, separator: &str, header: bool) -> Self {
        Self {
            id: id.to_owned(),
            name: name.into(),
            separator: separator.to_owned(),
            quote: Some('"'),
            header,
            null_text: String::new(),
            line_end: LineEnd::Lf,
        }
    }

    /// The formats offered until the user changes them.
    pub fn defaults() -> Vec<CsvFormat> {
        vec![
            Self::new("csv", i18n::t("extract.csv"), ",", true),
            Self::new("tsv", i18n::t("extract.tsv"), "\t", false),
            Self::new("tsv-header", i18n::t("extract.tsv_header"), "\t", true),
            Self::new("pipe", i18n::t("extract.pipe"), "|", true),
            Self::new("semicolon", i18n::t("extract.semicolon"), ";", true),
        ]
    }

    /// Whether it can be used (a separator; a quote that isn't part of it; a name).
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
            && !self.name.trim().is_empty()
            && !self.separator.is_empty()
            && !self.separator.contains(['\n', '\r'])
            && self.quote.is_none_or(|q| !self.separator.contains(q) && q != '\n' && q != '\r')
    }

    /// The extension of a file it writes.
    pub fn extension(&self) -> &'static str {
        if self.separator == "\t" { "tsv" } else { "csv" }
    }

    /// One line of the settings: `key=value` pairs separated by `;` (`\`, `;`, `=` escaped
    /// by `\`, a tab written `\t`).
    pub fn encode(&self) -> String {
        let quote = self.quote.map(String::from).unwrap_or_default();
        [
            ("id", self.id.as_str()),
            ("name", self.name.as_str()),
            ("separator", self.separator.as_str()),
            ("quote", quote.as_str()),
            ("header", if self.header { "true" } else { "false" }),
            ("null", self.null_text.as_str()),
            ("eol", self.line_end.id()),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", escape(v)))
        .collect::<Vec<_>>()
        .join(";")
    }

    /// A line [`Self::encode`] wrote (`None`: not one, or not a valid format).
    pub fn decode(line: &str) -> Option<Self> {
        let mut format = Self::new("", "", "", false);
        let mut seen_id = false;
        for pair in split_unescaped(line, ';') {
            let (key, value) = pair.split_once('=')?;
            let value = unescape(value);
            match key.trim() {
                "id" => {
                    format.id = value;
                    seen_id = true;
                }
                "name" => format.name = value,
                "separator" => format.separator = value,
                "quote" => {
                    let mut chars = value.chars();
                    format.quote = chars.next();
                    if chars.next().is_some() {
                        return None;
                    }
                }
                "header" => format.header = value == "true",
                "null" => format.null_text = value,
                "eol" => {
                    format.line_end = match value.as_str() {
                        "crlf" => LineEnd::CrLf,
                        _ => LineEnd::Lf,
                    }
                }
                _ => {}
            }
        }
        (seen_id && format.is_valid()).then_some(format)
    }

    /// A value as a field of a line (quoted when it needs to be).
    pub fn field(&self, value: &str, out: &mut String) {
        let Some(q) = self.quote else {
            out.push_str(value);
            return;
        };
        let needs = value.contains(['\n', '\r'])
            || value.contains(q)
            || value.contains(self.separator.as_str())
            || (!self.null_text.is_empty() && value == self.null_text);
        if needs {
            out.push(q);
            for c in value.chars() {
                if c == q {
                    out.push(q);
                }
                out.push(c);
            }
            out.push(q);
        } else {
            out.push_str(value);
        }
    }
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            '=' => out.push_str("\\="),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `line` split at each `separator` not escaped by `\`.
fn split_unescaped(line: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == separator {
            parts.push(&line[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&line[start..]);
    parts
}

/// The extractor of a [`CsvFormat`].
#[derive(Debug, Clone)]
pub struct Delimited(pub CsvFormat);

impl Extractor for Delimited {
    fn id(&self) -> &str {
        &self.0.id
    }

    fn name(&self) -> String {
        self.0.name.clone()
    }

    fn group(&self) -> Group {
        Group::Delimited
    }

    fn extension(&self) -> &str {
        self.0.extension()
    }

    fn open(&self, input: &Input) -> Box<dyn Stream> {
        Box::new(DelimitedStream {
            format: self.0.clone(),
            names: input.columns.iter().map(|c| c.name.clone()).collect(),
            lines: Lines::new(self.0.line_end.text()),
        })
    }
}

struct DelimitedStream {
    format: CsvFormat,
    names: Vec<String>,
    lines: Lines,
}

impl DelimitedStream {
    fn line<'a>(&self, values: impl Iterator<Item = Option<std::borrow::Cow<'a, str>>>) -> String {
        let mut line = String::new();
        for (i, value) in values.enumerate() {
            if i > 0 {
                line.push_str(&self.format.separator);
            }
            match value {
                Some(text) => self.format.field(&text, &mut line),
                None => line.push_str(&self.format.null_text),
            }
        }
        line
    }
}

impl Stream for DelimitedStream {
    fn begin(&mut self, out: &mut String) {
        if self.format.header {
            let line =
                self.line(self.names.iter().map(|n| Some(std::borrow::Cow::Borrowed(n.as_str()))));
            self.lines.push(out, &line);
        }
    }

    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        for row in rows {
            let line = self.line(row.iter().map(|v| (!v.is_null()).then(|| v.text())));
            self.lines.push(out, &line);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/delimited.rs"]
mod tests;
