//! What a place in a JavaScript / TypeScript comment is (plan 75): in a JSDoc / TSDoc
//! comment (`/** … */`), the names of the `{types}` after the tags, the names the comments
//! declare (`@typedef`, `@callback`, `@template`), the parameter a `@param name` documents,
//! what `@see` / `@extends` / `{@link …}` name — code; everything else, and every other
//! comment (`//`, `/* */`), text.

use code_intelligence::comments::{self, first_word};
use code_intelligence::{CommentPlace, Target};

/// The tags whose `{type}` follows them.
const TYPED: &[&str] = &[
    "param",
    "arg",
    "argument",
    "property",
    "prop",
    "returns",
    "return",
    "type",
    "typedef",
    "throws",
    "exception",
    "this",
    "enum",
    "extends",
    "augments",
    "implements",
    "yields",
    "yield",
    "satisfies",
    "const",
    "constant",
    "member",
    "var",
    "define",
    "template",
    "typeParam",
];

/// The types the language knows by itself: they lead nowhere.
const BUILT_IN: &[&str] = &[
    "string",
    "number",
    "boolean",
    "bigint",
    "symbol",
    "object",
    "any",
    "unknown",
    "never",
    "void",
    "null",
    "undefined",
    "true",
    "false",
    "this",
    "keyof",
    "typeof",
    "readonly",
    "infer",
    "is",
    "asserts",
    "import",
];

/// What `offset` of `text` is, when it's in a comment.
pub fn place(text: &str, offset: usize) -> Option<CommentPlace> {
    let offset = text.floor_char_boundary(offset);
    let place = comment_place(text, offset)?;
    // Written between backticks, in any comment: code.
    match comments::backticked(text, offset) {
        Some(code) => Some(backticked(&text[code.clone()], offset - code.start)),
        None => Some(place),
    }
}

/// `at` in code written between backticks (`` `Cart#total` ``, `` `new Cart` ``,
/// `` `format()` ``): a function called, a class made, else read as a `@see` reference.
fn backticked(code: &str, at: usize) -> CommentPlace {
    let Some((word, before)) = comments::code_word(code, at) else { return CommentPlace::Text };
    let written = &code[word.clone()];
    let at = at - word.start;
    let (written, called) = match written.find('(') {
        Some(paren) if at <= paren => (&written[..paren], true),
        Some(_) => return CommentPlace::Text,
        None => (written.trim_end_matches([';', ',']), false),
    };
    let plain = !written.is_empty() && written.chars().all(is_name);
    match (before, called, plain) {
        (Some("new"), _, true) => CommentPlace::Code(Target::Class(written.to_owned())),
        (_, true, true) => CommentPlace::Code(Target::Function(written.to_owned())),
        _ if written.starts_with("this.") => CommentPlace::Text,
        _ => reference(written, at),
    }
}

/// What `offset` is in a comment, before backticks are looked at.
fn comment_place(text: &str, offset: usize) -> Option<CommentPlace> {
    if let Some(block) = comments::block_at(text, offset, &hidden) {
        return Some(match comments::is_doc(text, block.0) {
            true => in_doc(text, block, offset),
            false => CommentPlace::Text,
        });
    }
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    scan(&before[line_start..]).1.then_some(CommentPlace::Text)
}

/// `offset` is in a doc comment (`/** … */`).
pub(crate) fn in_doc_comment(text: &str, offset: usize) -> bool {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    comments::block_at(text, offset, &hidden).is_some_and(|(open, _)| comments::is_doc(text, open))
}

/// JSDoc's and TSDoc's tags, as completed after `@` (their descriptions in the locales:
/// `jsdoc.<tag>`).
pub(crate) const TAGS: &[&str] = &[
    "@param",
    "@returns",
    "@type",
    "@typedef",
    "@property",
    "@callback",
    "@template",
    "@throws",
    "@example",
    "@deprecated",
    "@see",
    "@link",
    "@since",
    "@author",
    "@default",
    "@description",
    "@enum",
    "@extends",
    "@implements",
    "@this",
    "@async",
    "@private",
    "@protected",
    "@public",
    "@readonly",
    "@override",
    "@satisfies",
    "@module",
    "@abstract",
    "@static",
    "@yields",
    "@todo",
    "@inheritDoc",
    "@remarks",
];

/// Where the type name typed at `offset` starts, when it's inside a tag's `{…}` in a doc
/// comment (`@param {string|Us|}`): after the type's last `{`, `|`, `&`, `<`, `(`, `,`, `?`,
/// `!`, `:`, `=` or space. `None`: not in a tag's braces (`{@link …}` is a link).
pub(crate) fn doc_type_at(text: &str, offset: usize) -> Option<usize> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    if !in_doc_comment(text, offset) {
        return None;
    }
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..offset];
    let open = line.rfind('{')?;
    if line[open..].contains('}') || line[open + 1..].starts_with('@') {
        return None;
    }
    // A tag before the braces on the line (`@param {`, `@type {`).
    if !line[..open].contains('@') {
        return None;
    }
    let inside = &line[open + 1..];
    let part = inside.rfind(|c: char| "{|&<(,?!:= ".contains(c)).map_or(0, |i| i + 1);
    Some(line_start + open + 1 + part)
}

/// The types JSDoc and TypeScript name themselves (suggested in a tag's braces).
pub(crate) const DOC_TYPES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "bigint",
    "symbol",
    "object",
    "Object",
    "any",
    "unknown",
    "void",
    "null",
    "undefined",
    "never",
    "Array",
    "Promise",
    "Record",
    "Partial",
    "Readonly",
    "Function",
    "Map",
    "Set",
    "Date",
    "RegExp",
    "Error",
];

/// The tags followed by something (a type, a name): a space after them.
pub(crate) const SPACED: &[&str] = &[
    "param",
    "returns",
    "type",
    "typedef",
    "property",
    "callback",
    "template",
    "throws",
    "see",
    "link",
    "since",
    "author",
    "default",
    "description",
    "enum",
    "extends",
    "implements",
    "this",
    "satisfies",
    "module",
    "yields",
    "todo",
    "remarks",
];

/// Whether the end of `line` is in a string or after `//`.
fn hidden(line: &str) -> bool {
    let (string, comment) = scan(line);
    string || comment
}

/// (in a string, after `//`) at the end of `line`.
fn scan(line: &str) -> (bool, bool) {
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match (c, quote) {
            (b'\\', Some(_)) => escaped = true,
            (b'\'' | b'"' | b'`', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            (b'/', None) if bytes.get(i + 1) == Some(&b'/') => return (false, true),
            _ => {}
        }
    }
    (quote.is_some(), false)
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// In the doc comment `block`: the tag of `offset`'s line, then what it is there.
fn in_doc(text: &str, block: (usize, usize), offset: usize) -> CommentPlace {
    let line = comments::doc_line(text, block, offset);
    if offset < line.start || offset > line.end {
        return CommentPlace::Text;
    }
    // An inline link (`{@link Name}`, `{@linkcode …}`, `{@linkplain …}`).
    if let Some(place) = inline_link(text, line.clone(), offset) {
        return place;
    }
    let body = &text[line.clone()];
    let Some(tag) = body.strip_prefix('@') else { return CommentPlace::Text };
    let name_len = tag.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(tag.len());
    let name = &tag[..name_len];
    let rest_start = line.start + 1 + name_len;
    if offset < rest_start {
        return CommentPlace::Text;
    }
    let rest = &text[rest_start..line.end];
    let at = offset - rest_start;
    match name {
        "see" => reference(rest, at),
        n if TYPED.contains(&n) => typed(text, block, rest, at, n),
        "callback" => match first_word(rest) {
            (Some(w), _) if w.contains(&at) || w.end == at => {
                CommentPlace::Declared(rest_start + w.start)
            }
            _ => CommentPlace::Text,
        },
        _ => CommentPlace::Text,
    }
}

/// A tag with a `{type}`: its names are code; the word after it is the parameter it
/// documents (`@param`), the name it declares (`@typedef`, `@template`) or a class
/// (`@extends Base`, without braces); the rest text.
fn typed(text: &str, block: (usize, usize), rest: &str, at: usize, tag: &str) -> CommentPlace {
    let base = rest.as_ptr() as usize - text.as_ptr() as usize;
    let skipped = rest.len() - rest.trim_start().len();
    let mut after = skipped;
    if rest[skipped..].starts_with('{') {
        let Some(close) = matching(rest, skipped) else { return CommentPlace::Text };
        if (skipped + 1..=close).contains(&at) {
            let ty = &rest[skipped + 1..close];
            return in_type(text, block.0, ty, at - skipped - 1);
        }
        if at <= skipped {
            return CommentPlace::Text;
        }
        after = close + 1;
    }
    let tail = &rest[after..];
    let (Some(word), _) = first_word(tail) else { return CommentPlace::Text };
    let word = after + word.start..after + word.end;
    // `@template T, U` names several.
    let names_several = matches!(tag, "template" | "typeParam");
    if !(word.start..=word.end).contains(&at) && !names_several {
        return CommentPlace::Text;
    }
    match tag {
        "param" | "arg" | "argument" => {
            // `[name=default]`, `name.prop`: the parameter is the name before them.
            let written = rest[word.clone()].trim_start_matches('[');
            let name: String = written.chars().take_while(|c| is_name(*c)).collect();
            let name_start = word.start + word.len() - written.len();
            if at > name_start + name.len() || name.is_empty() {
                return CommentPlace::Text;
            }
            match declared_parameter(text, block.1, &name) {
                Some(found) => CommentPlace::Declared(found),
                None => CommentPlace::Text,
            }
        }
        "typedef" => CommentPlace::Declared(base + word.start),
        "template" | "typeParam" => {
            let names = &rest[after..];
            let Some(range) = comments::word_around(names, at.saturating_sub(after), &|_| false)
            else {
                return CommentPlace::Text;
            };
            if at < after {
                return CommentPlace::Text;
            }
            CommentPlace::Declared(base + after + range.start)
        }
        "extends" | "augments" | "implements" => {
            let name: String =
                rest[word].chars().take_while(|c| is_name(*c) || *c == '.').collect();
            CommentPlace::Code(Target::Class(last_part(&name).to_owned()))
        }
        _ => CommentPlace::Text,
    }
}

/// The `}` matching the `{` at `open` of `s`.
fn matching(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The last part of a dotted name (`ns.Type` → `Type`).
fn last_part(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// `at` in the type `ty` (inside a comment opened at `open`): a name of it is code, unless
/// it's built in, an object type's key, a literal, or a name the comments declare.
fn in_type(text: &str, open: usize, ty: &str, at: usize) -> CommentPlace {
    let Some(word) = comments::word_around(ty, at, &|c| c == '$') else {
        return CommentPlace::Text;
    };
    let name = &ty[word.clone()];
    let before = &ty[..word.start];
    let after = &ty[word.end..];
    let quotes = before.matches(['\'', '"', '`']).count();
    if quotes % 2 == 1 || name.starts_with(|c: char| c.is_ascii_digit()) {
        return CommentPlace::Text;
    }
    // An object type's key: `{a: T, b?: U}`.
    let next = after.trim_start();
    if next.starts_with(':') || next.starts_with("?:") {
        return CommentPlace::Text;
    }
    // `import('./mod')` itself is a path (a link elsewhere); `.Tip` after it a type.
    if BUILT_IN.contains(&name) || before.trim_end().ends_with("import(") {
        return CommentPlace::Text;
    }
    if !before.ends_with('.')
        && let Some(found) = declared_name(text, open, name)
    {
        return CommentPlace::Declared(found);
    }
    CommentPlace::Code(Target::Class(name.to_owned()))
}

/// Where a comment from the text's start to this one (opened at `open`) declares `name`:
/// `@typedef {…} Name`, `@callback Name`, `@template T`.
fn declared_name(text: &str, open: usize, name: &str) -> Option<usize> {
    let close = text[open..].find("*/").map_or(text.len(), |i| open + i);
    let mut best = None;
    let mut from = 0;
    while let Some(found) = text[from..close].find('@') {
        let tag_at = from + found + 1;
        from = tag_at;
        let tag_end = text[tag_at..close]
            .find(|c: char| c.is_whitespace() || c == '{')
            .map_or(close, |i| tag_at + i);
        let tag = &text[tag_at..tag_end];
        if !matches!(tag, "typedef" | "callback" | "template" | "typeParam") {
            continue;
        }
        let line_end = text[tag_end..close].find('\n').map_or(close, |i| tag_end + i);
        let mut rest_at = tag_end;
        let rest = text[tag_end..line_end].trim_start();
        rest_at += text[tag_end..line_end].len() - rest.len();
        // After the `{type}`, if any.
        let names_at = match rest.starts_with('{') {
            true => match matching(rest, 0) {
                Some(close) => rest_at + close + 1,
                None => continue,
            },
            false => rest_at,
        };
        let names = &text[names_at..line_end];
        let several = matches!(tag, "template" | "typeParam");
        for (i, word) in names
            .split(|c: char| c.is_whitespace() || c == ',')
            .scan(0usize, |pos, w| {
                let at = *pos;
                *pos += w.len() + 1;
                Some((at, w))
            })
            .filter(|(_, w)| !w.is_empty())
            .take(if several { usize::MAX } else { 1 })
        {
            if word == name {
                best = Some(names_at + i);
            }
        }
    }
    best
}

/// Where `name` is declared among the parameters of the function after the comment ending
/// at `close` (`function f(a, b)`, `f(a, b) {`, `(a, b) =>`).
fn declared_parameter(text: &str, close: usize, name: &str) -> Option<usize> {
    let after = &text[close..];
    // The declaration's first line or two: its parameters open before a `{` or `=>`.
    let open = after.find('(')?;
    if after[..open].contains([';', '{', '}']) {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in after[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let params = &after[open..open + i];
                    let mut from = 0;
                    while let Some(found) = params[from..].find(name) {
                        let at = from + found;
                        let end = at + name.len();
                        from = end;
                        let before = params[..at].chars().next_back();
                        let next = params[end..].chars().next();
                        let whole = !before.is_some_and(|c| is_name(c) || c == '.')
                            && !next.is_some_and(is_name);
                        if whole {
                            return Some(close + open + at);
                        }
                    }
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

/// `@see` / `{@link …}`: `Name`, `Name#member`, `Name.member` (a class, then its member);
/// an address or a module path (`module:foo~Bar`) text.
fn reference(rest: &str, at: usize) -> CommentPlace {
    let (Some(word), _) = first_word(rest) else { return CommentPlace::Text };
    if !(word.start..=word.end).contains(&at) {
        return CommentPlace::Text;
    }
    let written = rest[word.clone()].trim_end_matches(['.', ',', '}', '|']);
    let written = written.split('|').next().unwrap_or(written);
    if written.contains("://") || written.contains(':') || written.contains('/') {
        return CommentPlace::Text;
    }
    let written = written.trim_end_matches("()");
    let rel = at - word.start;
    match written.find(['#', '.']) {
        Some(split) => {
            let (class, member) = (&written[..split], &written[split + 1..]);
            if rel <= split {
                return CommentPlace::Code(Target::Class(class.to_owned()));
            }
            match member.is_empty() {
                true => CommentPlace::Text,
                false => CommentPlace::Code(Target::Static {
                    class: class.to_owned(),
                    name: member.to_owned(),
                }),
            }
        }
        None if !written.is_empty() && written.chars().all(is_name) => {
            CommentPlace::Code(Target::Class(written.to_owned()))
        }
        None => CommentPlace::Text,
    }
}

/// An inline link on the line (`{@link Name}`, `{@linkcode Name}`, `{@linkplain Name}`)
/// around `offset`.
fn inline_link(text: &str, line: std::ops::Range<usize>, offset: usize) -> Option<CommentPlace> {
    let before = &text[line.start..offset];
    let open = before.rfind("{@")?;
    if before[open..].contains('}') {
        return None;
    }
    let tag_start = line.start + open + 2;
    let close = text[tag_start..line.end].find('}').map_or(line.end, |i| tag_start + i);
    if offset > close {
        return None;
    }
    let inner = &text[tag_start..close];
    let name_len = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let rest_start = tag_start + name_len;
    if offset < rest_start {
        return Some(CommentPlace::Text);
    }
    match &inner[..name_len] {
        "link" | "linkcode" | "linkplain" => {
            Some(reference(&text[rest_start..close], offset - rest_start))
        }
        _ => Some(CommentPlace::Text),
    }
}

#[cfg(test)]
#[path = "../tests/unit/comment_places.rs"]
mod tests;

/// JSDoc's and TSDoc's parts colored as code (plan 75h): the tags, the `{type}` after a
/// typed tag, the name it documents.
#[derive(Debug)]
pub struct DocParts(pub languages::LanguageId);

impl syntax_ts::DocComments for DocParts {
    fn language(&self) -> languages::LanguageId {
        self.0.clone()
    }

    fn nodes(&self) -> &'static [&'static str] {
        &["comment"]
    }

    fn parts(&self, comment: &str) -> Vec<(std::ops::Range<usize>, syntax_ts::DocPart)> {
        if !comments::is_doc(comment, 0) {
            return Vec::new();
        }
        let style = comments::DocStyle {
            typed: &|tag| TYPED.contains(&tag),
            named: &|tag| {
                matches!(
                    tag,
                    "param" | "arg" | "argument" | "property" | "prop" | "typedef" | "callback"
                )
            },
            is_name: &|_| true,
            type_len: None,
        };
        comments::doc_parts(comment, &style)
            .into_iter()
            .map(|(r, t)| {
                let part = match t {
                    comments::DocText::Tag => syntax_ts::DocPart::Tag,
                    comments::DocText::Type => syntax_ts::DocPart::Type,
                    comments::DocText::Name => syntax_ts::DocPart::Variable,
                };
                (r, part)
            })
            .collect()
    }
}
