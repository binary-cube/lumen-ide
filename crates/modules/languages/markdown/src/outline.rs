//! A Markdown file as the Structure panel and the breadcrumb see it: its headings
//! ([`SymbolKind::Section`]), each reaching to the next heading of its level or a higher one
//! (the outline's nesting and the breadcrumb's chain come from those ranges: a `##` inside
//! the `#` before it). ATX headings (`## Title`) and setext ones (a line underlined with `===`
//! or `---`); none inside fenced code blocks, the front matter or an indented code block.
//! Pure, one pass over the lines.

use code_intelligence::{FileSymbols, Symbol, SymbolKind, Visibility};

/// A heading's text is cut at this many characters (a heading is a line, not a page).
const MAX_NAME: usize = 200;

/// A heading found: its level (1–6), its text, where its line starts, where its text starts,
/// its line number (0-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Heading {
    pub level: u8,
    pub text: String,
    pub start: usize,
    pub text_at: usize,
    pub line: u32,
}

/// The file's headings as sections, each to the end of the line before the next heading of
/// its level or higher (or the end of the file).
pub fn symbols(text: &str) -> FileSymbols {
    let found = headings(text);
    let symbols = found
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let next =
                found[i + 1..].iter().find(|n| n.level <= h.level).map_or(text.len(), |n| n.start);
            // The newline before the next heading isn't the section's: a cursor there is
            // still in the next one.
            let end = if next < text.len() { next.saturating_sub(1).max(h.start) } else { next };
            Symbol {
                fqn: h.text.clone().into_boxed_str(),
                name: h.text.clone().into_boxed_str(),
                kind: SymbolKind::Section,
                container: None,
                parents: Box::default(),
                range: h.start..end,
                offset: h.text_at,
                line: h.line,
                detail: None,
                is_static: false,
                visibility: Visibility::Public,
                note: None,
                types: None,
            }
        })
        .collect();
    FileSymbols { language: crate::LANGUAGE.as_str().into(), symbols, ..FileSymbols::default() }
}

/// The lines of `text` with where each starts, without their line break.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    text.split_inclusive('\n').map(move |raw| {
        let start = at;
        at += raw.len();
        (start, raw.trim_end_matches(['\n', '\r']))
    })
}

/// The headings, in file order.
pub(crate) fn headings(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    // The fence a code block opened with (its character and length), while in one.
    let mut fence: Option<(u8, usize)> = None;
    // The previous line, if it can be a setext heading's text (a paragraph's line).
    let mut previous: Option<(usize, &str, u32)> = None;
    let mut front_matter = false;
    for (n, (start, line)) in lines(text).enumerate() {
        let number = u32::try_from(n).unwrap_or(u32::MAX);
        // The front matter (`---` on the first line, to the next `---` or `...`).
        if n == 0 && line.trim_end() == "---" {
            front_matter = true;
            continue;
        }
        if front_matter {
            if matches!(line.trim_end(), "---" | "...") {
                front_matter = false;
            }
            continue;
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        let body = line.trim_start_matches(' ');
        if let Some((ch, len)) = fence {
            if indent < 4 && closes_fence(body, ch, len) {
                fence = None;
            }
            previous = None;
            continue;
        }
        if indent < 4
            && let Some(opened) = opens_fence(body)
        {
            fence = Some(opened);
            previous = None;
            continue;
        }
        if indent < 4 {
            if let Some((level, title)) = atx(body) {
                // The title is a part of the line: where it starts in it.
                let text_at = start + (title.as_ptr() as usize - line.as_ptr() as usize);
                out.push(heading(level, title, start, text_at, number));
                previous = None;
                continue;
            }
            if let Some((p_start, p_line, p_number)) = previous
                && let Some(level) = setext(body)
            {
                let p_indent = p_line.len() - p_line.trim_start().len();
                out.push(heading(level, p_line.trim(), p_start, p_start + p_indent, p_number));
                previous = None;
                continue;
            }
        }
        // A paragraph's line may be underlined next; a blank line, a list item or a quote
        // can't (a `---` after them is a rule or an item).
        let paragraph = !body.is_empty()
            && indent < 4
            && !body.starts_with(['>', '-', '*', '+', '|', '<'])
            && !body.starts_with(|c: char| c.is_ascii_digit());
        previous = paragraph.then_some((start, line, number));
    }
    out
}

fn heading(level: u8, title: &str, start: usize, text_at: usize, line: u32) -> Heading {
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = match title.char_indices().nth(MAX_NAME) {
        Some((cut, _)) => format!("{}…", &title[..cut]),
        None => title,
    };
    Heading { level, text, start, text_at, line }
}

/// An ATX heading's level and text (its closing `#`s left out): `## Title ##` → (2, "Title").
fn atx(body: &str) -> Option<(u8, &str)> {
    let hashes = body.len() - body.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &body[hashes..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim();
    // A closing sequence only after a space (`# C#` keeps its `#`).
    let closed = rest.trim_end_matches('#');
    let title =
        if closed.is_empty() || closed.ends_with([' ', '\t']) { closed.trim_end() } else { rest };
    Some((u8::try_from(hashes).unwrap_or(6), title))
}

/// A setext underline's level: `===` → 1, `---` → 2.
fn setext(body: &str) -> Option<u8> {
    let line = body.trim_end();
    let first = line.chars().next()?;
    (matches!(first, '=' | '-') && line.chars().all(|c| c == first)).then_some(if first == '=' {
        1
    } else {
        2
    })
}

/// A fence opening a code block (three or more backticks or tildes): its character and
/// length.
fn opens_fence(body: &str) -> Option<(u8, usize)> {
    let ch = *body.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = body.len() - body.trim_start_matches(char::from(ch)).len();
    // A backtick fence's info string has no backtick (else it's inline code).
    let info_ok = ch == b'~' || !body[len..].contains('`');
    (len >= 3 && info_ok).then_some((ch, len))
}

/// The fence closing a block opened with `len` × `ch`: at least as long, nothing after it.
fn closes_fence(body: &str, ch: u8, len: usize) -> bool {
    let run = body.len() - body.trim_start_matches(char::from(ch)).len();
    run >= len && body[run..].trim().is_empty()
}

#[cfg(test)]
#[path = "../tests/unit/outline.rs"]
mod tests;
