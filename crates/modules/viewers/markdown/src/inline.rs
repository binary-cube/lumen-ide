//! What GitHub does to a paragraph's plain text beyond CommonMark: bare URLs become links
//! (`https://…`, `www.…`), `:shortcodes:` become emoji, and headings get their anchors
//! (`#a-heading`).

use std::collections::HashMap;
use std::sync::OnceLock;

/// A piece of plain text: as written, or a link found in it (its destination).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a str),
    Link(&'a str, String),
}

/// `text` cut where bare URLs start and end (GFM's extended autolinks).
pub fn autolinks(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut from = 0;
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < text.len() {
        let boundary = i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'(' | b'*' | b'_');
        let rest = &text[i..];
        let scheme = ["https://", "http://", "www."].into_iter().find(|p| {
            rest.len() > p.len() && rest.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p))
        });
        if boundary && let Some(scheme) = scheme {
            let mut end = rest
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                .unwrap_or(rest.len());
            // Trailing punctuation isn't part of it, nor an unbalanced `)`.
            loop {
                let url = &rest[..end];
                let Some(last) = url.chars().last() else { break };
                let unbalanced = last == ')' && url.matches(')').count() > url.matches('(').count();
                if matches!(last, '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | ';' | '\'' | '"')
                    || unbalanced
                {
                    end -= last.len_utf8();
                } else {
                    break;
                }
            }
            let url = &rest[..end];
            if url.len() > scheme.len()
                && url[scheme.len()..].contains(|c: char| c.is_alphanumeric())
            {
                if from < i {
                    out.push(Piece::Text(&text[from..i]));
                }
                let dest = if scheme == "www." { format!("http://{url}") } else { url.to_owned() };
                out.push(Piece::Link(url, dest));
                i += end;
                from = i;
                continue;
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    if from < text.len() {
        out.push(Piece::Text(&text[from..]));
    }
    out
}

/// The shortcodes and their emoji (GitHub's gemoji, `data/emoji.data`), read once.
fn emoji_table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../data/emoji.data")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once(' '))
            .collect()
    })
}

/// The emoji of shortcode `name` (`rocket`), if GitHub knows it.
pub fn emoji(name: &str) -> Option<&'static str> {
    emoji_table().get(name).copied()
}

/// `text` with its `:shortcodes:` replaced by their emoji (unknown ones kept).
pub fn emojify(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(':') {
        return text.into();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')))
            .unwrap_or(after.len());
        if end > 0
            && after[end..].starts_with(':')
            && let Some(e) = emoji(&after[..end])
        {
            out.push_str(e);
            rest = &after[end + 1..];
        } else {
            out.push(':');
            rest = after;
        }
    }
    out.push_str(rest);
    out.into()
}

/// A heading's anchor as GitHub makes it: lowercase, punctuation dropped (but `-` and `_`),
/// spaces as `-`; a repeated one gets `-1`, `-2`… (`seen` counts them).
pub fn slug(text: &str, seen: &mut HashMap<String, usize>) -> String {
    let base: String = text
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect();
    let n = seen.entry(base.clone()).or_insert(0);
    let slug = if *n == 0 { base } else { format!("{base}-{n}") };
    *n += 1;
    slug
}

#[cfg(test)]
#[path = "../tests/unit/inline.rs"]
mod tests;
