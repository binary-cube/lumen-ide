//! Small text helpers: an error's message for the tooltip, a common prefix, collapsed whitespace.

use super::*;

/// A syntax error's message, in the UI language (with the unexpected code fragment).
pub(super) fn error_message(
    doc: &Document,
    error: &syntax_ts::SyntaxError,
) -> (String, Option<String>) {
    match &error.kind {
        syntax_ts::SyntaxErrorKind::Missing(token) => (tr!("error.missing"), Some(token.clone())),
        syntax_ts::SyntaxErrorKind::Unexpected => {
            let buffer = doc.buffer();
            let range = error.range.start.min(buffer.len())..error.range.end.min(buffer.len());
            let text = buffer.slice(range);
            let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
            if first.is_empty() {
                return (tr!("error.invalid"), None);
            }
            let mut excerpt: String = first.chars().take(32).collect();
            if first.chars().count() > 32 {
                excerpt.push('…');
            }
            (tr!("error.unexpected"), Some(excerpt))
        }
    }
}

/// The length of the bytes two chunked texts start with, compared a slice at a time
/// (`memcmp`), not byte by byte.
pub(super) fn common_prefix<'a>(
    mut a: impl Iterator<Item = &'a str>,
    mut b: impl Iterator<Item = &'a str>,
) -> usize {
    let (mut x, mut y): (&[u8], &[u8]) = (&[], &[]);
    let mut same = 0;
    loop {
        if x.is_empty() {
            let Some(chunk) = a.next() else { return same };
            x = chunk.as_bytes();
            continue;
        }
        if y.is_empty() {
            let Some(chunk) = b.next() else { return same };
            y = chunk.as_bytes();
            continue;
        }
        let n = x.len().min(y.len());
        if x[..n] != y[..n] {
            return same + x.iter().zip(y).take_while(|(p, q)| p == q).count();
        }
        same += n;
        (x, y) = (&x[n..], &y[n..]);
    }
}

/// A declaration's header on a single line: spaces and newlines become one space;
/// the color ranges move into the new text.
pub(super) fn collapse_whitespace(
    raw: &str,
    colors: &[(std::ops::Range<usize>, Rgba)],
) -> (String, Vec<(std::ops::Range<usize>, Rgba)>) {
    let mut out = String::with_capacity(raw.len());
    let mut map = vec![0; raw.len() + 1];
    let mut space = false;
    for (i, c) in raw.char_indices() {
        map[i] = out.len();
        if c.is_whitespace() {
            if !space && !out.is_empty() {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    map[raw.len()] = out.len();
    out.truncate(out.trim_end().len());
    // Captures overlap (e.g. a whole declaration and the words in it): as in the editor,
    // the later one covers the earlier one. Applied per byte, then regrouped.
    let mut per_byte: Vec<Option<Rgba>> = vec![None; out.len()];
    for (r, color) in colors {
        if !raw.is_char_boundary(r.start.min(raw.len()))
            || !raw.is_char_boundary(r.end.min(raw.len()))
        {
            continue;
        }
        let (a, b) =
            (map[r.start.min(raw.len())].min(out.len()), map[r.end.min(raw.len())].min(out.len()));
        per_byte[a..b].fill(Some(*color));
    }
    let mut colors: Vec<(std::ops::Range<usize>, Rgba)> = Vec::new();
    for (i, c) in per_byte.into_iter().enumerate() {
        let Some(c) = c else { continue };
        match colors.last_mut() {
            Some((r, last)) if r.end == i && *last == c => r.end = i + 1,
            _ => colors.push((i..i + 1, c)),
        }
    }
    (out, colors)
}
