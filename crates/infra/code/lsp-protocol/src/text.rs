//! Positions as a server counts them, and a document's text as the server has it.
//!
//! The editor counts bytes of UTF-8 (lines split on `\n` only: the editor keeps `\r\n` as
//! the file's line ending, not in its text); a server counts lines and, in a line, UTF-16
//! code units — or UTF-8 / UTF-32 ones if both agree at `initialize`. [`Mirror`] is the
//! text a server was last sent: offsets and positions are converted against it, and a new
//! version becomes the one change (the span that differs) sent with `didChange`.

use std::ops::Range;
use std::sync::Arc;

use serde_json::{Value, json};

/// How a server counts a line's characters (LSP 3.17 `positionEncoding`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    Utf8,
    #[default]
    Utf16,
    Utf32,
}

impl Encoding {
    /// As LSP names it (`utf-16`); an unknown name is UTF-16 (the protocol's default).
    pub fn from_name(name: Option<&str>) -> Self {
        match name {
            Some("utf-8") => Self::Utf8,
            Some("utf-32") => Self::Utf32,
            _ => Self::Utf16,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
            Self::Utf32 => "utf-32",
        }
    }

    /// How many units `c` takes.
    pub fn units(self, c: char) -> usize {
        match self {
            Self::Utf8 => c.len_utf8(),
            Self::Utf16 => c.len_utf16(),
            Self::Utf32 => 1,
        }
    }

    /// How many units `s` takes.
    pub fn len(self, s: &str) -> usize {
        match self {
            Self::Utf8 => s.len(),
            Self::Utf16 => s.chars().map(char::len_utf16).sum(),
            Self::Utf32 => s.chars().count(),
        }
    }
}

/// A place in a document as LSP says it: a 0-based line and a column in units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }

    pub fn to_value(self) -> Value {
        json!({"line": self.line, "character": self.character})
    }

    /// From LSP's JSON (`None` if it isn't a position).
    pub fn from_value(value: &Value) -> Option<Self> {
        let line = value.get("line")?.as_u64()?;
        let character = value.get("character")?.as_u64()?;
        Some(Self::new(
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(character).unwrap_or(u32::MAX),
        ))
    }
}

/// A span as LSP says it.
pub fn range_value(start: Position, end: Position) -> Value {
    json!({"start": start.to_value(), "end": end.to_value()})
}

/// A span from LSP's JSON.
pub fn range_from_value(value: &Value) -> Option<(Position, Position)> {
    Some((Position::from_value(value.get("start")?)?, Position::from_value(value.get("end")?)?))
}

/// The byte where column `column` (in `encoding`'s units) falls in `line`: clamped to the
/// line's end, and to a character's start when it falls inside one (a surrogate pair's
/// middle).
pub fn column_to_byte(line: &str, column: usize, encoding: Encoding) -> usize {
    if encoding == Encoding::Utf8 {
        let mut at = column.min(line.len());
        while !line.is_char_boundary(at) {
            at -= 1;
        }
        return at;
    }
    let mut units = 0;
    for (i, c) in line.char_indices() {
        if units >= column {
            return i;
        }
        units += encoding.units(c);
        if units > column {
            return i;
        }
    }
    line.len()
}

/// A text with its lines' starts: offsets and positions converted both ways.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lines {
    text: String,
    /// Where each line starts (bytes); never empty.
    starts: Vec<usize>,
}

impl Lines {
    pub fn new(text: String) -> Self {
        let mut lines = Self { text, starts: Vec::new() };
        lines.index_from(0);
        lines
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// Indexes the starts of the lines after line `line` again.
    fn index_from(&mut self, line: usize) {
        self.starts.truncate(line + 1);
        if self.starts.is_empty() {
            self.starts.push(0);
        }
        let from = self.starts[self.starts.len() - 1];
        let bytes = self.text.as_bytes();
        self.starts.extend(
            bytes[from..]
                .iter()
                .enumerate()
                .filter(|(_, b)| **b == b'\n')
                .map(|(i, _)| from + i + 1),
        );
    }

    /// Line `line`'s text, without its `\n` (and a `\r` before it); empty past the end.
    pub fn line(&self, line: usize) -> &str {
        let Some(&start) = self.starts.get(line) else { return "" };
        let end = self.starts.get(line + 1).map_or(self.text.len(), |next| next - 1);
        let text = &self.text[start..end];
        text.strip_suffix('\r').unwrap_or(text)
    }

    /// The line holding byte `offset` (clamped to the text).
    pub fn line_of(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        self.starts.partition_point(|&s| s <= offset).saturating_sub(1)
    }

    /// Byte `offset` as a position (clamped to the text, and to a character's start).
    pub fn position(&self, offset: usize, encoding: Encoding) -> Position {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_of(offset);
        let start = self.starts[line];
        let column = encoding.len(&self.text[start..offset]);
        Position::new(line as u32, u32::try_from(column).unwrap_or(u32::MAX))
    }

    /// Position `position` as a byte offset: a line past the end is the text's end, a
    /// column past the line's end is the line's end.
    pub fn offset(&self, position: Position, encoding: Encoding) -> usize {
        let line = position.line as usize;
        let Some(&start) = self.starts.get(line) else { return self.text.len() };
        start + column_to_byte(self.line(line), position.character as usize, encoding)
    }

    /// A span of bytes as positions.
    pub fn range(&self, range: &Range<usize>, encoding: Encoding) -> (Position, Position) {
        (self.position(range.start, encoding), self.position(range.end, encoding))
    }

    /// Positions as a span of bytes (the end never before the start).
    pub fn bytes(&self, start: Position, end: Position, encoding: Encoding) -> Range<usize> {
        let start = self.offset(start, encoding);
        start..self.offset(end, encoding).max(start)
    }

    /// Replaces `range` (bytes, on character boundaries) with `with`.
    pub fn replace(&mut self, range: Range<usize>, with: &str) {
        let line = self.line_of(range.start);
        self.text.replace_range(range, with);
        self.index_from(line);
    }
}

/// One change sent with `didChange`: a span of the text the server has (its positions),
/// and what replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub start: Position,
    pub end: Position,
    pub text: String,
}

impl Change {
    pub fn to_value(&self) -> Value {
        json!({"range": range_value(self.start, self.end), "text": self.text})
    }
}

/// The span where `old` and `new` differ: the bytes `old[start..old_end]` replaced by
/// `new[start..new_end]` (on character boundaries). `None`: they're the same.
pub fn differing(old: &str, new: &str) -> Option<(usize, usize, usize)> {
    if old == new {
        return None;
    }
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let most = a.len().min(b.len()) - start;
    let mut suffix =
        a.iter().rev().zip(b.iter().rev()).take(most).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(a.len() - suffix) || !new.is_char_boundary(b.len() - suffix) {
        suffix -= 1;
    }
    Some((start, a.len() - suffix, b.len() - suffix))
}

/// A document's text as the server has it, and its version there. Its lines are shared
/// with the requests still about this version (copied only if one is when it changes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    lines: Arc<Lines>,
    /// The editor's version this text is of.
    pub version: u64,
}

impl Mirror {
    pub fn new(text: String, version: u64) -> Self {
        Self { lines: Arc::new(Lines::new(text)), version }
    }

    pub fn lines(&self) -> &Lines {
        &self.lines
    }

    /// Its lines as they are now, kept by whoever holds them (a request's answer is about
    /// this version).
    pub fn shared(&self) -> Arc<Lines> {
        Arc::clone(&self.lines)
    }

    pub fn text(&self) -> &str {
        self.lines.text()
    }

    /// The text becomes `new` (the editor's version `version`): the change to send, in
    /// the old text's positions (`None`: nothing differs).
    pub fn update(&mut self, new: &str, version: u64, encoding: Encoding) -> Option<Change> {
        self.version = version;
        let (start, old_end, new_end) = differing(self.lines.text(), new)?;
        let change = Change {
            start: self.lines.position(start, encoding),
            end: self.lines.position(old_end, encoding),
            text: new[start..new_end].to_owned(),
        };
        Arc::make_mut(&mut self.lines).replace(start..old_end, &new[start..new_end]);
        Some(change)
    }
}

#[cfg(test)]
#[path = "../tests/unit/text.rs"]
mod tests;
