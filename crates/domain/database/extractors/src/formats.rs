//! The other built-in formats: JSON (an array of objects, values keeping their types),
//! Markdown (a table, numbers aligned right), HTML (a table), XML (`<data>` of `<row>`s),
//! one row (every value on one line, separated by commas) and Pretty (a text table aligned
//! in columns, as a terminal client prints it).

use std::fmt::Write as _;

use crate::cell::{Cell, Column, hex};
use crate::extractor::{Extractor, Group, Input, Lines, Stream};

macro_rules! simple {
    ($ty:ident, $id:literal, $key:literal, $ext:literal, $names:expr) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $ty;

        impl $ty {
            fn info() -> (&'static str, &'static str, &'static str) {
                ($id, $key, $ext)
            }

            /// The file names' extensions that ask for it.
            const NAMES: &'static [&'static str] = $names;
        }
    };
}

simple!(Json, "json", "extract.json", "json", &["json"]);
simple!(Markdown, "markdown", "extract.markdown", "md", &["md", "markdown"]);
simple!(Html, "html", "extract.html", "html", &["html", "htm"]);
simple!(Xml, "xml", "extract.xml", "xml", &["xml"]);
simple!(OneRow, "one-row", "extract.one_row", "txt", &[]);
// A text file: the table as a terminal prints it.
simple!(Pretty, "pretty", "extract.pretty", "txt", &["txt"]);

macro_rules! extractor {
    ($ty:ident, $open:expr) => {
        impl Extractor for $ty {
            fn id(&self) -> &str {
                Self::info().0
            }

            fn name(&self) -> String {
                i18n::t(Self::info().1)
            }

            fn group(&self) -> Group {
                Group::Other
            }

            fn extension(&self) -> &str {
                Self::info().2
            }

            fn names_files(&self) -> &[&str] {
                Self::NAMES
            }

            fn open(&self, input: &Input) -> Box<dyn Stream> {
                let open: fn(&Input) -> Box<dyn Stream> = $open;
                open(input)
            }
        }
    };
}

extractor!(Json, |input| Box::new(JsonStream { names: names(input), written: 0 }));
extractor!(Markdown, |input| Box::new(MarkdownStream {
    columns: input.columns.clone(),
    lines: Lines::new("\n"),
}));
extractor!(Html, |input| Box::new(HtmlStream { names: names(input) }));
extractor!(Xml, |input| Box::new(XmlStream { table: input.table.clone(), names: names(input) }));
extractor!(OneRow, |_| Box::new(OneRowStream { written: 0 }));
extractor!(Pretty, |input| Box::new(PrettyStream {
    columns: input.columns.clone(),
    widths: Vec::new(),
    started: false,
}));

fn names(input: &Input) -> Vec<String> {
    input.columns.iter().map(|c| c.name.clone()).collect()
}

/// `text` as a JSON string.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A value as JSON: numbers and booleans as they are, the engine's JSON as it is (blank:
/// `null`), bytes as a `0x…` string, the rest as strings.
pub fn json_value(value: Cell<'_>) -> String {
    match value {
        Cell::Null => "null".to_owned(),
        Cell::Bool(b) => b.to_string(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) if f.is_finite() => f.to_string(),
        Cell::Float(_) => "null".to_owned(),
        Cell::Json(t) if !t.trim().is_empty() => t.trim().to_owned(),
        Cell::Json(_) => "null".to_owned(),
        Cell::Bytes(b) => json_string(&format!("0x{}", hex(b))),
        Cell::Text(t) | Cell::Other(t) => json_string(t),
    }
}

struct JsonStream {
    names: Vec<String>,
    written: u64,
}

impl Stream for JsonStream {
    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        for row in rows {
            out.push_str(if self.written == 0 { "[\n  {" } else { ",\n  {" });
            for (i, (name, value)) in self.names.iter().zip(row).enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&json_string(name));
                out.push_str(": ");
                out.push_str(&json_value(*value));
            }
            out.push('}');
            self.written += 1;
        }
    }

    fn finish(&mut self, out: &mut String) {
        out.push_str(if self.written == 0 { "[]" } else { "\n]" });
    }
}

/// A Markdown cell: pipes escaped, line breaks as `<br>`.
pub fn md_cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace(['\n', '\r'], "<br>")
}

struct MarkdownStream {
    columns: Vec<Column>,
    lines: Lines,
}

impl Stream for MarkdownStream {
    fn begin(&mut self, out: &mut String) {
        let names: Vec<String> = self.columns.iter().map(|c| md_cell(&c.name)).collect();
        self.lines.push(out, &format!("| {} |", names.join(" | ")));
        let rules: Vec<&str> =
            self.columns.iter().map(|c| if c.kind.is_number() { "---:" } else { "---" }).collect();
        self.lines.push(out, &format!("| {} |", rules.join(" | ")));
    }

    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        for row in rows {
            let cells: Vec<String> = row
                .iter()
                .map(|v| if v.is_null() { "NULL".to_owned() } else { md_cell(&v.text()) })
                .collect();
            self.lines.push(out, &format!("| {} |", cells.join(" | ")));
        }
    }
}

/// `text` escaped for HTML and XML (`&`, `<`, `>`, `"`, `'`); characters XML can't hold
/// (controls other than tab and line breaks) replaced by `�`.
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

struct HtmlStream {
    names: Vec<String>,
}

impl Stream for HtmlStream {
    fn begin(&mut self, out: &mut String) {
        out.push_str("<table>\n  <tr>");
        for name in &self.names {
            let _ = write!(out, "<th>{}</th>", xml_escape(name));
        }
        out.push_str("</tr>");
    }

    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        for row in rows {
            out.push_str("\n  <tr>");
            for value in row {
                if value.is_null() {
                    out.push_str("<td></td>");
                } else {
                    let text = xml_escape(&value.text())
                        .replace("\r\n", "<br>")
                        .replace(['\n', '\r'], "<br>");
                    let _ = write!(out, "<td>{text}</td>");
                }
            }
            out.push_str("</tr>");
        }
    }

    fn finish(&mut self, out: &mut String) {
        out.push_str("\n</table>");
    }
}

struct XmlStream {
    table: String,
    names: Vec<String>,
}

impl Stream for XmlStream {
    fn begin(&mut self, out: &mut String) {
        let _ = write!(
            out,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<data table=\"{}\">",
            xml_escape(&self.table)
        );
    }

    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        for row in rows {
            out.push_str("\n  <row>");
            for (name, value) in self.names.iter().zip(row) {
                let name = xml_escape(name);
                if value.is_null() {
                    let _ = write!(out, "\n    <column name=\"{name}\" null=\"true\"/>");
                } else {
                    let _ = write!(
                        out,
                        "\n    <column name=\"{name}\">{}</column>",
                        xml_escape(&value.text())
                    );
                }
            }
            out.push_str("\n  </row>");
        }
    }

    fn finish(&mut self, out: &mut String) {
        out.push_str("\n</data>");
    }
}

struct OneRowStream {
    written: u64,
}

impl Stream for OneRowStream {
    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        let format = crate::delimited::CsvFormat::new("one-row", "", ",", false);
        for value in rows.iter().flatten() {
            if self.written > 0 {
                out.push(',');
            }
            if !value.is_null() {
                format.field(&value.text(), out);
            }
            self.written += 1;
        }
    }
}

/// A cell of the text table: `NULL` as such, line breaks and tabs shown as escapes.
fn pretty_cell(value: Cell<'_>) -> String {
    if value.is_null() {
        return "NULL".to_owned();
    }
    value
        .text()
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn width(text: &str) -> usize {
    text.chars().count()
}

struct PrettyStream {
    columns: Vec<Column>,
    /// Each column's width, from its name and the first block of rows (a later wider value
    /// just stretches its own line).
    widths: Vec<usize>,
    started: bool,
}

impl PrettyStream {
    fn rule(&self, out: &mut String) {
        out.push('+');
        for w in &self.widths {
            out.push_str(&"-".repeat(w + 2));
            out.push('+');
        }
    }

    fn line(&self, out: &mut String, cells: &[String], right: &[bool]) {
        out.push('|');
        for (i, cell) in cells.iter().enumerate() {
            let pad = self.widths.get(i).copied().unwrap_or(0).saturating_sub(width(cell));
            out.push(' ');
            if right.get(i).copied().unwrap_or(false) {
                out.push_str(&" ".repeat(pad));
                out.push_str(cell);
            } else {
                out.push_str(cell);
                out.push_str(&" ".repeat(pad));
            }
            out.push_str(" |");
        }
    }

    fn start(&mut self, out: &mut String, first: &[Vec<String>]) {
        self.started = true;
        self.widths = self.columns.iter().map(|c| width(&c.name)).collect();
        for row in first {
            for (w, cell) in self.widths.iter_mut().zip(row) {
                *w = (*w).max(width(cell));
            }
        }
        let names: Vec<String> = self.columns.iter().map(|c| c.name.clone()).collect();
        self.rule(out);
        out.push('\n');
        self.line(out, &names, &[]);
        out.push('\n');
        self.rule(out);
    }
}

impl Stream for PrettyStream {
    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        let cells: Vec<Vec<String>> =
            rows.iter().map(|row| row.iter().map(|v| pretty_cell(*v)).collect()).collect();
        if !self.started {
            self.start(out, &cells);
        }
        let right: Vec<bool> = self.columns.iter().map(|c| c.kind.is_number()).collect();
        for (row, values) in cells.iter().zip(rows) {
            let right: Vec<bool> = right
                .iter()
                .zip(values)
                .map(|(r, v)| *r && !v.is_null() || v.is_number())
                .collect();
            out.push('\n');
            self.line(out, row, &right);
        }
    }

    fn finish(&mut self, out: &mut String) {
        if !self.started {
            self.start(out, &[]);
        }
        out.push('\n');
        self.rule(out);
    }
}

#[cfg(test)]
#[path = "../tests/unit/formats.rs"]
mod tests;
