//! An engine's error position taken back to the document: the statement sent may not be the
//! document's text (its values written in, [`BindMap`]), and engines count differently.

use super::params::BindMap;

/// Where an engine says an error is, in the text it was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePosition {
    /// A character, counted from 1 (PostgreSQL's `position`).
    Character(usize),
    /// A byte offset, from 0 (SQLite's `sqlite3_error_offset`).
    Byte(usize),
}

/// A place in a document: its byte offset, and its line and column (both from 0; the column
/// in characters). `\n`, `\r\n` and `\r` each end a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl TextPosition {
    /// The place of byte `offset` in `text` (kept within it, on a char boundary).
    pub fn of(text: &str, offset: usize) -> Self {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let before = &text[..offset];
        let bytes = before.as_bytes();
        let mut line = 0;
        let mut line_start = 0;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    line += 1;
                    line_start = i + 1;
                }
                b'\r' => {
                    line += 1;
                    if bytes.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                    line_start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        let column = text[line_start..offset].chars().count();
        Self { offset, line, column }
    }
}

/// Where in `document` the error at `position` in `sent` (the text sent, mapped by `map`)
/// is; `None` when the position isn't one (0 for a character). A position past the end
/// (PostgreSQL's "at end of input") is the statement's end.
pub fn locate(
    document: &str,
    sent: &str,
    map: &BindMap,
    position: EnginePosition,
) -> Option<TextPosition> {
    let sent_offset = match position {
        EnginePosition::Character(0) => return None,
        EnginePosition::Character(n) => {
            sent.char_indices().nth(n - 1).map_or(sent.len(), |(i, _)| i)
        }
        EnginePosition::Byte(n) => {
            let mut n = n.min(sent.len());
            while !sent.is_char_boundary(n) {
                n -= 1;
            }
            n
        }
    };
    Some(TextPosition::of(document, map.to_document(sent_offset)))
}

#[cfg(test)]
#[path = "../../tests/unit/sql/errors.rs"]
mod tests;
