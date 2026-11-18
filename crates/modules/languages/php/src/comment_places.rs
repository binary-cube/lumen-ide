//! What a place in a PHP comment is (plan 75): in a doc comment (`/** … */`), the types
//! after the tags that take one, the names the comment declares (`@template T`, a type
//! alias), the parameter a `@param $x` documents, what `@see` / `@uses` / `@covers` / `{@link}`
//! name, the method `@dataProvider` / `@depends` name, a Doctrine annotation's class — code;
//! everything else, and every other comment (`//`, `#`, `/* */`), text.

use code_intelligence::comments::{self, first_word};
use code_intelligence::{CommentPlace, Receiver, Target};

use crate::types::split_doc_type;

/// The tags followed by a type (without their `@phpstan-` / `@psalm-` prefix).
const TYPED: &[&str] = &[
    "var",
    "param",
    "param-out",
    "return",
    "returns",
    "throws",
    "property",
    "property-read",
    "property-write",
    "mixin",
    "extends",
    "implements",
    "use",
    "template-extends",
    "template-implements",
    "template-use",
    "assert",
    "assert-if-true",
    "assert-if-false",
    "self-out",
    "this-out",
    "require-extends",
    "require-implements",
];

/// The tags whose first word names something (`Class`, `Class::method()`, `function()`).
const REFERENCES: &[&str] = &["see", "uses", "used-by", "covers", "coversDefaultClass", "link"];

/// The types PHP and the static analysers know by themselves: they lead nowhere.
const BUILT_IN: &[&str] = &[
    "int",
    "integer",
    "string",
    "bool",
    "boolean",
    "float",
    "double",
    "array",
    "list",
    "iterable",
    "callable",
    "object",
    "mixed",
    "void",
    "never",
    "null",
    "false",
    "true",
    "resource",
    "scalar",
    "numeric",
    "max",
    "min",
    "empty",
    "noreturn",
    "never-return",
    "never-returns",
    "no-return",
];

/// What `offset` of `text` is, when it's in a comment.
pub fn place(text: &str, offset: usize) -> Option<CommentPlace> {
    let offset = text.floor_char_boundary(offset);
    let comment = comment_at(text, offset)?;
    // Written between backticks, in any comment: code.
    if let Some(code) = comments::backticked(text, offset) {
        return Some(backticked(&text[code.clone()], offset - code.start));
    }
    match comment {
        Comment::Doc { open, close } => Some(in_doc(text, open, close, offset)),
        Comment::Other => Some(CommentPlace::Text),
    }
}

/// `at` in code written between backticks (`` `Cart::total()` ``, `` `new Cart` ``,
/// `` `format()` ``): read as a `@see` reference, a call's arguments left out.
fn backticked(code: &str, at: usize) -> CommentPlace {
    let Some((word, before)) = comments::code_word(code, at) else { return CommentPlace::Text };
    let written = &code[word.clone()];
    let at = at - word.start;
    // A call's arguments: the name it calls (`total($cart)` is `total()`).
    let (written, called) = match written.find('(') {
        Some(paren) if at <= paren => (&written[..paren], true),
        Some(_) => return CommentPlace::Text,
        None => (written.trim_end_matches([';', ',', '.']), false),
    };
    if before == Some("new") {
        return match written.chars().all(is_name) && !written.is_empty() {
            true => CommentPlace::Code(Target::Class(written.to_owned())),
            false => CommentPlace::Text,
        };
    }
    let reference_text = if called { format!("{written}()") } else { written.to_owned() };
    reference(&reference_text, at)
}

enum Comment {
    /// A doc comment: where its `/**` starts and its `*/` ends (or the text's end).
    Doc { open: usize, close: usize },
    /// Another comment: `//`, `#`, `/* */`.
    Other,
}

/// The comment `offset` is in, if any: read from the text before it (no parse).
fn comment_at(text: &str, offset: usize) -> Option<Comment> {
    // A block comment opened before and not closed: its `/*` not in a string of its line.
    if let Some((open, close)) = comments::block_at(text, offset, &in_string_or_line_comment) {
        let doc = comments::is_doc(text, open);
        return Some(if doc { Comment::Doc { open, close } } else { Comment::Other });
    }
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    in_line_comment(&before[line_start..]).then_some(Comment::Other)
}

/// Whether the end of `line` (a line's start) is in a string, or after `//` / `#`.
fn in_string_or_line_comment(line: &str) -> bool {
    let (string, comment) = scan(line);
    string || comment
}

fn in_line_comment(line: &str) -> bool {
    scan(line).1
}

/// (in a string, after a line comment's start) at the end of `line`.
fn scan(line: &str) -> (bool, bool) {
    let (mut single, mut double, mut escaped) = (false, false, false);
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            b'\\' if single || double => escaped = true,
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b'/' if !single && !double && bytes.get(i + 1) == Some(&b'/') => return (false, true),
            b'#' if !single && !double && bytes.get(i + 1) != Some(&b'[') => return (false, true),
            _ => {}
        }
    }
    (single || double, false)
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\\'
}

/// The name around `at` of `s` (with `\`, and `-` for the pseudo-types): its range, or
/// `None` on anything else.
fn word_around(s: &str, at: usize) -> Option<std::ops::Range<usize>> {
    comments::word_around(s, at, &|c| c == '\\' || c == '-')
}

/// In the doc comment `open..close`: the tag of `offset`'s line, then what it is there.
fn in_doc(text: &str, open: usize, close: usize, offset: usize) -> CommentPlace {
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1).max(open);
    let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i).min(close);
    let line = &text[line_start..line_end];
    // The line's text: after `/**`, the `*` margin and the spaces.
    let lead = {
        let t = line.trim_start();
        let t = t.strip_prefix("/**").unwrap_or(t);
        let t = t.trim_start_matches('*').trim_start();
        line.len() - t.len()
    };
    let body_start = line_start + lead;
    if offset < body_start {
        return CommentPlace::Text;
    }
    // An inline tag (`{@see X}`, `{@link X}`) around the offset.
    if let Some(place) = inline_tag(text, line_start, line_end, offset) {
        return place;
    }
    let body = &text[body_start..line_end];
    let Some(tag) = body.strip_prefix('@') else { return CommentPlace::Text };
    let name_len = tag.find(|c: char| c.is_whitespace() || c == '(').unwrap_or(tag.len());
    let name = &tag[..name_len];
    let name_range = body_start + 1..body_start + 1 + name_len;
    let rest_start = name_range.end;
    let rest = &text[rest_start..line_end];
    let rest = rest.strip_suffix("*/").unwrap_or(rest);
    if offset < name_range.end || (offset == name_range.end && name_range.contains(&(offset - 1))) {
        if matches!(name, "inheritDoc" | "inheritdoc") {
            return inherited(text, close);
        }
        // On the tag itself: a Doctrine / Symfony annotation names its class.
        return match is_annotation(name) {
            true => CommentPlace::Code(Target::Class(name.to_owned())),
            false => CommentPlace::Text,
        };
    }
    let bare = ["phpstan-", "psalm-"].iter().find_map(|p| name.strip_prefix(p)).unwrap_or(name);
    let at = offset - rest_start;
    match bare {
        "covers" if rest.trim_start().starts_with("::") => covered(text, rest, at),
        b if TYPED.contains(&b) => typed(text, rest, at, (open, close), b),
        "template" | "template-covariant" | "template-contravariant" => {
            declaring(text, (rest_start, rest), at, open, &["of", "as", "="])
        }
        "type" => declaring(text, (rest_start, rest), at, open, &["="]),
        "import-type" => import_type((rest_start, rest), at),
        "method" => method(text, (rest_start, rest), at, open),
        b if REFERENCES.contains(&b) => reference(rest, at),
        _ if is_annotation(name) => annotation_argument(rest, at),
        "dataProvider" | "depends" => {
            let (word, _) = first_word(rest);
            match word.filter(|w| w.contains(&at)) {
                Some(w) => CommentPlace::Code(Target::Member {
                    receiver: Receiver::This,
                    name: rest[w].trim_end_matches("()").to_owned(),
                }),
                None => CommentPlace::Text,
            }
        }
        _ => CommentPlace::Text,
    }
}

/// A tag with a type (`@param Type $x description`): the type's names are code, `$x` the
/// parameter it documents (`@param`, `@param-out`, the assertions), the rest text.
fn typed(
    text: &str,
    rest: &str,
    at: usize,
    (open, close): (usize, usize),
    tag: &str,
) -> CommentPlace {
    let skipped = rest.len() - rest.trim_start().len();
    let (ty, after) = split_doc_type(rest);
    if ty.starts_with('$') && ty != "$this" {
        // No type (`@param $x`): the variable.
        return parameter_at(text, ty, at - skipped.min(at), close, tag);
    }
    if (skipped..skipped + ty.len()).contains(&at) || at == skipped + ty.len() && at > skipped {
        return in_type(text, ty, at - skipped, open);
    }
    let after_start = rest.len() - after.len();
    match at.checked_sub(after_start) {
        Some(rel) => parameter_at(text, after, rel, close, tag),
        None => CommentPlace::Text,
    }
}

/// `$x` after a parameter's tag: the function's parameter (declared after the comment).
fn parameter_at(text: &str, after: &str, at: usize, close: usize, tag: &str) -> CommentPlace {
    let documents =
        matches!(tag, "param" | "param-out" | "assert" | "assert-if-true" | "assert-if-false");
    let (Some(word), _) = first_word(after) else { return CommentPlace::Text };
    if !documents || !word.contains(&at) {
        return CommentPlace::Text;
    }
    let name = after[word].trim_start_matches('&').trim_start_matches("...");
    let Some(name) = name.strip_prefix('$') else { return CommentPlace::Text };
    let name: String = name.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    match declared_parameter(text, close, &name) {
        Some(at) => CommentPlace::Declared(at),
        None => CommentPlace::Text,
    }
}

/// Where `$name` is declared in the parameters of the function after the comment ending at
/// `close`.
fn declared_parameter(text: &str, close: usize, name: &str) -> Option<usize> {
    let after = &text[close..];
    let function = after.find("function")?;
    // Only the declaration this comment is on: nothing but modifiers and attributes between.
    let between = after[..function].trim();
    let modifiers = ["public", "protected", "private", "static", "abstract", "final"];
    if !between.split_whitespace().all(|w| modifiers.contains(&w) || w.starts_with("#[")) {
        return None;
    }
    let open = function + after[function..].find('(')?;
    let mut depth = 0usize;
    for (i, c) in after[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let params = &after[open..open + i];
                    let needle = format!("${name}");
                    let mut from = 0;
                    while let Some(found) = params[from..].find(&needle) {
                        let at = from + found;
                        let end = at + needle.len();
                        if !params[end..].starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                            return Some(close + open + at);
                        }
                        from = end;
                    }
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

/// `at` in the type `ty` written in another file's comment (a template's `@var`, its
/// comment opened at `open` of `text`): what [`in_type`] says of it.
pub(crate) fn type_place(text: &str, ty: &str, at: usize, open: usize) -> CommentPlace {
    in_type(text, ty, at, open)
}

/// `at` in the type `ty` (of a comment of `text` opened at `open`): a name of it is code, unless it's
/// built in, a shape's key, a literal, or a name the comments declare (`T`, an alias).
fn in_type(text: &str, ty: &str, at: usize, open: usize) -> CommentPlace {
    let at = at.min(ty.len());
    let Some(word) = word_around(ty, at) else {
        return CommentPlace::Text;
    };
    let name = &ty[word.clone()];
    let before = &ty[..word.start];
    let after = &ty[word.end..];
    // The class itself (`self`, `static`, `$this`) or its parent.
    let this = before.ends_with('$') && name == "this";
    if this || ["self", "static"].iter().any(|w| w.eq_ignore_ascii_case(name)) {
        return own_class(text, open, "self");
    }
    if name.eq_ignore_ascii_case("parent") {
        return own_class(text, open, "parent");
    }
    // A variable, a literal string, a number: text.
    let quotes = before.matches(['\'', '"']).count();
    if before.ends_with('$') || quotes % 2 == 1 || name.starts_with(|c: char| c.is_ascii_digit()) {
        return CommentPlace::Text;
    }
    // `Foo::BAR` (or `Foo::BAR_*`): the class, then its constant.
    if let Some(class) = before.strip_suffix("::") {
        if after.starts_with('*') {
            return CommentPlace::Text;
        }
        let start = class.rfind(|c: char| !is_name(c)).map_or(0, |i| i + 1);
        let class = &class[start..];
        if class.is_empty() {
            return CommentPlace::Text;
        }
        return CommentPlace::Code(Target::Static {
            class: class.to_owned(),
            name: name.to_owned(),
        });
    }
    // A shape's key: `array{id: int, user?: User}`.
    let next = after.trim_start();
    if (next.starts_with(':') && !next.starts_with("::")) || next.starts_with("?:") {
        return CommentPlace::Text;
    }
    if name.contains('-') || BUILT_IN.iter().any(|b| b.eq_ignore_ascii_case(name)) {
        return CommentPlace::Text;
    }
    if let Some(at) = declared_name(text, open, name) {
        return CommentPlace::Declared(at);
    }
    CommentPlace::Code(Target::Class(name.to_owned()))
}

/// Where a comment before `open` (or this one) declares `name`: a template
/// (`@template T`), a type alias (`@phpstan-type Name`, an imported one).
fn declared_name(text: &str, open: usize, name: &str) -> Option<usize> {
    if name.contains('\\') {
        return None;
    }
    // This comment and the ones before it (a method's templates, its class's).
    let close = text[open..].find("*/").map_or(text.len(), |i| open + i);
    let mut best = None;
    let declaring =
        ["template", "template-covariant", "template-contravariant", "type", "import-type"];
    let mut from = 0;
    while let Some(found) = text[from..close].find('@') {
        let tag_at = from + found + 1;
        from = tag_at;
        let tag_end = text[tag_at..close].find(char::is_whitespace).map_or(close, |i| tag_at + i);
        let tag = &text[tag_at..tag_end];
        let bare = ["phpstan-", "psalm-"].iter().find_map(|p| tag.strip_prefix(p)).unwrap_or(tag);
        if !declaring.contains(&bare) {
            continue;
        }
        let rest = &text[tag_end..close];
        let (Some(word), _) = first_word(rest) else { continue };
        let declared = rest[word.clone()].trim_end_matches(['=', ',']);
        if declared == name {
            best = Some(tag_end + word.start);
        }
    }
    best
}

/// `@template T of Bound = Default`, `@phpstan-type Name = Type`: the name is its own
/// declaration; after one of `separators`, a type.
fn declaring(
    text: &str,
    (base, rest): (usize, &str),
    at: usize,
    open: usize,
    separators: &[&str],
) -> CommentPlace {
    let (Some(word), end) = first_word(rest) else { return CommentPlace::Text };
    if word.contains(&at) || at == word.end {
        return CommentPlace::Declared(base + word.start);
    }
    // The type after a separator (`of`, `as`, `=`), else right after the name (`@phpstan-type
    // Name Type`).
    let mut ty_start = end;
    let tail = &rest[end..];
    let trimmed = tail.trim_start();
    for sep in separators {
        if let Some(after) = trimmed.strip_prefix(sep)
            && (sep.len() == 1 || after.starts_with(char::is_whitespace))
        {
            ty_start = rest.len() - after.len();
            break;
        }
    }
    let Some(rel) = at.checked_sub(ty_start) else { return CommentPlace::Text };
    let tail = &rest[ty_start..];
    let skipped = tail.len() - tail.trim_start().len();
    let (ty, _) = split_doc_type(tail);
    match rel.checked_sub(skipped).filter(|r| *r <= ty.len()) {
        Some(r) => in_type(text, ty, r, open),
        None => CommentPlace::Text,
    }
}

/// `@phpstan-import-type Alias from Class [as Local]`: the class is code; the name it's
/// known by here, its declaration.
fn import_type((base, rest): (usize, &str), at: usize) -> CommentPlace {
    let words: Vec<(usize, &str)> = rest
        .split_whitespace()
        .map(|w| (w.as_ptr() as usize - rest.as_ptr() as usize, w))
        .collect();
    let Some(&(i, word)) = words.iter().find(|(i, w)| (*i..*i + w.len()).contains(&at)) else {
        return CommentPlace::Text;
    };
    let index = words.iter().position(|(j, _)| *j == i).unwrap_or(0);
    match (index, words.get(index.wrapping_sub(1)).map(|w| w.1)) {
        (0, _) => CommentPlace::Declared(base + i),
        (_, Some("from")) => CommentPlace::Code(Target::Class(word.to_owned())),
        (_, Some("as")) => CommentPlace::Declared(base + i),
        _ => CommentPlace::Text,
    }
}

/// `@method [static] [Type] name(Type $a = default) description`: the types are code, the
/// name its own declaration, the rest text.
fn method(text: &str, (base, rest): (usize, &str), at: usize, open: usize) -> CommentPlace {
    let Some(paren) = rest.find('(') else {
        return in_type_or_text(text, rest, at, open);
    };
    let head = &rest[..paren];
    let name_start =
        head.trim_end().rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    let name_end = head.trim_end().len();
    if (name_start..=name_end).contains(&at) && name_start < name_end {
        return CommentPlace::Declared(base + name_start);
    }
    if at < name_start {
        // The return type (after `static`).
        let ty_part = &rest[..name_start];
        return in_type_or_text(text, ty_part, at, open);
    }
    // The parameters (to the matching `)`), then `: Type`, then the description.
    let mut depth = 0usize;
    let mut close = rest.len();
    for (i, c) in rest[paren..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = paren + i;
                    break;
                }
            }
            _ => {}
        }
    }
    if at > close {
        let tail = &rest[close + 1..];
        if let Some(ty) = tail.trim_start().strip_prefix(':') {
            let ty_start = rest.len() - ty.len();
            let skipped = ty.len() - ty.trim_start().len();
            let (ty, _) = split_doc_type(ty);
            if let Some(rel) = at.checked_sub(ty_start + skipped).filter(|r| *r <= ty.len()) {
                return in_type(text, ty, rel, open);
            }
        }
        return CommentPlace::Text;
    }
    // In a parameter: its type (before `$`), not its name or its default value.
    let params = &rest[paren + 1..close];
    let rel = at - (paren + 1);
    let start = params[..rel.min(params.len())].rfind(',').map_or(0, |i| i + 1);
    let param = &params[start..];
    let param = &param[..param.find(',').unwrap_or(param.len())];
    let dollar = param.find('$').unwrap_or(param.len());
    let inside = rel - start;
    if inside >= dollar {
        return CommentPlace::Text;
    }
    in_type_or_text(text, &param[..dollar], inside, open)
}

/// `at` in `part`: a type's name if it's in one (`static` is a modifier there).
fn in_type_or_text(text: &str, part: &str, at: usize, open: usize) -> CommentPlace {
    let skipped = part.len() - part.trim_start().len();
    let trimmed = part.trim_start();
    let (modifier, ty_rest) = match trimmed.strip_prefix("static") {
        Some(r) if r.starts_with(char::is_whitespace) => (6, r),
        _ => (0, trimmed),
    };
    let ty_skipped = skipped + modifier + (ty_rest.len() - ty_rest.trim_start().len());
    let (ty, _) = split_doc_type(ty_rest);
    match at.checked_sub(ty_skipped).filter(|r| *r <= ty.len() && !ty.is_empty()) {
        Some(rel) => in_type(text, ty, rel, open),
        None => CommentPlace::Text,
    }
}

/// `@see` / `@uses` / `@covers` / `@link` and the inline `{@see …}`: their first word names
/// a class (`Class`), a member (`Class::method()`, `Class::$prop`, `Class::CONST`) or a
/// function (`name()`); an address is a link (shown as one elsewhere), the rest text.
fn reference(rest: &str, at: usize) -> CommentPlace {
    let (Some(word), _) = first_word(rest) else { return CommentPlace::Text };
    if !(word.start..=word.end).contains(&at) {
        return CommentPlace::Text;
    }
    let written = rest[word.clone()].trim_end_matches(['.', ',', '}']);
    if written.contains("://") || written.starts_with("http") {
        return CommentPlace::Text;
    }
    let rel = at - word.start;
    match written.split_once("::") {
        Some((class, member)) => {
            if class.is_empty() {
                return CommentPlace::Text;
            }
            if rel <= class.len() {
                return CommentPlace::Code(Target::Class(class.to_owned()));
            }
            let member = member.trim_start_matches('$').trim_end_matches("()");
            if member.is_empty() {
                return CommentPlace::Text;
            }
            CommentPlace::Code(Target::Static { class: class.to_owned(), name: member.to_owned() })
        }
        None => match written.strip_suffix("()") {
            Some(function) if !function.is_empty() => {
                CommentPlace::Code(Target::Function(function.to_owned()))
            }
            Some(_) => CommentPlace::Text,
            None if written.chars().all(is_name) && !written.is_empty() => {
                CommentPlace::Code(Target::Class(written.to_owned()))
            }
            None => CommentPlace::Text,
        },
    }
}

/// A Doctrine / Symfony annotation (`@ORM\Entity`, `@Route`): a class's name, not a tag.
fn is_annotation(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase()) || name.contains('\\')
}

/// What the comment ending at `close` is on: a type (its name, the parent it extends) or a
/// function / method (its name).
enum Declaration<'a> {
    Type { name: &'a str, parent: Option<&'a str> },
    Function(&'a str),
}

fn declaration_after(text: &str, close: usize) -> Option<Declaration<'_>> {
    let modifiers = ["public", "protected", "private", "static", "abstract", "final", "readonly"];
    let mut words = text[close..]
        .split(|c: char| c.is_whitespace() || c == '(' || c == '{')
        .filter(|w| !w.is_empty());
    loop {
        match words.next()? {
            w if modifiers.contains(&w) || w.starts_with("#[") => {}
            "class" | "interface" | "trait" | "enum" => {
                let name = words.next()?;
                let parent = match words.next() {
                    Some("extends") => words.next().map(|p| p.trim_end_matches(',')),
                    _ => None,
                };
                return Some(Declaration::Type { name, parent });
            }
            "function" => {
                return Some(Declaration::Function(words.next()?.trim_start_matches('&')));
            }
            _ => return None,
        }
    }
}

/// `self` / `static` / `$this` (`word` "self") or `parent` in the comment opened at `open`:
/// on a class's own comment, its name (or the one it extends); inside its body, the word,
/// which the language resolves to the class around it.
fn own_class(text: &str, open: usize, word: &str) -> CommentPlace {
    let close = text[open..].find("*/").map_or(text.len(), |i| open + i + 2);
    match declaration_after(text, close) {
        Some(Declaration::Type { name, parent }) => match (word, parent) {
            ("parent", Some(parent)) => CommentPlace::Code(Target::Class(parent.to_owned())),
            ("parent", None) => CommentPlace::Text,
            _ => CommentPlace::Code(Target::Class(name.to_owned())),
        },
        _ => CommentPlace::Code(Target::Class(word.to_owned())),
    }
}

/// `@inheritDoc` on a method (its comment ending at `close`): the parent's method of the
/// same name.
fn inherited(text: &str, close: usize) -> CommentPlace {
    match declaration_after(text, close) {
        Some(Declaration::Function(name)) if !name.is_empty() => {
            CommentPlace::Code(Target::Static { class: "parent".to_owned(), name: name.to_owned() })
        }
        _ => CommentPlace::Text,
    }
}

/// `@covers ::method` (`rest` a slice of `text`): the method of the class the
/// `@coversDefaultClass` before it names.
fn covered(text: &str, rest: &str, at: usize) -> CommentPlace {
    let (Some(word), _) = first_word(rest) else { return CommentPlace::Text };
    if !(word.start..=word.end).contains(&at) {
        return CommentPlace::Text;
    }
    let base = rest.as_ptr() as usize - text.as_ptr() as usize;
    let Some(found) = text[..base].rfind("@coversDefaultClass") else {
        return CommentPlace::Text;
    };
    let after = &text[found + "@coversDefaultClass".len()..base];
    let (Some(class), _) = first_word(after) else { return CommentPlace::Text };
    let name = rest[word].trim_start_matches("::").trim_end_matches("()");
    CommentPlace::Code(Target::Static { class: after[class].to_owned(), name: name.to_owned() })
}

/// In an annotation's arguments (`(targetEntity="App\User", repositoryClass=Repo::class)`):
/// a class named by `X::class`, or quoted after `targetEntity` / `repositoryClass` /
/// `entityClass` / `class`; the rest text.
fn annotation_argument(rest: &str, at: usize) -> CommentPlace {
    let at = at.min(rest.len());
    let Some(word) = word_around(rest, at) else {
        return CommentPlace::Text;
    };
    let name = &rest[word.clone()];
    if rest[word.end..].starts_with("::class") {
        return CommentPlace::Code(Target::Class(name.to_owned()));
    }
    let before = &rest[..word.start];
    let quoted = before.ends_with(['"', '\'']);
    let key = before.trim_end_matches(['"', '\'']).trim_end();
    let key = key.strip_suffix('=').map_or("", str::trim_end);
    let key = &key[key.rfind(|c: char| !c.is_alphanumeric()).map_or(0, |i| i + 1)..];
    match quoted && ["targetEntity", "repositoryClass", "entityClass", "class"].contains(&key) {
        true => CommentPlace::Code(Target::Class(name.to_owned())),
        false => CommentPlace::Text,
    }
}

/// An inline tag on the line (`{@see X}`, `{@link X}`, `{@inheritDoc}`) around `offset`.
fn inline_tag(
    text: &str,
    line_start: usize,
    line_end: usize,
    offset: usize,
) -> Option<CommentPlace> {
    let before = &text[line_start..offset];
    let open = before.rfind("{@")?;
    if before[open..].contains('}') {
        return None;
    }
    let tag_start = line_start + open + 2;
    let close = text[tag_start..line_end].find('}').map_or(line_end, |i| tag_start + i);
    if offset > close {
        return None;
    }
    let inner = &text[tag_start..close];
    let name_len = inner.find(char::is_whitespace).unwrap_or(inner.len());
    if matches!(&inner[..name_len], "inheritDoc" | "inheritdoc") {
        let end = text[close..].find("*/").map_or(text.len(), |i| close + i + 2);
        return Some(inherited(text, end));
    }
    if offset < tag_start + name_len {
        return Some(CommentPlace::Text);
    }
    let name = &inner[..name_len];
    let rest_start = tag_start + name_len;
    match name {
        "see" | "link" | "uses" => Some(reference(&text[rest_start..close], offset - rest_start)),
        _ => Some(CommentPlace::Text),
    }
}

#[cfg(test)]
#[path = "../tests/unit/comment_places.rs"]
mod tests;

/// PHPDoc's parts colored as code (plan 75h): the tags, the type after a typed tag (read
/// as its other readings read it), the variable it documents.
#[derive(Debug, Default)]
pub struct DocParts;

impl syntax_ts::DocComments for DocParts {
    fn language(&self) -> languages::LanguageId {
        crate::LANGUAGE
    }

    fn nodes(&self) -> &'static [&'static str] {
        &["comment"]
    }

    fn parts(&self, comment: &str) -> Vec<(std::ops::Range<usize>, syntax_ts::DocPart)> {
        if !comments::is_doc(comment, 0) {
            return Vec::new();
        }
        let bare = |tag: &str| {
            ["phpstan-", "psalm-"]
                .iter()
                .find_map(|p| tag.strip_prefix(p))
                .unwrap_or(tag)
                .to_owned()
        };
        let style = comments::DocStyle {
            typed: &|tag| TYPED.contains(&bare(tag).as_str()),
            named: &|tag| {
                matches!(
                    bare(tag).as_str(),
                    "param" | "param-out" | "var" | "property" | "property-read" | "property-write"
                )
            },
            is_name: &|word| word.trim_start_matches(['&', '.']).starts_with('$'),
            type_len: Some(&|rest: &str| split_doc_type(rest).0.len()),
        };
        comments::doc_parts(comment, &style).into_iter().map(|(r, t)| (r, part(t))).collect()
    }
}

fn part(text: comments::DocText) -> syntax_ts::DocPart {
    match text {
        comments::DocText::Tag => syntax_ts::DocPart::Tag,
        comments::DocText::Type => syntax_ts::DocPart::Type,
        comments::DocText::Name => syntax_ts::DocPart::Variable,
    }
}
