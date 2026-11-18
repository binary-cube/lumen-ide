//! The text buffer: a rope (see ADR-0004) hidden behind a byte-offset API.
//!
//! Internally, lines are separated only by `\n`; the original terminator (LF / CRLF) is
//! kept in [`crate::LineEnding`] and reapplied on save.

use std::ops::Range;

use kernel::TextPosition;
use ropey::Rope;

#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    rope: Rope,
}

/// An immutable, cheap (O(1)) copy of the text, for work on other threads.
#[derive(Debug, Clone)]
pub struct TextSnapshot {
    rope: Rope,
}

impl TextSnapshot {
    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn write_line(&self, line: usize, out: &mut String) {
        write_line(&self.rope, line, out);
    }

    /// The bytes from `offset` to the end of the rope chunk that contains it. Empty past
    /// the end of the text. For parsers that read incrementally (bytes: an offset inside
    /// a multi-byte character is answered, not a panic).
    pub fn chunk_from(&self, offset: usize) -> &[u8] {
        if offset >= self.rope.len_bytes() {
            return &[];
        }
        let (chunk, chunk_start, _, _) = self.rope.chunk_at_byte(offset);
        &chunk.as_bytes()[offset - chunk_start..]
    }

    /// The text chunks covering the range (without copying).
    pub fn chunks_in(&self, range: std::ops::Range<usize>) -> impl Iterator<Item = &str> {
        // The ends are aligned to a character boundary (an offset in the middle of a
        // UTF-8 character, e.g. `start + 256` over a diacritic, must not break anything).
        let len = self.rope.len_bytes();
        let floor = |b: usize| self.rope.char_to_byte(self.rope.byte_to_char(b.min(len)));
        let end = floor(range.end);
        let start = floor(range.start).min(end);
        self.rope.byte_slice(start..end).chunks()
    }

    pub fn slice(&self, range: std::ops::Range<usize>) -> String {
        self.chunks_in(range).collect()
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.byte_to_line(offset.min(self.rope.len_bytes()))
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_byte(line.min(self.rope.len_lines()))
    }

    pub fn position(&self, offset: usize) -> TextPosition {
        let offset = offset.min(self.rope.len_bytes());
        let line = self.line_of(offset);
        TextPosition::new(line as u32, (offset - self.line_start(line)) as u32)
    }
}

impl TextBuffer {
    pub fn new(text: &str) -> Self {
        Self { rope: Rope::from_str(text) }
    }

    /// The length in UTF-8 bytes.
    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// The number of lines; a text ending with `\n` has an empty last line.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// The line containing the offset (clamped at the end).
    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.byte_to_line(offset.min(self.len()))
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_byte(line.min(self.line_count()))
    }

    /// The end of the line, without `\n`.
    pub fn line_end(&self, line: usize) -> usize {
        let start = self.line_start(line);
        start + self.line_len(line)
    }

    /// The line's length in bytes, without `\n`.
    pub fn line_len(&self, line: usize) -> usize {
        if line >= self.line_count() {
            return 0;
        }
        let slice = self.rope.line(line);
        let len = slice.len_bytes();
        if len > 0 && slice.byte(len - 1) == b'\n' { len - 1 } else { len }
    }

    /// Writes the line's content (without `\n`) into `out`, reusing its memory.
    pub fn write_line(&self, line: usize, out: &mut String) {
        write_line(&self.rope, line, out);
    }

    pub fn line_text(&self, line: usize) -> String {
        let mut s = String::new();
        self.write_line(line, &mut s);
        s
    }

    /// The text in `range`, its ends clamped and moved back to a char's start (a range kept
    /// from an older version may fall inside one now: never a panic).
    pub fn slice(&self, range: Range<usize>) -> String {
        let range = self.clamp(range);
        let (start, end) =
            (self.floor_char_boundary(range.start), self.floor_char_boundary(range.end));
        self.rope.byte_slice(start..end.max(start)).to_string()
    }

    /// The nearest char boundary at or before `offset` (O(log n), no copy).
    pub fn floor_char_boundary(&self, offset: usize) -> usize {
        let offset = offset.min(self.rope.len_bytes());
        self.rope.char_to_byte(self.rope.byte_to_char(offset))
    }

    /// How many characters the range has (O(log n), without copying).
    pub fn char_count(&self, range: Range<usize>) -> usize {
        let range = self.clamp(range);
        self.rope.byte_to_char(range.end) - self.rope.byte_to_char(range.start)
    }

    pub fn char_before(&self, offset: usize) -> Option<char> {
        let offset = offset.min(self.len());
        let idx = self.rope.byte_to_char(offset);
        (idx > 0).then(|| self.rope.char(idx - 1))
    }

    pub fn char_after(&self, offset: usize) -> Option<char> {
        let idx = self.rope.byte_to_char(offset.min(self.len()));
        (idx < self.rope.len_chars()).then(|| self.rope.char(idx))
    }

    pub fn position(&self, offset: usize) -> TextPosition {
        let offset = offset.min(self.len());
        let line = self.line_of(offset);
        TextPosition::new(line as u32, (offset - self.line_start(line)) as u32)
    }

    pub fn offset(&self, pos: TextPosition) -> usize {
        let line = (pos.line.0 as usize).min(self.line_count().saturating_sub(1));
        self.line_start(line) + (pos.column.0 as usize).min(self.line_len(line))
    }

    /// Replaces the byte range with `text`. The ends must be character boundaries.
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let range = self.clamp(range);
        let start = self.rope.byte_to_char(range.start);
        let end = self.rope.byte_to_char(range.end);
        if start < end {
            self.rope.remove(start..end);
        }
        if !text.is_empty() {
            self.rope.insert(start, text);
        }
    }

    pub fn snapshot(&self) -> TextSnapshot {
        TextSnapshot { rope: self.rope.clone() }
    }

    pub fn chunks(&self) -> impl Iterator<Item = &str> {
        self.rope.chunks()
    }

    /// The first `needle` at or after `from` (as `text[from..].find`), searched over the
    /// rope's chunks: nothing copied but a needle's length around each chunk's end.
    pub fn find(&self, needle: &str, from: usize) -> Option<usize> {
        self.search(needle, from, 1).first().copied()
    }

    /// Every `needle`, apart (as `text.match_indices`), over the chunks.
    pub fn find_all(&self, needle: &str) -> Vec<usize> {
        self.search(needle, 0, usize::MAX)
    }

    fn search(&self, needle: &str, from: usize, limit: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let len = self.len();
        if needle.is_empty() || limit == 0 || from > len {
            return out;
        }
        let from = self.floor_char_boundary(from);
        // The chunk being searched, after the end of the ones before that may start a match.
        let mut window = String::new();
        let mut window_start = from;
        // Where the next match may start (after the last one: they don't overlap).
        let mut next = from;
        let keep = needle.len() - 1;
        for chunk in self.rope.byte_slice(from..len).chunks() {
            window.push_str(chunk);
            let at = next - window_start;
            for (i, _) in window[at..].match_indices(needle) {
                let start = window_start + at + i;
                out.push(start);
                if out.len() >= limit {
                    return out;
                }
                next = start + needle.len();
            }
            if window.len() > keep {
                let mut cut = window.len() - keep;
                while !window.is_char_boundary(cut) {
                    cut -= 1;
                }
                window.drain(..cut);
                window_start += cut;
                next = next.max(window_start);
            }
        }
        out
    }

    /// The whole text, copied into a string of exactly its size (`to_string` goes through
    /// `Display`, growing the string by doubling: about twice the bytes allocated and copied).
    pub fn text(&self) -> String {
        let mut text = String::with_capacity(self.len());
        for chunk in self.rope.chunks() {
            text.push_str(chunk);
        }
        text
    }

    fn clamp(&self, range: Range<usize>) -> Range<usize> {
        let len = self.len();
        range.start.min(len)..range.end.clamp(range.start.min(len), len)
    }
}

impl std::fmt::Display for TextBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for chunk in self.rope.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

fn write_line(rope: &Rope, line: usize, out: &mut String) {
    out.clear();
    if line >= rope.len_lines() {
        return;
    }
    for chunk in rope.line(line).chunks() {
        out.push_str(chunk);
    }
    if out.ends_with('\n') {
        out.pop();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/domain/buffer.rs"]
mod tests;
