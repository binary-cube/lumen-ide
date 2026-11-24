//! How text is shown wherever it's shown (the code editor, a diff, a body, a log): one value
//! the app hands every view (`Viewer::set_text_options`, `ToolPanel::set_text_options`) when
//! it changes, so a choice made once (word wrap) holds in all of them. A new option is a
//! new field here, read by the views that draw what it's about; none of them names another.
//! And what they share to follow it: a line's rows when it's wrapped, and how many
//! columns a character takes.

use std::ops::Range;

/// How text is shown (see the module's doc). New options start as they've always been
/// (the default), so a view that doesn't read one is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextOptions {
    /// Lines longer than the view is wide go on in rows under them (off: they run on to the
    /// right and scroll).
    pub word_wrap: bool,
}

/// The fewest columns a wrapped row holds (a narrow view still shows words).
pub const LEAST_WRAP_COLUMNS: usize = 8;

/// The columns a character takes on screen: two for wide ones (CJK, Hangul, fullwidth
/// forms, most emoji), one for the others; tabs are the caller's (they go to the next stop).
pub fn char_columns(c: char) -> usize {
    let wide = matches!(c as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD);
    if wide { 2 } else { 1 }
}

/// Where a wrapped row may end after `c`: a space, or a mark code breaks at.
fn breaks_after(c: char) -> bool {
    matches!(c, ' ' | '\t' | ',' | ';' | '(' | '[' | '{' | '-' | '/' | '.' | ':' | '=' | '>' | '|')
}

/// `line`'s rows when it's wrapped at `columns` (tabs to stops of `tab`), as byte ranges
/// of the line, written into `out` (cleared first; one row when it fits). A row ends after
/// the last space or mark it can break at in its second half, else where it's full; a
/// character never splits. Nothing allocated past `out`'s room.
pub fn wrap_rows(line: &str, columns: usize, tab: u32, out: &mut Vec<Range<usize>>) {
    wrap_rows_hanging(line, columns, tab, 0, out);
}

/// [`wrap_rows`] with the rows after the first starting `hang` columns in (as the line's
/// indentation, the wrapped rows under its text): they hold that many fewer, never fewer
/// than [`LEAST_WRAP_COLUMNS`].
pub fn wrap_rows_hanging(
    line: &str,
    columns: usize,
    tab: u32,
    hang: usize,
    out: &mut Vec<Range<usize>>,
) {
    wrap_rows_with(line, (columns, tab, hang), &[], out);
}

/// [`wrap_rows_hanging`] (`(columns, tab, hang)`) with room shown in the line that isn't its
/// text (`extra`: before which byte, how many columns, sorted — an editor's inlay hints): it
/// counts as the character after it does, never split from it.
pub fn wrap_rows_with(
    line: &str,
    (columns, tab, hang): (usize, u32, usize),
    extra: &[(usize, usize)],
    out: &mut Vec<Range<usize>>,
) {
    out.clear();
    let columns = columns.max(LEAST_WRAP_COLUMNS);
    let later = columns.saturating_sub(hang).max(LEAST_WRAP_COLUMNS);
    let tab = tab.max(1) as usize;
    let (mut start, mut at, mut used) = (0, 0, 0);
    // The room of the row being filled: the first's, then the later ones'.
    let mut room = columns;
    // Where the row may end (a byte after a break mark), and its columns there.
    let mut cut: Option<(usize, usize)> = None;
    let mut next_extra = 0;
    for (i, c) in line.char_indices() {
        // The room shown before it (a hint), taken with it.
        let mut room_before = 0;
        while let Some(&(before, width)) = extra.get(next_extra) {
            if before > i {
                break;
            }
            room_before += width;
            next_extra += 1;
        }
        at += room_before;
        let width = room_before + if c == '\t' { tab - at % tab } else { char_columns(c) };
        if used + width > room && i > start {
            let (end, kept) = cut.filter(|(_, kept)| *kept > room / 2).unwrap_or((i, used));
            out.push(start..end);
            start = end;
            used -= kept;
            cut = None;
            room = later;
        }
        used += width;
        at += width - room_before;
        if breaks_after(c) {
            cut = Some((i + c.len_utf8(), used));
        }
    }
    out.push(start..line.len());
}

#[cfg(test)]
#[path = "../tests/unit/text_options.rs"]
mod tests;
