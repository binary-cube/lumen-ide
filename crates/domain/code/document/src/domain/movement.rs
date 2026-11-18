//! Cursor movements: graphemes, words, lines, visual columns.

use unicode_segmentation::UnicodeSegmentation;

use super::buffer::TextBuffer;

/// The direction or target of a cursor movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    /// The line's first non-whitespace character; if already there, the start of the line.
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    PageUp(u32),
    PageDown(u32),
}

/// The grapheme boundary before `offset` (a composed emoji or a letter with a combining
/// diacritic are traversed in one step).
pub fn prev_grapheme(buffer: &TextBuffer, offset: usize) -> usize {
    let line = buffer.line_of(offset);
    let start = buffer.line_start(line);
    if offset <= start {
        return offset.saturating_sub(1).max(if line == 0 { 0 } else { start - 1 });
    }
    let text = buffer.line_text(line);
    let col = offset - start;
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|&i| i < col)
        .last()
        .map_or(start, |i| start + i)
}

pub fn next_grapheme(buffer: &TextBuffer, offset: usize) -> usize {
    let line = buffer.line_of(offset);
    let start = buffer.line_start(line);
    let end = buffer.line_end(line);
    if offset >= end {
        return if line + 1 < buffer.line_count() { buffer.line_start(line + 1) } else { end };
    }
    let text = buffer.line_text(line);
    let col = offset - start;
    text.grapheme_indices(true)
        .map(|(i, g)| i + g.len())
        .find(|&i| i > col)
        .map_or(end, |i| start + i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

/// The start of the word to the left (skips whitespace, then characters of the same class).
pub fn word_left(buffer: &TextBuffer, offset: usize) -> usize {
    let line = buffer.line_of(offset);
    let start = buffer.line_start(line);
    if offset <= start {
        return prev_grapheme(buffer, offset);
    }
    let text = buffer.line_text(line);
    let mut col = offset - start;
    let mut chars = text[..col].char_indices().rev().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if class(c) != CharClass::Space {
            break;
        }
        col = i;
        chars.next();
    }
    if let Some(&(_, c)) = chars.peek() {
        let cls = class(c);
        for (i, c) in chars {
            if class(c) != cls {
                break;
            }
            col = i;
        }
    }
    start + col
}

/// The end of the word to the right.
pub fn word_right(buffer: &TextBuffer, offset: usize) -> usize {
    let line = buffer.line_of(offset);
    let start = buffer.line_start(line);
    let end = buffer.line_end(line);
    if offset >= end {
        return next_grapheme(buffer, offset);
    }
    let text = buffer.line_text(line);
    let mut col = offset - start;
    let base = col;
    let mut chars = text[base..].char_indices().map(|(i, c)| (i + base, c)).peekable();
    while let Some(&(i, c)) = chars.peek() {
        if class(c) != CharClass::Space {
            break;
        }
        col = i + c.len_utf8();
        chars.next();
    }
    if let Some(&(_, c)) = chars.peek() {
        let cls = class(c);
        for (i, c) in chars {
            if class(c) != cls {
                break;
            }
            col = i + c.len_utf8();
        }
    }
    start + col
}

/// The visual column of an offset (tabs expand to the next multiple of `tab_width`).
pub fn visual_column(buffer: &TextBuffer, offset: usize, tab_width: u32) -> u32 {
    let line = buffer.line_of(offset);
    let start = buffer.line_start(line);
    let text = buffer.line_text(line);
    visual_width(&text[..(offset - start).min(text.len())], tab_width)
}

pub fn visual_width(text: &str, tab_width: u32) -> u32 {
    text.chars()
        .fold(0, |col, c| if c == '\t' { col + tab_width - col % tab_width } else { col + 1 })
}

/// The offset on `line` at visual column `column` (or the end of the line).
pub fn offset_at_column(buffer: &TextBuffer, line: usize, column: u32, tab_width: u32) -> usize {
    let start = buffer.line_start(line);
    let text = buffer.line_text(line);
    let mut col = 0;
    for (i, c) in text.char_indices() {
        let width = if c == '\t' { tab_width - col % tab_width } else { 1 };
        if col + width > column {
            // Halfway through a tab: go to the nearest edge.
            return start + if column - col > width / 2 { i + c.len_utf8() } else { i };
        }
        col += width;
    }
    start + text.len()
}

/// The target of a movement starting from `head`. Returns the new offset and the desired column.
pub fn move_head(
    buffer: &TextBuffer,
    head: usize,
    goal: Option<u32>,
    motion: Motion,
    tab_width: u32,
) -> (usize, Option<u32>) {
    let vertical = |lines: isize| {
        let goal = goal.unwrap_or_else(|| visual_column(buffer, head, tab_width));
        let line = buffer.line_of(head) as isize + lines;
        if line < 0 {
            return (0, Some(goal));
        }
        if line as usize >= buffer.line_count() {
            return (buffer.len(), Some(goal));
        }
        (offset_at_column(buffer, line as usize, goal, tab_width), Some(goal))
    };
    match motion {
        Motion::Left => (prev_grapheme(buffer, head), None),
        Motion::Right => (next_grapheme(buffer, head), None),
        Motion::Up => vertical(-1),
        Motion::Down => vertical(1),
        Motion::PageUp(n) => vertical(-(n as isize)),
        Motion::PageDown(n) => vertical(n as isize),
        Motion::WordLeft => (word_left(buffer, head), None),
        Motion::WordRight => (word_right(buffer, head), None),
        Motion::LineStart => {
            let line = buffer.line_of(head);
            let start = buffer.line_start(line);
            let text = buffer.line_text(line);
            let indent = start + (text.len() - text.trim_start().len());
            (if head == indent { start } else { indent }, None)
        }
        Motion::LineEnd => (buffer.line_end(buffer.line_of(head)), None),
        Motion::DocumentStart => (0, None),
        Motion::DocumentEnd => (buffer.len(), None),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/domain/movement.rs"]
mod tests;
