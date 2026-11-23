//! Tabs as shown: a line expanded to its display columns, and offsets between the source and the
//! display.

/// A line's inlay hints as the display makes room for them: (the byte of the line they're
/// before, how many columns they take), sorted.
pub(super) type LineInlays = [(usize, usize)];

/// The columns of the inlays before byte `i` (at `*next` on): added, `*next` moved past them.
fn inlays_at(inlays: &LineInlays, i: usize, next: &mut usize) -> usize {
    let mut columns = 0;
    while let Some(&(at, width)) = inlays.get(*next) {
        if at > i {
            break;
        }
        columns += width;
        *next += 1;
    }
    columns
}

/// The displayed text for a line: tabs expanded, at most `max_columns` characters.
pub(super) fn expand_tabs(line: &str, max_columns: usize, tab: u32, out: &mut String) {
    expand_tabs_with(line, max_columns, tab, &[], out);
}

/// [`expand_tabs`] with room (spaces) for the line's inlay hints.
pub(super) fn expand_tabs_with(
    line: &str,
    max_columns: usize,
    tab: u32,
    inlays: &LineInlays,
    out: &mut String,
) {
    out.clear();
    let tab = tab as usize;
    let mut columns = 0;
    let mut next = 0;
    for (i, c) in line.char_indices() {
        if columns >= max_columns {
            break;
        }
        let room = inlays_at(inlays, i, &mut next);
        out.extend(std::iter::repeat_n(' ', room));
        columns += room;
        if c == '\t' {
            let spaces = tab - columns % tab;
            out.extend(std::iter::repeat_n(' ', spaces));
            columns += spaces;
        } else {
            out.push(c);
            columns += 1;
        }
    }
    if columns < max_columns {
        let room = inlays_at(inlays, line.len(), &mut next);
        out.extend(std::iter::repeat_n(' ', room));
    }
}

/// For each byte in the displayed text, the corresponding byte in the source line.
#[cfg(test)]
pub(super) fn build_display_map(line: &str, tab: u32, out: &mut Vec<u32>) {
    build_display_map_with(line, tab, &[], out);
}

/// [`build_display_map`] with the inlays' room (each of its bytes: the byte it's before).
pub(super) fn build_display_map_with(
    line: &str,
    tab: u32,
    inlays: &LineInlays,
    out: &mut Vec<u32>,
) {
    out.clear();
    let tab = tab as usize;
    let mut columns = 0;
    let mut next = 0;
    for (i, c) in line.char_indices() {
        let room = inlays_at(inlays, i, &mut next);
        out.extend(std::iter::repeat_n(i as u32, room));
        columns += room;
        let width = if c == '\t' { tab - columns % tab } else { c.len_utf8() };
        out.extend(std::iter::repeat_n(i as u32, width));
        columns += if c == '\t' { width } else { 1 };
    }
    let room = inlays_at(inlays, line.len(), &mut next);
    out.extend(std::iter::repeat_n(line.len() as u32, room));
}

/// The offset in the displayed text (tabs expanded) for offset `src` in the source line.
pub(super) fn source_to_display(line: &str, src: usize, tab: u32) -> usize {
    source_to_display_with(line, src, tab, &[])
}

/// [`source_to_display`] with the inlays: a byte an inlay is before is shown after it (the
/// caret at an argument's start sits right before the argument, its hint before it).
pub(super) fn source_to_display_with(
    line: &str,
    src: usize,
    tab: u32,
    inlays: &LineInlays,
) -> usize {
    let tab = tab as usize;
    let mut disp = 0;
    let mut columns = 0;
    let mut next = 0;
    for (i, c) in line.char_indices() {
        if i > src {
            break;
        }
        let room = inlays_at(inlays, i, &mut next);
        disp += room;
        columns += room;
        if i == src {
            return disp;
        }
        if c == '\t' {
            let spaces = tab - columns % tab;
            disp += spaces;
            columns += spaces;
        } else {
            disp += c.len_utf8();
            columns += 1;
        }
    }
    if src >= line.len() {
        disp += inlays_at(inlays, line.len(), &mut next);
    }
    disp
}

/// The inverse of [`source_to_display`]; an offset in the middle of a tab goes to the
/// nearest edge.
#[cfg(test)]
pub(super) fn display_to_source(line: &str, disp: usize, tab: u32) -> usize {
    display_to_source_with(line, disp, tab, &[])
}

/// [`display_to_source`] with the inlays: a point on an inlay is the byte it's before.
pub(super) fn display_to_source_with(
    line: &str,
    disp: usize,
    tab: u32,
    inlays: &LineInlays,
) -> usize {
    let tab = tab as usize;
    let mut d = 0;
    let mut columns = 0;
    let mut next = 0;
    for (i, c) in line.char_indices() {
        let room = inlays_at(inlays, i, &mut next);
        if d + room > disp {
            return i;
        }
        d += room;
        columns += room;
        let width = if c == '\t' { tab - columns % tab } else { c.len_utf8() };
        if d + width > disp {
            return if c == '\t' && disp - d > width / 2 { i + 1 } else { i };
        }
        d += width;
        columns += if c == '\t' { width } else { 1 };
    }
    line.len()
}

/// A line's indentation in display columns (tabs to the next stop); `None` for a blank
/// line.
pub(super) fn indent_width(line: &str, tab: u32) -> Option<usize> {
    let tab = tab.max(1) as usize;
    let mut width = 0;
    for c in line.chars() {
        match c {
            ' ' => width += 1,
            '\t' => width += tab - width % tab,
            '\n' | '\r' => return None,
            _ => return Some(width),
        }
    }
    None
}

/// The columns a character takes (the components' rule, the same everywhere).
pub(super) use ui_components::text_options::char_columns;

/// The widest line's display columns (tabs expanded to their stops, wide characters as two),
/// over the text's `chunks`, the lines in `hidden` (sorted `[first, last]` ranges: collapsed
/// regions) left out. One pass over the bytes, nothing allocated.
pub(super) fn longest_line_columns<'a>(
    chunks: impl Iterator<Item = &'a str>,
    tab: u32,
    hidden: &[(u32, u32)],
) -> usize {
    widest_line(chunks, tab, hidden).0
}

/// [`longest_line_columns`] and the (first) line that wide.
/// The widest line's display columns (tabs expanded to their stops, wide characters as two),
/// over the text's `chunks`, the lines in `hidden` (sorted `[first, last]` ranges: collapsed
/// regions) left out. One pass over the bytes, nothing allocated.
pub(super) fn widest_line<'a>(
    chunks: impl Iterator<Item = &'a str>,
    tab: u32,
    hidden: &[(u32, u32)],
) -> (usize, u32) {
    let tab = tab.max(1) as usize;
    let (mut widest, mut columns, mut line) = (0usize, 0usize, 0u32);
    let mut at = 0u32;
    let mut next_hidden = 0;
    let shown = |line: u32, next: &mut usize| {
        while hidden.get(*next).is_some_and(|&(_, end)| end < line) {
            *next += 1;
        }
        !hidden.get(*next).is_some_and(|&(start, _)| start <= line)
    };
    for chunk in chunks {
        // Per byte (most code is ASCII); a character beyond ASCII is read at its first byte,
        // its other bytes skipped.
        let bytes = chunk.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'\n' => {
                    if columns > widest && shown(line, &mut next_hidden) {
                        (widest, at) = (columns, line);
                    }
                    columns = 0;
                    line += 1;
                }
                b'\t' => columns += tab - columns % tab,
                b'\r' => {}
                0..0x80 => columns += 1,
                // A continuation byte.
                0x80..0xC0 => {}
                _ => columns += chunk[i..].chars().next().map_or(1, char_columns),
            }
        }
    }
    if columns > widest && shown(line, &mut next_hidden) {
        (widest, at) = (columns, line);
    }
    (widest, at)
}

/// The widest line after `changes` (in order, each in the text the ones before made), from
/// the widest before (`columns` wide, on `line`) and the changed lines measured again:
/// `None` when that line was changed and nothing as wide is left among them (the whole text
/// is counted again then), or when too much changed.
pub(super) fn widest_after(
    buffer: &document::TextBuffer,
    changes: &[document::TextChange],
    (columns, line): (usize, u32),
    tab: u32,
    hidden: &[(u32, u32)],
) -> Option<(usize, u32)> {
    if changes.len() > 256 {
        return None;
    }
    let mut widest = Some(line as i64);
    let mut dirty: Vec<(i64, i64)> = Vec::with_capacity(changes.len());
    for c in changes {
        let (start, old_end, new_end) = (
            i64::from(c.start_point.line.0),
            i64::from(c.old_end_point.line.0),
            i64::from(c.new_end_point.line.0),
        );
        let map = |l: i64, inside: i64| {
            if l < start {
                l
            } else if l > old_end {
                l + new_end - old_end
            } else {
                inside
            }
        };
        widest = widest.filter(|&l| l < start || l > old_end).map(|l| map(l, l));
        for d in &mut dirty {
            *d = (map(d.0, start), map(d.1, new_end).max(map(d.0, start)));
        }
        dirty.push((start, new_end));
    }
    let lines = buffer.line_count() as i64;
    let touched: i64 = dirty.iter().map(|(a, b)| b - a + 1).sum();
    if touched > lines / 2 + 1 {
        return None;
    }
    let is_hidden = |l: u32| {
        let i = hidden.partition_point(|(_, end)| *end < l);
        hidden.get(i).is_some_and(|(first, _)| *first <= l)
    };
    let mut best: Option<(usize, u32)> = None;
    let mut text = String::new();
    for (a, b) in dirty {
        for l in a.max(0)..=b.min(lines - 1) {
            let l = l as u32;
            if is_hidden(l) {
                continue;
            }
            buffer.write_line(l as usize, &mut text);
            let width = longest_line_columns([text.as_str()].into_iter(), tab, &[]);
            if best.is_none_or(|(w, _)| width > w) {
                best = Some((width, l));
            }
        }
    }
    match (widest, best) {
        (Some(_), Some(b)) if b.0 > columns => Some(b),
        (Some(l), _) => Some((columns, l as u32)),
        (None, Some(b)) if b.0 >= columns => Some(b),
        (None, _) => None,
    }
}
