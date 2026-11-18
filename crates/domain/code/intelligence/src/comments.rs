//! What the languages share to read their comments (plan 75): where the block comment
//! around a place opens, the line of a doc comment without its margin, the name around a
//! place. Each language says the rest ([`crate::LanguageIntelligence::comment_place`]):
//! its strings and line comments, its tags. Read from the text alone, no parse.

use std::ops::Range;

/// Where the block comment (`/* … */`) around `offset` opens and where it closes (after
/// its `*/`, or the text's end), if `offset` is in one. `hidden(line)`: whether the end of
/// `line` (a line's start up to an opener) is in a string or a line comment, as the
/// language writes them; such an opener isn't one.
pub fn block_at(
    text: &str,
    offset: usize,
    hidden: &dyn Fn(&str) -> bool,
) -> Option<(usize, usize)> {
    let before = text.get(..offset)?;
    let mut end = before.len();
    while let Some(open) = before[..end].rfind("/*") {
        let line_start = before[..open].rfind('\n').map_or(0, |i| i + 1);
        if hidden(&before[line_start..open]) {
            end = open;
            continue;
        }
        if before[open..].contains("*/") {
            return None;
        }
        let close = text[offset..].find("*/").map_or(text.len(), |i| offset + i + 2);
        return Some((open, close));
    }
    None
}

/// A block comment opened at `open` is a doc comment (`/** … */`, not `/**/`).
pub fn is_doc(text: &str, open: usize) -> bool {
    text[open..].starts_with("/**") && !text[open..].starts_with("/**/")
}

/// The line of `offset` in the comment `open..close`: where its text starts (after `/**`,
/// the `*` margin and the spaces) and where it ends (at the line's end or `*/`).
pub fn doc_line(text: &str, (open, close): (usize, usize), offset: usize) -> Range<usize> {
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1).max(open);
    let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i).min(close);
    let line = &text[line_start..line_end];
    let t = line.trim_start();
    let t = t.strip_prefix("/**").unwrap_or(t);
    let t = t.trim_start_matches('*').trim_start();
    let end = line_end - line.len() + line.trim_end_matches("*/").trim_end().len();
    let start = line_start + line.len() - t.len();
    start..end.max(start)
}

/// The name around `at` of `s`: its characters are letters, digits, `_` and those `part`
/// adds (a namespace's `\`, a pseudo-type's `-`); `None` on anything else. Right after a
/// name (`at` at its end) counts as on it.
pub fn word_around(s: &str, at: usize, part: &dyn Fn(char) -> bool) -> Option<Range<usize>> {
    let at = s.floor_char_boundary(at.min(s.len()));
    let is = |c: char| c.is_alphanumeric() || c == '_' || part(c);
    let around = |at: usize| {
        let start =
            s[..at].char_indices().rev().take_while(|(_, c)| is(*c)).last().map_or(at, |(i, _)| i);
        let end = s[at..].char_indices().find(|(_, c)| !is(*c)).map_or(s.len(), |(i, _)| at + i);
        let named = s[start..end].chars().any(|c| c.is_alphanumeric() || c == '_');
        named.then_some(start..end)
    };
    around(at).or_else(|| {
        let before = s[..at].chars().next_back()?;
        around(at - before.len_utf8())
    })
}

/// The first word of `rest` (after its spaces): its range, and where the text after it
/// starts.
pub fn first_word(rest: &str) -> (Option<Range<usize>>, usize) {
    let start = rest.len() - rest.trim_start().len();
    let end = rest[start..].find(char::is_whitespace).map_or(rest.len(), |i| start + i);
    ((start < end).then_some(start..end), end)
}

/// The code written between backticks around `offset`, on its line (`` `Cart::total()` ``,
/// a run of several backticks closed by as many): the range inside them. A comment's text
/// written so is code (plan 75), however the language reads it.
pub fn backticked(text: &str, offset: usize) -> Option<Range<usize>> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    let bytes = &text.as_bytes()[start..end];
    let at = offset - start;
    let (mut i, mut open) = (0, None::<(usize, usize)>);
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let ticks = bytes[i..].iter().take_while(|&&b| b == b'`').count();
        match open {
            None => open = Some((i + ticks, ticks)),
            Some((inside, n)) if n == ticks => {
                if (inside..=i).contains(&at) && inside < i {
                    return Some(start + inside..start + i);
                }
                open = None;
            }
            Some(_) => {}
        }
        i += ticks;
    }
    None
}

/// The word of `code` (split at spaces) around `at`, and the word before it (`new` of
/// `new Cart()`).
pub fn code_word(code: &str, at: usize) -> Option<(Range<usize>, Option<&str>)> {
    let mut previous = None;
    for word in code.split_whitespace() {
        let start = word.as_ptr() as usize - code.as_ptr() as usize;
        let range = start..start + word.len();
        if (range.start..=range.end).contains(&at) {
            return Some((range, previous));
        }
        previous = Some(word);
    }
    None
}

/// A doc tag being typed at `offset` (`@ret|`): where its `@` is, when it starts the
/// line's text (after the comment's `/**`, `*` or `///` margin) or follows a `{` (an inline
/// tag, `{@li|`). An `@` in a word (an address) isn't one.
pub fn tag_at(text: &str, offset: usize) -> Option<usize> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = &before[line_start..];
    let name =
        line.len() - line.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '-').len();
    let at = line.len() - name;
    let margin = line[..at].strip_suffix('@')?;
    let lead = margin.trim_end_matches(|c: char| c.is_whitespace());
    let opens = lead.ends_with('{') || lead.trim_start_matches(['/', '*', ' ', '\t']).is_empty();
    opens.then_some(line_start + at - 1)
}

/// The suggestions for doc tags: each tag (`@param`), its description from the locales
/// (`<section>.<tag>`, `-` written `_`), a space after those followed by something
/// (`spaced`).
pub fn tag_items(tags: &[&str], section: &str, spaced: &[&str]) -> Vec<crate::CompletionItem> {
    tags.iter()
        .map(|tag| {
            let name = tag.trim_start_matches('@');
            let detail = i18n::t(&format!("{section}.{}", name.replace('-', "_")));
            let mut item =
                crate::CompletionItem::word(tag, crate::CompletionKind::Keyword, Some(&detail));
            if spaced.contains(&name) {
                item.insert = format!("{tag} ");
            }
            item
        })
        .collect()
}

/// A part of a doc comment written as code (colored as such).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocText {
    Tag,
    Type,
    Name,
}

/// How a language writes its doc comments' tags.
#[allow(missing_debug_implementations)] // Functions only: nothing to show.
pub struct DocStyle<'a> {
    /// The tag (without `@`) is followed by a type.
    pub typed: &'a dyn Fn(&str) -> bool,
    /// The tag documents a name (after its type, if any).
    pub named: &'a dyn Fn(&str) -> bool,
    /// The word there is such a name (PHPDoc's `$x`: else it's the description's).
    pub is_name: &'a dyn Fn(&str) -> bool,
    /// How long the type at the start of a text is (PHPDoc's first word, with its
    /// generics and spaces inside brackets); `None`: types are in braces (`{…}`: JSDoc,
    /// SassDoc).
    pub type_len: Option<&'a dyn Fn(&str) -> usize>,
}

/// The parts of doc comment `comment` (its whole text; ranges within it): on each line, a
/// tag starting its text (after the `/**`, `*` or `///` margin) or inline (`{@link`), the
/// type after a typed tag, the name after a tag that documents one.
pub fn doc_parts(comment: &str, style: &DocStyle<'_>) -> Vec<(Range<usize>, DocText)> {
    let mut out = Vec::new();
    let mut at = 0;
    for line in comment.split_inclusive('\n') {
        line_parts(line, at, style, &mut out);
        at += line.len();
    }
    out
}

/// Where the `}` closing the `{` that starts `s` is (nested ones counted).
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '\\')
}

fn line_parts(
    line: &str,
    base: usize,
    style: &DocStyle<'_>,
    out: &mut Vec<(Range<usize>, DocText)>,
) {
    let text = line.trim_start();
    let text = text.strip_prefix("/**").or_else(|| text.strip_prefix("///")).unwrap_or(text);
    let text = text.trim_start_matches('*').trim_start();
    let start = line.len() - text.len();
    // Inline tags anywhere on the line (`{@link Name}`, `{@see …}`).
    let mut search = 0;
    while let Some(found) = line[search..].find("{@") {
        let tag = search + found + 1;
        let len = line[tag + 1..].find(|c: char| !is_tag_char(c)).unwrap_or(line.len() - tag - 1);
        if len > 0 {
            out.push((base + tag..base + tag + 1 + len, DocText::Tag));
        }
        search = tag + 1;
    }
    let Some(after) = text.strip_prefix('@') else { return };
    let len = after.find(|c: char| !is_tag_char(c)).unwrap_or(after.len());
    if len == 0 {
        return;
    }
    let name = &after[..len];
    out.push((base + start..base + start + 1 + len, DocText::Tag));
    let mut at = start + 1 + len;
    let skip = |at: usize| at + line[at..].len() - line[at..].trim_start().len();
    if (style.typed)(name) {
        let from = skip(at);
        let rest = &line[from..];
        match style.type_len {
            Some(len) => {
                let len = len(rest).min(rest.len());
                if len > 0 {
                    out.push((base + from..base + from + len, DocText::Type));
                    at = from + len;
                }
            }
            None => {
                if let Some(close) = rest.starts_with('{').then(|| matching_brace(rest)).flatten() {
                    if close > 1 {
                        out.push((base + from + 1..base + from + close, DocText::Type));
                    }
                    at = from + close + 1;
                }
            }
        }
    }
    if (style.named)(name) {
        let from = skip(at);
        let rest = &line[from..];
        let len = rest
            .find(|c: char| {
                !(c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '&' | '[' | ']' | '='))
            })
            .unwrap_or(rest.len());
        // An optional one (`[name=default]`): its name alone.
        let word = &rest[..len];
        let lead = word.len() - word.trim_start_matches('[').len();
        let word = &word[lead..];
        let word = &word[..word.find(['=', ']']).unwrap_or(word.len())];
        if word.chars().any(|c| c.is_alphanumeric() || c == '_') && (style.is_name)(word) {
            let from = from + lead;
            out.push((base + from..base + from + word.len(), DocText::Name));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/comments.rs"]
mod tests;
