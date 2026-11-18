//! CSS selectors, shared by the languages that hold CSS (stylesheets, and the `<style>`
//! blocks of HTML, PHP, Blade…): the classes and ids a selector declares.

use code_intelligence::{FileSymbols, Symbol, SymbolKind};

/// A character of a CSS name (a class, an id, a property).
pub fn is_name(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// The classes (`.card`) and ids (`#main`) in a selector, with their offsets in it (at
/// the name, after `.` / `#`); attribute selectors and `:not(…)` arguments are skipped.
pub fn selector_names(selector: &str) -> Vec<(SymbolKind, String, usize)> {
    let chars: Vec<(usize, char)> = selector.char_indices().collect();
    let mut out = Vec::new();
    let mut bracket = 0;
    for (k, &(i, c)) in chars.iter().enumerate() {
        match c {
            '[' | '(' => bracket += 1,
            ']' | ')' => bracket -= 1,
            '.' | '#' if bracket == 0 => {
                let name: String =
                    chars[k + 1..].iter().map(|(_, c)| *c).take_while(|c| is_name(*c)).collect();
                let valid =
                    name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '-');
                if valid {
                    let kind = if c == '.' { SymbolKind::CssClass } else { SymbolKind::CssId };
                    out.push((kind, name, i + 1));
                }
            }
            _ => {}
        }
    }
    out
}

/// The classes and ids declared by the CSS in `text[range]` (a `<style>` block's content),
/// added to `file` with offsets in `text`: every selector before a `{` (at any depth, so
/// inside `@media` too), at-rule preludes and comments skipped.
pub fn add_css_selectors(text: &str, range: std::ops::Range<usize>, file: &mut FileSymbols) {
    let css = &text[range.clone()];
    // The lines counted once up to the block (then on from there), the names already there
    // in a set: a big `<style>` block's thousands of selectors, not a pass over all for each.
    let before = text[..range.start].matches('\n').count() as u32;
    let breaks: Vec<usize> = css.match_indices('\n').map(|(i, _)| range.start + i).collect();
    let line_of = |offset: usize| before + breaks.partition_point(|&b| b < offset) as u32;
    let mut seen: std::collections::HashSet<(SymbolKind, String)> =
        file.symbols.iter().map(|s| (s.kind, s.name.to_string())).collect();
    let mut start = 0;
    let mut i = 0;
    let bytes = css.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = css[i + 2..].find("*/").map_or(bytes.len(), |e| i + 2 + e + 2);
                start = i;
                continue;
            }
            b'{' => {
                let prelude = &css[start..i];
                if !prelude.trim_start().starts_with('@') {
                    for (kind, name, at) in selector_names(prelude) {
                        if !seen.insert((kind, name.clone())) {
                            continue;
                        }
                        let offset = range.start + start + at;
                        file.symbols.push(Symbol {
                            fqn: name.clone().into_boxed_str(),
                            name: name.into_boxed_str(),
                            kind,
                            container: None,
                            parents: Box::default(),
                            range: offset..offset,
                            offset,
                            line: line_of(offset),
                            detail: None,
                            is_static: false,
                            visibility: Default::default(),
                            note: None,
                            types: None,
                        });
                    }
                }
                start = i + 1;
            }
            b'}' | b';' => start = i + 1,
            _ => {}
        }
        i += 1;
    }
}

/// The classes and ids declared in the `<style>` blocks of an HTML-like text (HTML, PHP,
/// templates), added to `file`.
pub fn add_style_blocks(text: &str, file: &mut FileSymbols) {
    let mut from = 0;
    while let Some(open) = find_ignoring_case(text, from, b"<style") {
        let Some(content) = text[open..].find('>').map(|i| open + i + 1) else { break };
        let end = find_ignoring_case(text, content, b"</style").unwrap_or(text.len());
        add_css_selectors(text, content..end, file);
        from = end;
    }
}

/// Where ASCII `needle` (lowercase) is in `text` from `from` on, whatever its letters' case
/// (not a lowercased copy of the whole text at each look).
fn find_ignoring_case(text: &str, from: usize, needle: &[u8]) -> Option<usize> {
    let bytes = text.as_bytes();
    let first = needle.first()?;
    (from..bytes.len().saturating_sub(needle.len() - 1)).find(|&i| {
        bytes[i].to_ascii_lowercase() == *first
            && bytes[i..i + needle.len()].eq_ignore_ascii_case(needle)
    })
}

/// The CSS rule around the selector name at `offset` (`.abc, .x { color: red; }`), at most
/// 12 lines, its body re-indented by 4 spaces.
pub fn rule_at(text: &str, offset: usize) -> Option<String> {
    let before = &text[..offset];
    let start = before.rfind(['{', '}', ';', '>']).map_or(0, |i| i + 1);
    let open = text[offset..].find('{').map(|i| offset + i)?;
    let mut depth = 0;
    let mut close = text.len();
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let selector = text[start..open].split_whitespace().collect::<Vec<_>>().join(" ");
    let body: Vec<String> = text[open + 1..close]
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| format!("    {l}"))
        .collect();
    let mut lines = vec![format!("{selector} {{")];
    let more = body.len() > 10;
    lines.extend(body.into_iter().take(10));
    if more {
        lines.push("    …".to_owned());
    }
    lines.push("}".to_owned());
    Some(lines.join("\n"))
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
