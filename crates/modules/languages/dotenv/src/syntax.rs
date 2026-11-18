//! A `.env` file read the way the dotenv loaders read it (Symfony's Dotenv, phpdotenv,
//! Node's dotenv, Docker Compose): an entry per `KEY=value` — `export` before it allowed,
//! spaces around `=` too —, its value unquoted (trimmed, up to a `#` after whitespace), in
//! single quotes (literal) or double quotes (escapes; both may span lines), the variables it
//! interpolates (`$NAME`, `${NAME}`, `${NAME:-default}`; none in single quotes), a comment
//! after it; whole-line comments and blank lines between.
//!
//! Pure and cheap: one pass over the text, every part a byte range of it. The grammar colors
//! the file; this reading is what the sections, the keys, the usages and the checks stand on.

use std::ops::Range;

/// A `.env` file's lines, in order (a value spanning lines is one of them).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvFile {
    pub lines: Vec<Line>,
}

/// A line of the file — or several, for a quoted value going on past its first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// What it covers, without the line break (and without trailing whitespace).
    pub span: Range<usize>,
    /// Its first and last line (0-based).
    pub first: u32,
    pub last: u32,
    pub kind: LineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    /// Nothing but whitespace.
    Blank,
    /// A whole-line comment: its text after the `#`s, trimmed.
    Comment {
        text: Range<usize>,
    },
    Entry(Entry),
    /// Not an entry (`just words`): left as it is.
    Invalid,
}

/// `KEY=value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The `export` before the key.
    pub export: Option<Range<usize>>,
    pub key: Range<usize>,
    pub value: Value,
    /// A comment after the value: from its `#`.
    pub comment: Option<Range<usize>>,
}

/// An entry's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    /// As written, quotes included (empty after `KEY=`).
    pub raw: Range<usize>,
    /// What it holds: inside the quotes, or the unquoted text trimmed.
    pub content: Range<usize>,
    pub quote: Option<Quote>,
    pub interpolations: Vec<Interpolation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    /// `'…'`: literal, nothing interpolated.
    Single,
    /// `"…"`: escapes, variables interpolated.
    Double,
}

/// A variable interpolated in a value: `$NAME`, `${NAME}`, `${NAME:-default}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interpolation {
    /// From the `$` to the end (its `}`).
    pub span: Range<usize>,
    pub name: Range<usize>,
    /// What's used when the variable isn't set (after `:-`, `-`, `:=`, `=`…).
    pub default: Option<Range<usize>>,
}

/// Where a key's name is written in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPlace {
    /// The entry defining it.
    Definition,
    /// A value interpolating it.
    Interpolation,
}

impl EnvFile {
    pub fn parse(text: &str) -> Self {
        let mut lines = Vec::new();
        let (mut at, mut line) = (0, 0u32);
        while at < text.len() {
            let parsed = parse_line(text, at, line);
            let next = line_end(text, parsed.span.end.max(at));
            line = parsed.last + 1;
            lines.push(parsed);
            at = next + 1;
        }
        Self { lines }
    }

    /// The entries, in order, with their lines.
    pub fn entries(&self) -> impl Iterator<Item = (&Line, &Entry)> {
        self.lines.iter().filter_map(|l| match &l.kind {
            LineKind::Entry(e) => Some((l, e)),
            _ => None,
        })
    }

    /// The entries defining `key` (in `text`), in order.
    pub fn definitions<'a, 'k>(
        &'a self,
        text: &'a str,
        key: &'k str,
    ) -> impl Iterator<Item = (&'a Line, &'a Entry)> + use<'a, 'k> {
        self.entries().filter(move |(_, e)| &text[e.key.clone()] == key)
    }

    /// Every place a key's name is written: its definitions and its interpolations, in
    /// order (`(range of the name, what it is)`).
    pub fn names(&self) -> impl Iterator<Item = (Range<usize>, KeyPlace)> + '_ {
        self.entries().flat_map(|(_, e)| {
            std::iter::once((e.key.clone(), KeyPlace::Definition)).chain(
                e.value.interpolations.iter().map(|i| (i.name.clone(), KeyPlace::Interpolation)),
            )
        })
    }

    /// The key's name at `offset` (the cursor on it or right after it), and what it is.
    pub fn name_at(&self, offset: usize) -> Option<(Range<usize>, KeyPlace)> {
        self.names().find(|(r, _)| r.start <= offset && offset <= r.end)
    }

    /// The entry whose lines hold `offset`.
    pub fn entry_at(&self, offset: usize) -> Option<(&Line, &Entry)> {
        self.entries().find(|(l, _)| l.span.start <= offset && offset <= l.span.end)
    }
}

/// A character a key's name starts with / goes on with.
fn starts_name(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn in_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Where the line holding `at` ends (its `\n`, or the text's end).
fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

/// Past the blanks (spaces, tabs) from `at`, up to `end`.
fn skip_blanks(bytes: &[u8], mut at: usize, end: usize) -> usize {
    while at < end && is_blank(bytes[at]) {
        at += 1;
    }
    at
}

/// Back over trailing whitespace (`\r` too) before `end`, down to `start`.
fn trim_end(bytes: &[u8], start: usize, mut end: usize) -> usize {
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    end
}

/// The name from `at` (letters, digits, `_`, starting with a letter or `_`): its end.
fn name_end(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    if at >= end || !starts_name(bytes[at]) {
        return None;
    }
    let mut i = at + 1;
    while i < end && in_name(bytes[i]) {
        i += 1;
    }
    Some(i)
}

/// The line starting at `start` (line number `line`).
fn parse_line(text: &str, start: usize, line: u32) -> Line {
    let bytes = text.as_bytes();
    let end = line_end(text, start);
    let first = skip_blanks(bytes, start, end);
    let content_end = trim_end(bytes, first, end);
    let single = |span: Range<usize>, kind| Line { span, first: line, last: line, kind };
    if first == content_end {
        return single(start..start, LineKind::Blank);
    }
    if bytes[first] == b'#' {
        let mut at = first;
        while at < content_end && bytes[at] == b'#' {
            at += 1;
        }
        let at = skip_blanks(bytes, at, content_end);
        return single(first..content_end, LineKind::Comment { text: at..content_end });
    }
    match parse_entry(text, first, end) {
        Some((entry, span_end)) => {
            let last = line + text[first..span_end].matches('\n').count() as u32;
            Line { span: first..span_end, first: line, last, kind: LineKind::Entry(entry) }
        }
        None => single(first..content_end, LineKind::Invalid),
    }
}

/// `[export ]KEY = value [# comment]` from `at` (the line ending at `end`): the entry and
/// where it ends (past its value's closing quote's line, for one spanning lines).
fn parse_entry(text: &str, at: usize, end: usize) -> Option<(Entry, usize)> {
    let bytes = text.as_bytes();
    let mut export = None;
    let mut key_start = at;
    if text[at..end].starts_with("export") {
        let after = at + "export".len();
        let next = skip_blanks(bytes, after, end);
        if next > after && name_end(bytes, next, end).is_some() {
            export = Some(at..after);
            key_start = next;
        }
    }
    let key_end = name_end(bytes, key_start, end)?;
    let equals = skip_blanks(bytes, key_end, end);
    if bytes.get(equals) != Some(&b'=') {
        return None;
    }
    let value_start = skip_blanks(bytes, equals + 1, end);
    let (value, rest) = parse_value(text, value_start, end);
    // A comment after a quoted value (an unquoted one ends at its comment).
    let line_end_at = line_end(text, rest);
    let after = skip_blanks(bytes, rest, line_end_at);
    let comment_end = trim_end(bytes, after, line_end_at);
    let comment = (after < comment_end && bytes[after] == b'#').then_some(after..comment_end);
    let span_end = comment
        .as_ref()
        .map_or_else(|| trim_end(bytes, value.raw.start, value.raw.end).max(equals + 1), |c| c.end);
    Some((Entry { export, key: key_start..key_end, value, comment }, span_end))
}

/// The value from `at` (its line ending at `end`), and where reading goes on after it.
fn parse_value(text: &str, at: usize, end: usize) -> (Value, usize) {
    let bytes = text.as_bytes();
    let quoted = |quote: Quote, close: Option<usize>| {
        // Never closed: the rest of its line (the next lines stay entries of their own).
        let (content_end, raw_end) = match close {
            Some(close) => (close, close + 1),
            None => {
                let line_end = trim_end(bytes, at + 1, end);
                (line_end, line_end)
            }
        };
        let content = at + 1..content_end;
        let interpolations = match quote {
            Quote::Double => interpolations(text, content.clone()),
            Quote::Single => Vec::new(),
        };
        (Value { raw: at..raw_end, content, quote: Some(quote), interpolations }, raw_end)
    };
    match bytes.get(at) {
        Some(b'\'') if at < end => {
            quoted(Quote::Single, text[at + 1..].find('\'').map(|i| at + 1 + i))
        }
        Some(b'"') if at < end => quoted(Quote::Double, closing_double(bytes, at + 1)),
        _ => {
            // Up to a `#` starting the value or after whitespace.
            let stop = (at..end)
                .find(|&i| bytes[i] == b'#' && (i == at || is_blank(bytes[i - 1])))
                .unwrap_or(end);
            let content = at..trim_end(bytes, at, stop);
            let interpolations = interpolations(text, content.clone());
            (Value { raw: content.clone(), content, quote: None, interpolations }, stop)
        }
    }
}

/// The closing `"` of a double-quoted value opened before `from` (escapes skipped).
fn closing_double(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// The variables `range` of `text` interpolates (`$NAME`, `${NAME}`, `${NAME:-default}`;
/// an escaped `\$` is a dollar).
fn interpolations(text: &str, range: Range<usize>) -> Vec<Interpolation> {
    let bytes = text.as_bytes();
    let end = range.end;
    let mut out = Vec::new();
    let mut i = range.start;
    while i < end {
        match bytes[i] {
            b'\\' => i += 2,
            b'$' if i + 1 < end && bytes[i + 1] == b'{' => {
                let name = i + 2;
                let Some(close) = text[name..end].find('}').map(|c| name + c) else {
                    i += 1;
                    continue;
                };
                let Some(name_end) = name_end(bytes, name, close) else {
                    i += 1;
                    continue;
                };
                // `${NAME:-default}`, `${NAME-default}`: past the operator.
                let default = (name_end < close).then(|| {
                    let skip = if bytes[name_end] == b':' { 2 } else { 1 };
                    (name_end + skip).min(close)..close
                });
                out.push(Interpolation { span: i..close + 1, name: name..name_end, default });
                i = close + 1;
            }
            b'$' => match name_end(bytes, i + 1, end) {
                Some(name_end) => {
                    out.push(Interpolation {
                        span: i..name_end,
                        name: i + 1..name_end,
                        default: None,
                    });
                    i = name_end;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/syntax.rs"]
mod tests;
