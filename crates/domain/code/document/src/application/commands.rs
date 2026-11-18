//! Editing commands. They all work on all cursors (multi-cursor).

use std::ops::Range;

use crate::domain::document::{Document, Indent};
use crate::domain::edit::{Edit, Transaction};
use crate::domain::history::EditKind;
use crate::domain::movement::{self, Motion};
use crate::domain::selection::{Selection, Selections};

/// Pairs auto-closed on Enter (a cursor between them produces an indented block).
const BRACKETS: &[(char, char)] = &[('{', '}'), ('(', ')'), ('[', ']')];

/// Inserts text at each cursor, replacing the selections.
pub fn insert_text(doc: &mut Document, text: &str) {
    if text.is_empty() {
        return;
    }
    let edits = doc.selections().iter().map(|s| Edit::new(s.range(), text)).collect();
    let kind = if text.chars().count() == 1 && text != "\n" {
        EditKind::Typing
    } else {
        EditKind::Command
    };
    doc.apply(Transaction::new(edits), None, kind);
}

/// Replaces `range` with `text` (the cursors follow), as part of the typing: one undo step
/// with it (a line re-indented as a closing tag is typed).
pub fn replace_range(doc: &mut Document, range: Range<usize>, text: &str) {
    doc.apply(Transaction::new(vec![Edit::new(range, text)]), None, EditKind::Typing);
}

/// A typed character, with auto-closed pairs:
/// - `(` `[` `{` and the `quotes` are auto-closed if the cursor is followed by whitespace,
///   end of line or a closing bracket (not in the middle of a word);
/// - a closing character typed right before an existing one just steps over it;
/// - over a selection, the pair wraps it;
/// - quotes are not doubled next to letters (`don't`).
pub fn type_char(doc: &mut Document, c: char, quotes: &[char]) {
    let buffer = doc.buffer();
    let close_of = |c: char| BRACKETS.iter().find(|(o, _)| *o == c).map(|(_, cl)| *cl);
    let is_closer = |c: char| BRACKETS.iter().any(|(_, cl)| *cl == c);
    let quote = quotes.contains(&c);
    let mut edits = Vec::new();
    let mut cursors = Vec::new();
    let mut delta: isize = 0;
    for sel in doc.selections().iter() {
        let range = sel.range();
        let before = buffer.char_before(range.start);
        let after = buffer.char_after(range.end);
        let shift = |offset: usize| (offset as isize + delta) as usize;
        let pair = close_of(c).or(quote.then_some(c));
        // Over a selection: wrap it.
        if !sel.is_empty()
            && let Some(close) = pair
        {
            let inner = buffer.slice(range.clone());
            let start = shift(range.start);
            cursors.push(Selection::new(start + c.len_utf8(), start + c.len_utf8() + inner.len()));
            delta += (c.len_utf8() + close.len_utf8()) as isize;
            edits.push(Edit::new(range, format!("{c}{inner}{close}")));
            continue;
        }
        // An existing closing character: just step over it.
        if sel.is_empty() && after == Some(c) && (is_closer(c) || quote) {
            cursors.push(Selection::cursor(shift(range.end) + c.len_utf8()));
            continue;
        }
        // A closer typed first on its line: the line goes back to its opener's indentation.
        if sel.is_empty()
            && let Some((open, _)) = BRACKETS.iter().find(|(_, cl)| *cl == c)
        {
            let line_start = buffer.line_start(buffer.line_of(range.start));
            let leading = buffer.slice(line_start..range.start);
            let opener = (!leading.is_empty() && leading.chars().all(|ch| ch == ' ' || ch == '\t'))
                .then(|| unmatched_opener(&buffer.slice(0..line_start), *open, c))
                .flatten();
            if let Some(at) = opener {
                let indent: String = buffer
                    .line_text(buffer.line_of(at))
                    .chars()
                    .take_while(|ch| *ch == ' ' || *ch == '\t')
                    .collect();
                if indent != leading {
                    let text = format!("{indent}{c}");
                    cursors.push(Selection::cursor(shift(line_start) + text.len()));
                    delta += text.len() as isize - leading.len() as isize;
                    edits.push(Edit::new(line_start..range.start, text));
                    continue;
                }
            }
        }
        let free_after =
            after.is_none_or(|a| a.is_whitespace() || is_closer(a) || matches!(a, ',' | ';' | ':'));
        let word = |ch: Option<char>| ch.is_some_and(|x| x.is_alphanumeric() || x == '_');
        let auto = match pair {
            Some(_) if quote => free_after && !word(before) && before != Some(c),
            Some(_) => free_after,
            None => false,
        };
        let text = match (auto, pair) {
            (true, Some(close)) => format!("{c}{close}"),
            _ => c.to_string(),
        };
        let start = shift(range.start);
        cursors.push(Selection::cursor(start + c.len_utf8()));
        delta += text.len() as isize - range.len() as isize;
        edits.push(Edit::new(range, text));
    }
    let primary = doc.selections().primary_index();
    let selections = Selections::new(cursors, primary);
    if edits.is_empty() {
        doc.set_selections(selections);
    } else {
        doc.apply(Transaction::new(edits), Some(selections), EditKind::Typing);
    }
}

/// Enter: new line with the current line's indentation; after `{` / `(` / `[` adds a level,
/// and if the closing bracket follows, moves it to the next line. If the opening
/// bracket has no pair yet (e.g. `function a() {` + Enter), the closing one is added on
/// the line after.
pub fn newline(doc: &mut Document) {
    newline_with(doc, &|_| None);
}

/// Enter, the syntax tree knowing better: `base_of(offset)` is the indentation of the line
/// where the innermost block still open at `offset` starts (`None`: unknown — the text's
/// rules above). The new line goes one level in from that base, or stays at it when it
/// starts with what closes the block (`}`, `]`, `)`, `</…>`); between a pair (`{|}`,
/// `<div>|</div>`) the closer gets its own line at the base.
pub fn newline_with(doc: &mut Document, base_of: &dyn Fn(usize) -> Option<String>) {
    let buffer = doc.buffer();
    let unit = doc.indent().unit();
    let mut edits = Vec::new();
    let mut cursors = Vec::new();
    let mut delta: isize = 0;
    for sel in doc.selections().iter() {
        let range = sel.range();
        let line = buffer.line_of(range.start);
        let text = buffer.line_text(line);
        let col = range.start - buffer.line_start(line);
        let indent: String = text[..col].chars().take_while(|c| *c == ' ' || *c == '\t').collect();

        let before = buffer.char_before(range.start);
        let after = buffer.char_after(range.end);
        let opens = before.and_then(|b| BRACKETS.iter().find(|(o, _)| *o == b));
        let mut insert = format!("\n{indent}");
        let mut cursor_offset = insert.len();
        let line_before = &text[..col.min(text.len())];
        let rest = &text[col.min(text.len())..];
        let next_line = buffer.line_text((line + 1).min(buffer.line_count().saturating_sub(1)));
        let doc_line = line_before.trim_start();
        if line_before.trim_end().ends_with("/**") && rest.trim().is_empty() {
            // `/**` + Enter: the doc block is closed, the cursor goes inside it.
            insert = format!("\n{indent} * ");
            cursor_offset = insert.len();
            if !next_line.trim_start().starts_with('*') || line + 1 >= buffer.line_count() {
                insert.push_str(&format!("\n{indent} */"));
            }
        } else if (doc_line.starts_with("* ") || doc_line == "*" || doc_line.starts_with("/*"))
            && !doc_line.contains("*/")
            && !line_before.trim_end().ends_with("/*")
        {
            // Inside a block comment: the next line continues with `* `.
            let star = if doc_line.starts_with("/*") { " * " } else { "* " };
            insert = format!("\n{indent}{star}");
            cursor_offset = insert.len();
        } else if let Some(base) = base_of(range.start) {
            let rest = rest.trim_start();
            let closes = rest.starts_with([')', ']', '}']) || rest.starts_with("</");
            // Between a pair: inside one level in, the closer on its own line at the base.
            let pair = opens.is_some_and(|(_, close)| after == Some(*close))
                || (before == Some('>') && rest.starts_with("</"));
            if pair {
                insert = format!("\n{base}{unit}");
                cursor_offset = insert.len();
                insert.push_str(&format!("\n{base}"));
            } else if closes {
                insert = format!("\n{base}");
                cursor_offset = insert.len();
            } else {
                insert = format!("\n{base}{unit}");
                cursor_offset = insert.len();
                // `function a() {` + Enter with no `}` yet: added below, at the base.
                if let Some((open, close)) = opens
                    && rest.is_empty()
                    && unbalanced(buffer.chunks().flat_map(str::chars), *open, *close)
                {
                    insert.push_str(&format!("\n{base}{close}"));
                }
            }
        } else if let Some((open, close)) = opens {
            insert.push_str(&unit);
            cursor_offset = insert.len();
            let rest_empty = text[col.min(text.len())..].trim().is_empty();
            if after == Some(*close) {
                insert.push('\n');
                insert.push_str(&indent);
            } else if rest_empty && unbalanced(buffer.chunks().flat_map(str::chars), *open, *close)
            {
                insert.push('\n');
                insert.push_str(&indent);
                insert.push(*close);
            }
        }
        let start = (range.start as isize + delta) as usize;
        cursors.push(Selection::cursor(start + cursor_offset));
        delta += insert.len() as isize - range.len() as isize;
        edits.push(Edit::new(range, insert));
    }
    let primary = doc.selections().primary_index();
    doc.apply(Transaction::new(edits), Some(Selections::new(cursors, primary)), EditKind::Command);
}

/// Where the last `open` bracket not closed in `text` is (ignoring strings and single-line
/// comments, approximately): what a `close` typed after it closes.
fn unmatched_opener(text: &str, open: char, close: char) -> Option<usize> {
    let mut stack = Vec::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q || c == '\n' {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '/' if chars.peek().is_some_and(|(_, n)| *n == '/') => {
                for (_, n) in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
            }
            c if c == open => stack.push(i),
            c if c == close => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.pop()
}

/// More `open` brackets than `close` in the text (ignoring strings and
/// single-line comments, approximately): auto-closing on Enter doesn't double a
/// bracket already closed further down. Read straight from the buffer (no copy of the file).
fn unbalanced(text: impl Iterator<Item = char>, open: char, close: char) -> bool {
    let mut depth: isize = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = text.peekable();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q || c == '\n' {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
            }
            c if c == open => depth += 1,
            c if c == close => depth -= 1,
            _ => {}
        }
    }
    depth > 0
}

/// Backspace: deletes the selection, otherwise the previous grapheme; in indentation made of
/// spaces, deletes back to the previous level. Inside an empty pair (`(|)`, `"|"`) deletes
/// both characters.
pub fn backspace(doc: &mut Document) {
    let buffer = doc.buffer();
    let empty_pair = |offset: usize| {
        let (before, after) = (buffer.char_before(offset), buffer.char_after(offset));
        matches!(
            (before, after),
            (Some('('), Some(')'))
                | (Some('['), Some(']'))
                | (Some('{'), Some('}'))
                | (Some('"'), Some('"'))
                | (Some('\''), Some('\''))
                | (Some('`'), Some('`'))
        )
    };
    let width = match doc.indent() {
        Indent::Spaces(n) => n as usize,
        Indent::Tabs => 1,
    };
    let edits = doc
        .selections()
        .iter()
        .map(|s| {
            if !s.is_empty() {
                return Edit::new(s.range(), "");
            }
            if empty_pair(s.head) {
                return Edit::new(s.head - 1..s.head + 1, "");
            }
            let line = buffer.line_of(s.head);
            let start = buffer.line_start(line);
            let before = buffer.slice(start..s.head);
            let col = before.len();
            if col > 0 && width > 1 && before.bytes().all(|b| b == b' ') {
                let remove = match col % width {
                    0 => width,
                    r => r,
                };
                return Edit::new(s.head - remove..s.head, "");
            }
            Edit::new(movement::prev_grapheme(buffer, s.head)..s.head, "")
        })
        .collect();
    doc.apply(Transaction::new(edits), None, EditKind::Deletion);
}

/// Delete: deletes the selection or the next grapheme.
pub fn delete_forward(doc: &mut Document) {
    let buffer = doc.buffer();
    let edits = doc
        .selections()
        .iter()
        .map(|s| {
            if s.is_empty() {
                Edit::new(s.head..movement::next_grapheme(buffer, s.head), "")
            } else {
                Edit::new(s.range(), "")
            }
        })
        .collect();
    doc.apply(Transaction::new(edits), None, EditKind::Deletion);
}

/// Deletes back to the start of the word (Alt/Ctrl+Backspace).
pub fn delete_word_left(doc: &mut Document) {
    let buffer = doc.buffer();
    let edits = doc
        .selections()
        .iter()
        .map(|s| {
            if s.is_empty() {
                Edit::new(movement::word_left(buffer, s.head)..s.head, "")
            } else {
                Edit::new(s.range(), "")
            }
        })
        .collect();
    doc.apply(Transaction::new(edits), None, EditKind::Command);
}

/// Deletes the lines touched by the cursors (Cmd/Ctrl+Backspace).
pub fn delete_lines(doc: &mut Document) {
    let buffer = doc.buffer();
    let edits = line_ranges(doc)
        .into_iter()
        .map(|(first, last)| {
            let start = buffer.line_start(first);
            let end = if last + 1 < buffer.line_count() {
                buffer.line_start(last + 1)
            } else if first > 0 {
                // Last line: also delete the `\n` before it.
                return Edit::new(buffer.line_end(first - 1)..buffer.line_end(last), "");
            } else {
                buffer.line_end(last)
            };
            Edit::new(start..end, "")
        })
        .collect();
    doc.apply(Transaction::new(edits), None, EditKind::Command);
}

/// Duplicates the selection, or the current line if nothing is selected (Cmd/Ctrl+D).
pub fn duplicate(doc: &mut Document) {
    let buffer = doc.buffer();
    let mut edits = Vec::new();
    let mut cursors = Vec::new();
    let mut delta: isize = 0;
    for s in doc.selections().iter() {
        if s.is_empty() {
            // A collapsed region's line: the region with it.
            let line = buffer.line_of(s.head);
            let (first, last) = doc.grow_lines(line, line);
            let (start, end) = (buffer.line_start(first), buffer.line_end(last));
            let text = format!("\n{}", buffer.slice(start..end));
            let head = (s.head as isize + delta) as usize + text.len();
            cursors.push(Selection::cursor(head));
            delta += text.len() as isize;
            edits.push(Edit::new(end..end, text));
        } else {
            let r = s.range();
            let text = buffer.slice(r.clone());
            let start = (r.end as isize + delta) as usize;
            cursors.push(Selection::new(start, start + text.len()));
            delta += text.len() as isize;
            edits.push(Edit::new(r.end..r.end, text));
        }
    }
    let primary = doc.selections().primary_index();
    doc.apply(Transaction::new(edits), Some(Selections::new(cursors, primary)), EditKind::Command);
}

/// Tab: with a multi-line selection indents the lines; otherwise inserts indentation
/// up to the next level.
pub fn indent(doc: &mut Document) {
    let multiline = doc.selections().iter().any(|s| {
        let r = s.range();
        doc.buffer().line_of(r.start) != doc.buffer().line_of(r.end)
    });
    if multiline {
        return shift_lines(doc, true);
    }
    let buffer = doc.buffer();
    let tab = doc.tab_width();
    let edits = doc
        .selections()
        .iter()
        .map(|s| {
            let text = match doc.indent() {
                Indent::Tabs => "\t".to_owned(),
                Indent::Spaces(n) => {
                    let col = movement::visual_column(buffer, s.range().start, tab);
                    " ".repeat((u32::from(n) - col % u32::from(n)) as usize)
                }
            };
            Edit::new(s.range(), text)
        })
        .collect();
    doc.apply(Transaction::new(edits), None, EditKind::Command);
}

/// Shift+Tab: removes one indentation level from the touched lines.
pub fn outdent(doc: &mut Document) {
    shift_lines(doc, false);
}

fn shift_lines(doc: &mut Document, right: bool) {
    let buffer = doc.buffer();
    let unit = doc.indent().unit();
    let mut edits = Vec::new();
    for (first, last) in line_ranges(doc) {
        for line in first..=last {
            let start = buffer.line_start(line);
            let text = buffer.line_text(line);
            if right {
                if !text.trim().is_empty() {
                    edits.push(Edit::new(start..start, unit.clone()));
                }
            } else {
                let remove = if text.starts_with('\t') {
                    1
                } else {
                    text.bytes().take(unit.len().max(1)).take_while(|b| *b == b' ').count()
                };
                if remove > 0 {
                    edits.push(Edit::new(start..start + remove, ""));
                }
            }
        }
    }
    // Selections are mapped through the transaction, so they stay on the same lines.
    doc.apply(Transaction::new(edits), None, EditKind::Command);
}

/// Moves all cursors. With `extend`, selections are extended instead of collapsed.
pub fn move_cursors(doc: &mut Document, motion: Motion, extend: bool) {
    let buffer = doc.buffer();
    let tab = doc.tab_width();
    let selections = doc.selections().transform(|s| {
        // Left/right without Shift on a selection: the cursor jumps to its end.
        if !extend && !s.is_empty() {
            match motion {
                Motion::Left => return Selection::cursor(s.range().start),
                Motion::Right => return Selection::cursor(s.range().end),
                _ => {}
            }
        }
        let (head, goal) = movement::move_head(buffer, s.head, s.goal, motion, tab);
        let anchor = if extend { s.anchor } else { head };
        Selection { anchor, head, goal }
    });
    doc.set_selections(selections);
}

/// Click in text: moves the cursor, extends the selection (`extend`) or adds a cursor (`add`).
pub fn click(doc: &mut Document, offset: usize, extend: bool, add: bool) {
    let selections = if add {
        doc.selections().add(Selection::cursor(offset))
    } else if extend {
        let p = doc.selections().primary();
        Selections::single(Selection::new(p.anchor, offset))
    } else {
        Selections::single(Selection::cursor(offset))
    };
    doc.set_selections(selections);
}

/// Moves the end of the primary selection (mouse dragging).
pub fn drag_to(doc: &mut Document, offset: usize) {
    let primary = doc.selections().primary();
    doc.set_selections(Selections::single(Selection::new(primary.anchor, offset)));
}

/// Dragging after a double / triple click: the selection covers the start unit (`origin`,
/// the word or line grabbed) and the unit under the cursor (`target`), without cutting them.
pub fn drag_extend(doc: &mut Document, origin: Range<usize>, target: Range<usize>) {
    let selection = if target.start < origin.start {
        Selection::new(origin.end, target.start)
    } else {
        Selection::new(origin.start, target.end.max(origin.end))
    };
    doc.set_selections(Selections::single(selection));
}

/// The word (or group of whitespace / punctuation) at `offset`. `word_chars` are
/// the characters that, besides letters, digits and `_`, are part of a word in the
/// current language (e.g. `$` in PHP, `-` in CSS).
pub fn word_range(doc: &Document, offset: usize, word_chars: &str) -> Range<usize> {
    let buffer = doc.buffer();
    let line = buffer.line_of(offset);
    let start = buffer.line_start(line);
    let text = buffer.line_text(line);
    let col = (offset - start).min(text.len());
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' || word_chars.contains(c) {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    // A click right after a word (e.g. at its end) grabs that word.
    let after = text[col..].chars().next();
    let before = text[..col].chars().last();
    let target = match (before, after) {
        (Some(b), Some(a)) if class(a) != 0 && class(b) == 0 => b,
        (_, Some(a)) => a,
        (Some(b), None) => b,
        (None, None) => return offset..offset,
    };
    let cls = class(target);
    let from = text[..col]
        .char_indices()
        .rev()
        .take_while(|(_, c)| class(*c) == cls)
        .last()
        .map_or(col, |(i, _)| i);
    let to = text[col..]
        .char_indices()
        .take_while(|(_, c)| class(*c) == cls)
        .last()
        .map_or(col, |(i, c)| col + i + c.len_utf8());
    start + from..start + to
}

/// The line at `offset`, including `\n`.
pub fn line_range(doc: &Document, offset: usize) -> Range<usize> {
    let buffer = doc.buffer();
    let line = buffer.line_of(offset);
    let end = if line + 1 < buffer.line_count() {
        buffer.line_start(line + 1)
    } else {
        buffer.line_end(line)
    };
    buffer.line_start(line)..end
}

/// Double-click: selects the word at `offset` (see [`word_range`]).
pub fn select_word_at(doc: &mut Document, offset: usize, word_chars: &str) {
    let range = word_range(doc, offset, word_chars);
    doc.set_selections(Selections::single(Selection::new(range.start, range.end)));
}

/// Triple-click: selects the whole line, including `\n`.
pub fn select_line_at(doc: &mut Document, offset: usize) {
    let range = line_range(doc, offset);
    doc.set_selections(Selections::single(Selection::new(range.start, range.end)));
}

/// Moves the cursors vertically to the line given by `target` (e.g. skipping collapsed
/// regions), keeping the desired column. `None` = edge: the start / end of the document.
pub fn move_vertical(
    doc: &mut Document,
    down: bool,
    extend: bool,
    target: &dyn Fn(usize) -> Option<usize>,
) {
    let buffer = doc.buffer();
    let tab = doc.tab_width();
    let selections = doc.selections().transform(|s| {
        let goal = s.goal.unwrap_or_else(|| movement::visual_column(buffer, s.head, tab));
        let head = match target(buffer.line_of(s.head)) {
            Some(line) => movement::offset_at_column(buffer, line, goal, tab),
            None if down => buffer.len(),
            None => 0,
        };
        Selection { anchor: if extend { s.anchor } else { head }, head, goal: Some(goal) }
    });
    doc.set_selections(selections);
}

pub fn select_all(doc: &mut Document) {
    let len = doc.buffer().len();
    doc.set_selections(Selections::single(Selection::new(0, len)));
}

/// Keeps only the primary cursor (Escape).
pub fn collapse_to_primary(doc: &mut Document) -> bool {
    let s = doc.selections();
    if s.len() == 1 && s.primary().is_empty() {
        return false;
    }
    let head = s.primary().head;
    let next =
        if s.len() > 1 { s.only_primary() } else { Selections::single(Selection::cursor(head)) };
    doc.set_selections(next);
    true
}

/// Adds a cursor on the line above / below the primary cursor.
pub fn add_cursor_vertical(doc: &mut Document, down: bool) {
    let buffer = doc.buffer();
    let tab = doc.tab_width();
    let primary = doc.selections().primary();
    let motion = if down { Motion::Down } else { Motion::Up };
    let (head, goal) = movement::move_head(buffer, primary.head, primary.goal, motion, tab);
    if buffer.line_of(head) == buffer.line_of(primary.head) {
        return;
    }
    let next = doc.selections().add(Selection { anchor: head, head, goal });
    doc.set_selections(next);
}

/// Selects the next occurrence of the selected text (or of the word under the cursor).
pub fn select_next_occurrence(doc: &mut Document) {
    let buffer = doc.buffer();
    let primary = doc.selections().primary();
    if primary.is_empty() {
        let start = movement::word_left(
            buffer,
            movement::next_grapheme(buffer, primary.head).min(primary.head + 1),
        );
        let end = movement::word_right(buffer, start);
        if start < end {
            doc.set_selections(doc.selections().transform(|s| {
                if s.head == primary.head { Selection::new(start, end) } else { *s }
            }));
        }
        return;
    }
    let needle = buffer.slice(primary.range());
    let after = primary.range().end;
    // Searched over the text's chunks (a press doesn't copy a large file).
    let found = buffer
        .find(&needle, after)
        .or_else(|| buffer.find(&needle, 0))
        .filter(|&i| !doc.selections().iter().any(|s| s.range().start == i));
    if let Some(start) = found {
        let next = doc.selections().add(Selection::new(start, start + needle.len()));
        doc.set_selections(next);
    }
}

/// Selects all occurrences of the selected text (or of the word under the cursor), with
/// one cursor on each; the current one stays primary.
pub fn select_all_occurrences(doc: &mut Document) {
    if doc.selections().primary().is_empty() {
        select_next_occurrence(doc);
    }
    let primary = doc.selections().primary();
    if primary.is_empty() {
        return;
    }
    let buffer = doc.buffer();
    let needle = buffer.slice(primary.range());
    let items: Vec<Selection> =
        buffer.find_all(&needle).into_iter().map(|i| Selection::new(i, i + needle.len())).collect();
    if items.len() < 2 {
        return;
    }
    let current = items.iter().position(|s| s.range() == primary.range()).unwrap_or(0);
    doc.set_selections(Selections::new(items, current));
}

/// Drops the last added occurrence (the primary selection), if there are several.
pub fn unselect_occurrence(doc: &mut Document) {
    let selections = doc.selections();
    if selections.len() < 2 {
        return;
    }
    let primary = selections.primary_index();
    let items: Vec<Selection> =
        selections.iter().enumerate().filter(|(i, _)| *i != primary).map(|(_, s)| *s).collect();
    let next = primary.saturating_sub(1).min(items.len() - 1);
    doc.set_selections(Selections::new(items, next));
}

/// ⌥ + click: adds a cursor at `offset`, or removes it if one is already there (and it's not
/// the only one).
pub fn toggle_cursor(doc: &mut Document, offset: usize) {
    let selections = doc.selections();
    let existing = selections.iter().position(|s| s.is_empty() && s.head == offset);
    match existing {
        Some(i) if selections.len() > 1 => {
            let items: Vec<Selection> =
                selections.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, s)| *s).collect();
            let primary = selections.primary_index().min(items.len() - 1);
            doc.set_selections(Selections::new(items, primary));
        }
        Some(_) => {}
        None => {
            let next = selections.add(Selection::cursor(offset));
            doc.set_selections(next);
        }
    }
}

/// Column selection (⌥ + drag): on each line between `anchor` and `head` (line,
/// column in characters), from one column to the other; shorter lines get only
/// the cursor at the end.
pub fn column_select(doc: &mut Document, anchor: (usize, usize), head: (usize, usize)) {
    let buffer = doc.buffer();
    let last = buffer.line_count().saturating_sub(1);
    let (l0, l1) = (anchor.0.min(head.0).min(last), anchor.0.max(head.0).min(last));
    let offset_of = |line: usize, col: usize| {
        let text = buffer.line_text(line);
        let byte = text.char_indices().nth(col).map_or(text.len(), |(b, _)| b);
        buffer.line_start(line) + byte
    };
    let mut items = Vec::new();
    for line in l0..=l1 {
        items.push(Selection::new(offset_of(line, anchor.1), offset_of(line, head.1)));
    }
    let primary = if head.0 >= anchor.0 { items.len() - 1 } else { 0 };
    doc.set_selections(Selections::new(items, primary));
}

/// The text to copy: the selections joined with `\n`; without a selection, the whole lines
/// (with `\n`).
pub fn copy(doc: &Document) -> String {
    let buffer = doc.buffer();
    if doc.selections().iter().all(Selection::is_empty) {
        return line_ranges(doc)
            .into_iter()
            .map(|(first, last)| {
                format!("{}\n", buffer.slice(buffer.line_start(first)..buffer.line_end(last)))
            })
            .collect();
    }
    doc.selections()
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| buffer.slice(s.range()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn cut(doc: &mut Document) -> String {
    let text = copy(doc);
    if doc.selections().iter().all(Selection::is_empty) {
        delete_lines(doc);
    } else {
        let edits = doc.selections().iter().map(|s| Edit::new(s.range(), "")).collect();
        doc.apply(Transaction::new(edits), None, EditKind::Command);
    }
    text
}

/// Paste: if the text has one line per cursor, each cursor gets its own line.
pub fn paste(doc: &mut Document, text: &str) {
    let text = text.replace("\r\n", "\n");
    let count = doc.selections().len();
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let edits = if count > 1 && lines.len() == count {
        doc.selections().iter().zip(lines).map(|(s, l)| Edit::new(s.range(), l)).collect()
    } else {
        doc.selections().iter().map(|s| Edit::new(s.range(), text.as_str())).collect()
    };
    doc.apply(Transaction::new(edits), None, EditKind::Command);
}

/// Comments / uncomments the lines touched by the cursors with a line comment
/// (e.g. `//`, `#`). If all non-empty lines are already commented, the comment is removed.
pub fn toggle_line_comment(doc: &mut Document, token: &str) {
    let buffer = doc.buffer();
    let mut edits = Vec::new();
    for (first, last) in line_ranges(doc) {
        let lines: Vec<(usize, String)> = (first..=last)
            .map(|l| (l, buffer.line_text(l)))
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let commented = lines.iter().all(|(_, t)| t.trim_start().starts_with(token));
        // The comment goes at the minimum indentation, so the block stays aligned.
        let indent = lines.iter().map(|(_, t)| t.len() - t.trim_start().len()).min().unwrap_or(0);
        for (line, text) in lines {
            let start = buffer.line_start(line);
            if commented {
                let at = text.len() - text.trim_start().len();
                let after = &text[at + token.len()..];
                let len = token.len() + usize::from(after.starts_with(' '));
                edits.push(Edit::new(start + at..start + at + len, ""));
            } else {
                edits.push(Edit::new(start + indent..start + indent, format!("{token} ")));
            }
        }
    }
    doc.apply(Transaction::new(edits), None, EditKind::Command);
}

/// Comments / uncomments with a block comment (e.g. `<!--` `-->`, `/*` `*/`):
/// the selection, or the current line's content if nothing is selected.
pub fn toggle_block_comment(doc: &mut Document, open: &str, close: &str) {
    let buffer = doc.buffer();
    let mut edits = Vec::new();
    for s in doc.selections().iter() {
        let range = if s.is_empty() {
            // A collapsed region's line: from its first line's content to its last's.
            let line = buffer.line_of(s.head);
            let (first, last) = doc.grow_lines(line, line);
            let (text, end_text) = (buffer.line_text(first), buffer.line_text(last));
            let start = buffer.line_start(first) + (text.len() - text.trim_start().len());
            start..buffer.line_start(last) + end_text.trim_end().len()
        } else {
            s.range()
        };
        let text = buffer.slice(range.clone());
        let trimmed = text.trim();
        if trimmed.starts_with(open)
            && trimmed.ends_with(close)
            && trimmed.len() >= open.len() + close.len()
        {
            let inner = &trimmed[open.len()..trimmed.len() - close.len()];
            let lead = text.len() - text.trim_start().len();
            let inner = inner.strip_prefix(' ').unwrap_or(inner);
            let inner = inner.strip_suffix(' ').unwrap_or(inner);
            edits.push(Edit::new(range.start + lead..range.start + lead + trimmed.len(), inner));
        } else if !range.is_empty() {
            edits.push(Edit::new(range.start..range.start, format!("{open} ")));
            edits.push(Edit::new(range.end..range.end, format!(" {close}")));
        }
    }
    doc.apply(Transaction::new(edits), None, EditKind::Command);
}

/// Cleanup before saving: trailing whitespace (except on lines with a
/// cursor, so freshly typed indentation doesn't vanish) and the final newline. A single undo
/// step; nothing if the document is already clean, or locked (saved as it is).
pub fn cleanup_for_save(doc: &mut Document) {
    if doc.is_read_only() {
        return;
    }
    let cleanup = doc.save_cleanup();
    let buffer = doc.buffer();
    let mut edits = Vec::new();
    if cleanup.trim_trailing_whitespace {
        let cursor_lines: Vec<usize> =
            doc.selections().iter().map(|s| buffer.line_of(s.head)).collect();
        let mut line = String::new();
        for i in 0..buffer.line_count() {
            if cursor_lines.contains(&i) {
                continue;
            }
            buffer.write_line(i, &mut line);
            let trimmed = line.trim_end_matches([' ', '\t']).len();
            if trimmed < line.len() {
                let start = buffer.line_start(i);
                edits.push(Edit::new(start + trimmed..start + line.len(), ""));
            }
        }
    }
    let end = buffer.len();
    let final_newline = cleanup.insert_final_newline == Some(true)
        && !buffer.is_empty()
        && buffer.char_before(end) != Some('\n');
    if final_newline {
        edits.push(Edit::new(end..end, "\n"));
    }
    if !edits.is_empty() {
        doc.apply(Transaction::new(edits), None, EditKind::Command);
    }
    // A cursor at the end stays before the added newline (saving, e.g. automatically,
    // while typing on the last line doesn't move it).
    if final_newline {
        let new_end = doc.buffer().len();
        let selections = doc.selections().transform(|s| {
            let back = |o: usize| if o == new_end { o - 1 } else { o };
            Selection { anchor: back(s.anchor), head: back(s.head), goal: s.goal }
        });
        doc.set_selections(selections);
    }
}

/// The line ranges (first, last) touched by the cursors, without duplicates.
/// The lines of the primary selection (0-based, end exclusive); a selection ending at a
/// line's start doesn't take that line.
pub fn selected_lines(doc: &Document) -> Range<usize> {
    let buffer = doc.buffer();
    let r = doc.selections().primary().range();
    let first = buffer.line_of(r.start);
    let mut last = buffer.line_of(r.end);
    if last > first && r.end == buffer.line_start(last) {
        last -= 1;
    }
    let (first, last) = doc.grow_lines(first, last);
    first..last + 1
}

/// Lines `unit` and the lines `past` next to it (above or below) trade places — "Move
/// Line", "Move Statement": the lines between them stay between, the selections in `unit`
/// move with it. One undo step. `false` if the ranges don't make sense.
pub fn swap_lines(doc: &mut Document, unit: Range<usize>, past: Range<usize>) -> bool {
    let buffer = doc.buffer();
    let count = buffer.line_count();
    let overlap = unit.start < past.end && past.start < unit.end;
    if unit.is_empty() || past.is_empty() || overlap || unit.end > count || past.end > count {
        return false;
    }
    let up = past.start < unit.start;
    let (first, second) = if up { (&past, &unit) } else { (&unit, &past) };
    let lines =
        |r: &Range<usize>| buffer.slice(buffer.line_start(r.start)..buffer.line_end(r.end - 1));
    let gap = first.end..second.start;
    let mut text = lines(second);
    text.push('\n');
    if !gap.is_empty() {
        text.push_str(&lines(&gap));
        text.push('\n');
    }
    let moved_from = buffer.line_start(unit.start);
    let moved_end = buffer.line_end(unit.end - 1);
    let span = buffer.line_start(first.start)..buffer.line_end(second.end - 1);
    // Where the unit starts after: first when it goes up, after the rest when it goes down.
    let moved_to = if up { span.start } else { span.start + text.len() };
    text.push_str(&lines(first));
    let shift = |at: usize| {
        if (moved_from..=moved_end).contains(&at) { at - moved_from + moved_to } else { at }
    };
    let selections: Vec<Selection> =
        doc.selections().iter().map(|s| Selection::new(shift(s.anchor), shift(s.head))).collect();
    let primary = doc.selections().primary_index();
    doc.apply(
        Transaction::single(span, text),
        Some(Selections::new(selections, primary)),
        EditKind::Command,
    );
    true
}

/// "Move Line": the selected lines past the line above / below. `false` at the start /
/// the end of the text.
pub fn move_lines(doc: &mut Document, up: bool) -> bool {
    let unit = selected_lines(doc);
    let past = if up {
        let Some(above) = unit.start.checked_sub(1) else { return false };
        above..unit.start
    } else {
        if unit.end >= doc.buffer().line_count() {
            return false;
        }
        unit.end..unit.end + 1
    };
    swap_lines(doc, unit, past)
}

fn line_ranges(doc: &Document) -> Vec<(usize, usize)> {
    let buffer = doc.buffer();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for s in doc.selections().iter() {
        let r = s.range();
        let first = buffer.line_of(r.start);
        let mut last = buffer.line_of(r.end);
        // A selection ending at the start of a line doesn't include it.
        if last > first && r.end == buffer.line_start(last) {
            last -= 1;
        }
        // A collapsed region's lines go together.
        let (first, last) = doc.grow_lines(first, last);
        match ranges.last_mut() {
            Some(prev) if first <= prev.1 + 1 => prev.1 = prev.1.max(last),
            _ => ranges.push((first, last)),
        }
    }
    ranges
}

#[cfg(test)]
#[path = "../../tests/unit/application/commands.rs"]
mod tests;
