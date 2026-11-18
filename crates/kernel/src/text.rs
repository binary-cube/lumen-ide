//! Positions and ranges in text.
//!
//! Internally we work in **UTF-8 byte offsets**. Conversions to UTF-16 (LSP) or to
//! visual columns (tabs, wide characters) happen only at the system boundary.

use std::fmt;
use std::ops::Range;

/// Offset in UTF-8 bytes from the start of the document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteOffset(pub usize);

/// Line, 0-indexed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Line(pub u32);

/// Column in UTF-8 bytes from the start of the line, 0-indexed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Column(pub u32);

/// Position (line, column); the derived ordering is document order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextPosition {
    pub line: Line,
    pub column: Column,
}

impl TextPosition {
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line: Line(line), column: Column(column) }
    }
}

impl fmt::Display for TextPosition {
    /// Display for the user, 1-indexed (as in the status bar).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line.0 + 1, self.column.0 + 1)
    }
}

/// Half-open byte range `[start, end)`. Invariant: `start <= end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ByteRange {
    start: ByteOffset,
    end: ByteOffset,
}

impl ByteRange {
    /// Builds the range, ordering the ends if they are reversed.
    pub fn new(a: ByteOffset, b: ByteOffset) -> Self {
        if a <= b { Self { start: a, end: b } } else { Self { start: b, end: a } }
    }

    pub const fn empty(at: ByteOffset) -> Self {
        Self { start: at, end: at }
    }

    pub const fn start(&self) -> ByteOffset {
        self.start
    }

    pub const fn end(&self) -> ByteOffset {
        self.end
    }

    pub const fn len(&self) -> usize {
        self.end.0 - self.start.0
    }

    pub const fn is_empty(&self) -> bool {
        self.start.0 == self.end.0
    }

    pub fn contains(&self, offset: ByteOffset) -> bool {
        self.start <= offset && offset < self.end
    }

    pub fn intersects(&self, other: &ByteRange) -> bool {
        self.start < other.end && other.start < self.end
    }
}

impl From<Range<usize>> for ByteRange {
    fn from(r: Range<usize>) -> Self {
        Self::new(ByteOffset(r.start), ByteOffset(r.end))
    }
}

/// Half-open range `[start, end)` in (line, column) coordinates. Invariant: `start <= end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TextRange {
    start: TextPosition,
    end: TextPosition,
}

impl TextRange {
    /// Builds the range, ordering the ends if they are reversed.
    pub fn new(a: TextPosition, b: TextPosition) -> Self {
        if a <= b { Self { start: a, end: b } } else { Self { start: b, end: a } }
    }

    pub const fn start(&self) -> TextPosition {
        self.start
    }

    pub const fn end(&self) -> TextPosition {
        self.end
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, pos: TextPosition) -> bool {
        self.start <= pos && pos < self.end
    }
}

/// The line (0-based) of byte `offset` in `text`: the newlines before it (an offset past the
/// end counts to the end).
pub fn line_of(text: &str, offset: usize) -> u32 {
    text.as_bytes()[..offset.min(text.len())].iter().filter(|b| **b == b'\n').count() as u32
}

/// `text` written on one line, for a message, a tooltip or a label shaped as a single line:
/// each run of whitespace (newlines and tabs included) becomes one space and the ends are
/// trimmed. Borrows when nothing changes (the usual case: no allocation).
pub fn one_line(text: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = text.trim();
    let mut prev_space = false;
    let clean = trimmed.chars().all(|c| {
        let ok = if c.is_whitespace() { c == ' ' && !prev_space } else { true };
        prev_space = c.is_whitespace();
        ok
    });
    if clean {
        return std::borrow::Cow::Borrowed(trimmed);
    }
    let mut out = String::with_capacity(trimmed.len());
    for word in trimmed.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    std::borrow::Cow::Owned(out)
}

/// `text` at most `max` characters, cut in the middle (`asdfasdf…sad.php`: its start and its
/// end — a file's extension — kept), where nothing measures it in pixels (a window's title,
/// the system's menus).
pub fn shorten_middle(text: &str, max: usize) -> std::borrow::Cow<'_, str> {
    let count = text.chars().count();
    if count <= max || max < 3 {
        return std::borrow::Cow::Borrowed(text);
    }
    let keep = max - 1;
    let tail = keep / 2;
    let head = keep - tail;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    std::borrow::Cow::Owned(format!("{start}…{end}"))
}

#[cfg(test)]
#[path = "../tests/unit/text.rs"]
mod tests;
