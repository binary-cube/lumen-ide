//! What a place in a stylesheet's comment is (plan 75): CSS's `/* */` and SCSS's `//` are
//! text; SCSS's SassDoc (`///`) names code — `@see` / `@require` a mixin, a function or a
//! `$variable`, `@param $x` the mixin's parameter. A URL in any of them stays a link (shown
//! as one elsewhere).

use code_intelligence::comments::{self, first_word};
use code_intelligence::{CommentPlace, SymbolKind, Target};
use stylesheet::is_name;

/// What `offset` of `text` is, when it's in a comment (`scss`: `//` and `///` are comments
/// too).
pub fn place(text: &str, offset: usize, scss: bool) -> Option<CommentPlace> {
    let offset = text.floor_char_boundary(offset);
    if comments::block_at(text, offset, &|line| hidden(line, scss)).is_some() {
        return Some(CommentPlace::Text);
    }
    if !scss {
        return None;
    }
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let opener = line_comment(&text[line_start..offset])?;
    let comment = &text[line_start + opener..];
    match comment.starts_with("///") {
        true => Some(sass_doc(text, line_start + opener + 3, offset)),
        false => Some(CommentPlace::Text),
    }
}

/// [`place`] in a CSS stylesheet.
pub fn in_css(text: &str, offset: usize) -> Option<CommentPlace> {
    place(text, offset, false)
}

/// [`place`] in an SCSS one.
pub fn in_scss(text: &str, offset: usize) -> Option<CommentPlace> {
    place(text, offset, true)
}

/// Where a `//` opens on `line` (before the place), outside strings and `url(…)`.
fn line_comment(line: &str) -> Option<usize> {
    let (mut quote, mut escaped, mut url) = (None, false, false);
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match (c, quote) {
            (b'\\', Some(_)) => escaped = true,
            (b'\'' | b'"', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            (b'(', None) if line[..i].ends_with("url") => url = true,
            (b')', None) => url = false,
            (b'/', None) if !url && bytes.get(i + 1) == Some(&b'/') => return Some(i),
            _ => {}
        }
    }
    None
}

/// Whether a `/*` at the end of `line` is in a string (or after SCSS's `//`).
fn hidden(line: &str, scss: bool) -> bool {
    let quotes = line.matches(['"', '\'']).count() % 2 == 1;
    quotes || (scss && line_comment(line).is_some())
}

/// A SassDoc line's text (from `body`, after `///`): its tag, then what the place is.
fn sass_doc(text: &str, body: usize, offset: usize) -> CommentPlace {
    let line_end = text[body..].find('\n').map_or(text.len(), |i| body + i);
    let line = &text[body..line_end];
    let lead = line.len() - line.trim_start().len();
    let Some(tag) = line[lead..].strip_prefix('@') else { return CommentPlace::Text };
    let name_len = tag.find(char::is_whitespace).unwrap_or(tag.len());
    let rest_start = body + lead + 1 + name_len;
    if offset < rest_start {
        return CommentPlace::Text;
    }
    let rest = &text[rest_start..line_end];
    let at = offset - rest_start;
    match &tag[..name_len] {
        "see" | "require" | "requires" => named(rest, at),
        "param" | "parameter" | "arg" | "argument" => parameter(text, line_end, rest, at),
        _ => CommentPlace::Text,
    }
}

/// `@see name` / `@require $var`: a mixin or a function, or a variable (`$`), `%`a
/// placeholder (none indexed: text); after the name, text.
fn named(rest: &str, at: usize) -> CommentPlace {
    let (Some(word), _) = first_word(rest) else { return CommentPlace::Text };
    if !(word.start..=word.end).contains(&at) {
        return CommentPlace::Text;
    }
    let written = &rest[word];
    let (sigil, name) = match written.strip_prefix('$') {
        Some(name) => ("$", name),
        None => ("", written),
    };
    let name: String = name.chars().take_while(|c| is_name(*c)).collect();
    if name.is_empty() || written.starts_with('%') {
        return CommentPlace::Text;
    }
    let kind = if sigil.is_empty() { SymbolKind::Mixin } else { SymbolKind::CssVariable };
    CommentPlace::Code(Target::Named { name: format!("{sigil}{name}"), kinds: vec![kind] })
}

/// `@param {Type} $x [default] description`: `$x` is the parameter of the mixin or function
/// declared after the comment (the line ending at `line_end`); its type and the rest text.
fn parameter(text: &str, line_end: usize, rest: &str, at: usize) -> CommentPlace {
    let trimmed = rest.trim_start();
    let mut after = rest.len() - trimmed.len();
    if trimmed.starts_with('{') {
        let Some(close) = rest[after..].find('}') else { return CommentPlace::Text };
        after += close + 1;
    }
    let (Some(word), _) = first_word(&rest[after..]) else { return CommentPlace::Text };
    let word = after + word.start..after + word.end;
    if !(word.start..=word.end).contains(&at) {
        return CommentPlace::Text;
    }
    let Some(name) = rest[word].strip_prefix('$') else { return CommentPlace::Text };
    let name: String = name.chars().take_while(|c| is_name(*c)).collect();
    match declared_parameter(text, line_end, &name) {
        Some(found) => CommentPlace::Declared(found),
        None => CommentPlace::Text,
    }
}

/// Where `$name` is declared in the parameters of the first `@mixin` / `@function` after
/// `from` (past the comment's other lines).
fn declared_parameter(text: &str, from: usize, name: &str) -> Option<usize> {
    let after = &text[from..];
    let declared =
        after.lines().map(str::trim_start).find(|l| !l.starts_with("//") && !l.is_empty())?;
    if !(declared.starts_with("@mixin") || declared.starts_with("@function")) {
        return None;
    }
    let line_at = declared.as_ptr() as usize - text.as_ptr() as usize;
    let open = declared.find('(')?;
    let close = declared[open..].find(')').map_or(declared.len(), |i| open + i);
    let needle = format!("${name}");
    let params = &declared[open..close];
    let mut from = 0;
    while let Some(found) = params[from..].find(&needle) {
        let at = from + found;
        let end = at + needle.len();
        from = end;
        if !params[end..].starts_with(is_name) {
            return Some(line_at + open + at);
        }
    }
    None
}

#[cfg(test)]
#[path = "../tests/unit/comment_places.rs"]
mod tests;

/// SassDoc's parts colored as code (plan 75h), on `///` lines: the tags, the `{type}`
/// after a typed tag, the `$name` it documents.
#[derive(Debug, Default)]
pub struct SassDocParts;

impl syntax_ts::DocComments for SassDocParts {
    fn language(&self) -> languages::LanguageId {
        languages::LanguageId::new_static("scss")
    }

    fn nodes(&self) -> &'static [&'static str] {
        &["js_comment", "comment"]
    }

    fn parts(&self, comment: &str) -> Vec<(std::ops::Range<usize>, syntax_ts::DocPart)> {
        if !comment.starts_with("///") {
            return Vec::new();
        }
        let style = code_intelligence::comments::DocStyle {
            typed: &|tag| {
                matches!(
                    tag,
                    "param"
                        | "arg"
                        | "argument"
                        | "parameter"
                        | "return"
                        | "returns"
                        | "type"
                        | "prop"
                        | "property"
                        | "throw"
                        | "throws"
                        | "exception"
                )
            },
            named: &|tag| {
                matches!(tag, "param" | "arg" | "argument" | "parameter" | "prop" | "property")
            },
            is_name: &|_| true,
            type_len: None,
        };
        use code_intelligence::comments::DocText;
        code_intelligence::comments::doc_parts(comment, &style)
            .into_iter()
            .map(|(r, t)| {
                let part = match t {
                    DocText::Tag => syntax_ts::DocPart::Tag,
                    DocText::Type => syntax_ts::DocPart::Type,
                    DocText::Name => syntax_ts::DocPart::Variable,
                };
                (r, part)
            })
            .collect()
    }
}
