//! PHP doc blocks (`/** … */`): completion of tags (`@param`, `@return`…),
//! types and documented variables, plus generating the block from the signature
//! of the declaration below (`/**` + Enter).

use code_intelligence::{CompletionContext, CompletionItem, CompletionKind, ContextKind};

pub const TAG: &str = "php-doc-tag";
pub const PARAM: &str = "php-doc-param";

/// The tags; their descriptions are in the locales (`phpdoc.<tag>`).
const TAGS: &[&str] = &[
    "@param",
    "@return",
    "@var",
    "@throws",
    "@deprecated",
    "@see",
    "@since",
    "@author",
    "@inheritDoc",
    "@property",
    "@property-read",
    "@property-write",
    "@method",
    "@template",
    "@extends",
    "@implements",
    "@internal",
    "@todo",
    "@link",
    "@mixin",
    "@psalm-param",
    "@phpstan-param",
    "@phpstan-return",
];

/// Scalar types and pseudo-types (including those of static analyzers).
const TYPES: &[&str] = &[
    "string",
    "int",
    "float",
    "bool",
    "array",
    "mixed",
    "void",
    "null",
    "callable",
    "iterable",
    "object",
    "never",
    "self",
    "static",
    "true",
    "false",
    "list",
    "array-key",
    "class-string",
    "non-empty-string",
    "non-empty-array",
    "positive-int",
    "resource",
];

/// Tags followed by a type.
const TYPED: &[&str] = &[
    "param",
    "return",
    "var",
    "throws",
    "property",
    "property-read",
    "property-write",
    "method",
    "psalm-param",
    "phpstan-param",
    "phpstan-return",
    "mixin",
    "extends",
    "implements",
];

/// Start (`/**`) of the doc block open at `offset`, if any.
pub fn open_at(text: &str, offset: usize) -> Option<usize> {
    let before = text.get(..offset)?;
    let open = before.rfind("/**")?;
    (!before[open..].contains("*/")).then_some(open)
}

fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Completion context inside a doc block (`None` outside it).
pub fn context(text: &str, offset: usize) -> Option<CompletionContext> {
    open_at(text, offset)?;
    let before = &text[..offset];
    let line = &before[before.rfind('\n').map_or(0, |i| i + 1)..];
    let line_start = offset - line.len();
    let none = || CompletionContext {
        replace: offset..offset,
        prefix: String::new(),
        kind: ContextKind::None,
    };
    // `@ta|`: tags.
    let word = line.len() - line.trim_end_matches(is_tag_char).len();
    let word_start = line.len() - word;
    if line[..word_start].ends_with('@') {
        let start = line_start + word_start - 1;
        return Some(CompletionContext {
            replace: start..offset,
            prefix: text[start..offset].to_owned(),
            kind: ContextKind::Vocabulary(TAG),
        });
    }
    // `@tag …`: what follows the tag.
    let Some(at) = line.rfind('@') else { return Some(none()) };
    let after_tag = &line[at + 1..];
    let tag_len = after_tag.len() - after_tag.trim_start_matches(is_tag_char).len();
    let (tag, rest) = after_tag.split_at(tag_len);
    if !TYPED.contains(&tag) || !rest.starts_with(char::is_whitespace) {
        return Some(none());
    }
    let words: Vec<&str> = rest.split_whitespace().collect();
    let typing_word = !rest.ends_with(char::is_whitespace);
    // How many complete words precede the one being typed.
    let done = if typing_word { words.len().saturating_sub(1) } else { words.len() };
    let current = if typing_word { words.last().copied().unwrap_or("") } else { "" };
    let start = offset - current.len();
    let is_param = matches!(tag, "param" | "psalm-param" | "phpstan-param");
    // `@param Type $na|` or `@param $na|`: the function's variables.
    if is_param && (current.starts_with('$') || (done == 1 && current.is_empty())) {
        return Some(CompletionContext {
            replace: start..offset,
            prefix: current.to_owned(),
            kind: ContextKind::Vocabulary(PARAM),
        });
    }
    // First word: the type (project classes + scalar types). Only the part after
    // the last `|`, `&`, `?`, `<`, `(` or `,` (e.g. `string|Us|`).
    if done == 0 {
        let part = current.rsplit(['|', '&', '?', '<', '(', ',']).next().unwrap_or("");
        return Some(CompletionContext {
            replace: offset - part.len()..offset,
            prefix: part.to_owned(),
            kind: ContextKind::TypeName,
        });
    }
    Some(none())
}

/// Suggestions for the contexts above.
pub fn vocabulary(context: &CompletionContext, text: &str) -> Vec<CompletionItem> {
    match context.kind {
        ContextKind::Vocabulary(TAG) => TAGS
            .iter()
            .map(|tag| {
                let detail = i18n::t(&format!("phpdoc.{}", tag[1..].replace('-', "_")));
                let mut item = CompletionItem::word(tag, CompletionKind::Keyword, Some(&detail));
                if TYPED.contains(&&tag[1..]) {
                    item.insert = format!("{tag} ");
                }
                item
            })
            .collect(),
        ContextKind::Vocabulary(PARAM) => {
            let offset = context.replace.start;
            let block = open_at(text, offset).map_or("", |o| &text[o..offset]);
            parameters(text, offset)
                .into_iter()
                .filter(|(name, _)| !documented(block, name))
                .enumerate()
                .map(|(i, (name, ty))| {
                    let label = format!("${name}");
                    let ty = ty.as_deref().map(doc_type);
                    let mut item =
                        CompletionItem::word(&label, CompletionKind::Variable, ty.as_deref());
                    // In signature order.
                    item.score = 20 - i.min(20) as i32;
                    item
                })
                .collect()
        }
        ContextKind::TypeName => TYPES
            .iter()
            .map(|t| CompletionItem::word(t, CompletionKind::Keyword, Some("tip")))
            .collect(),
        _ => Vec::new(),
    }
}

/// `$name` already appears in the block (followed by something other than a letter).
fn documented(block: &str, name: &str) -> bool {
    let needle = format!("${name}");
    block.match_indices(&needle).any(|(i, _)| {
        !block[i + needle.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    })
}

/// A documentable declaration.
enum Declaration {
    Function { params: Vec<(String, Option<String>)>, returns: Option<String>, constructor: bool },
    Property(Option<String>),
}

const MODIFIERS: &[&str] =
    &["public", "protected", "private", "static", "readonly", "final", "abstract", "var", "const"];

/// The declaration after the doc block containing `offset`.
fn declaration(text: &str, offset: usize) -> Option<Declaration> {
    let end = offset + text.get(offset..)?.find("*/")? + 2;
    let code = text[end..].trim_start();
    let function = code.find("function").filter(|&i| {
        // `function` must be in the immediately following declaration (only modifiers before).
        code[..i].split_whitespace().all(|w| MODIFIERS.contains(&w))
    });
    if let Some(i) = function {
        let after = &code[i + "function".len()..];
        let open = after.find('(')?;
        let name = after[..open].trim().trim_start_matches('&').trim();
        let close = matching_paren(after, open)?;
        let params = split_top_level(&after[open + 1..close])
            .into_iter()
            .filter_map(|p| parameter(&p))
            .collect();
        let tail = &after[close + 1..];
        let returns = tail
            .trim_start()
            .strip_prefix(':')
            .map(|r| r.trim_start())
            .map(|r| r.split(['{', ';']).next().unwrap_or("").trim().to_owned())
            .filter(|r| !r.is_empty());
        return Some(Declaration::Function {
            params,
            returns,
            constructor: name.eq_ignore_ascii_case("__construct"),
        });
    }
    // A property: `public ?Foo $x = …;`.
    let statement = code.split(';').next()?;
    let words: Vec<&str> = statement.split('=').next()?.split_whitespace().collect();
    let dollar = words.iter().position(|w| w.starts_with('$'))?;
    if dollar == 0 || !words[..dollar].iter().any(|w| MODIFIERS.contains(w)) {
        return None;
    }
    let ty = words[..dollar].iter().rev().find(|w| !MODIFIERS.contains(w)).map(|t| (*t).to_owned());
    Some(Declaration::Property(ty))
}

/// Parameters of the function after the block at `offset`: (name without `$`, type).
pub fn parameters(text: &str, offset: usize) -> Vec<(String, Option<String>)> {
    match declaration(text, offset) {
        Some(Declaration::Function { params, .. }) => params,
        _ => Vec::new(),
    }
}

fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
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

fn split_top_level(s: &str) -> Vec<String> {
    let (mut parts, mut current, mut depth) = (Vec::new(), String::new(), 0);
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    parts.push(current);
    parts.into_iter().filter(|p| !p.trim().is_empty()).collect()
}

/// `public ?string &...$name = 'x'` → (`name`, `?string`); `#[…]` attributes are ignored.
fn parameter(p: &str) -> Option<(String, Option<String>)> {
    let p = p.split('=').next()?.trim();
    let p = match p.rfind(']') {
        Some(i) if p.trim_start().starts_with("#[") => &p[i + 1..],
        _ => p,
    };
    let words: Vec<&str> = p.split_whitespace().collect();
    let dollar = words.iter().position(|w| w.trim_start_matches(['&', '.']).starts_with('$'))?;
    let name = words[dollar].trim_start_matches(['&', '.', '$']).to_owned();
    let ty = words[..dollar]
        .iter()
        .rev()
        .find(|w| !MODIFIERS.contains(w))
        .map(|t| t.trim_end_matches(['&', '.']).to_owned())
        .filter(|t| !t.is_empty());
    let variadic =
        words[dollar].contains("...") || words[..dollar].last().is_some_and(|w| w.ends_with("..."));
    let ty = if variadic { Some(format!("{}[]", ty.as_deref().unwrap_or("mixed"))) } else { ty };
    Some((name, ty))
}

/// `?Foo` → `Foo|null` (the usual form in documentation).
fn doc_type(ty: &str) -> String {
    match ty.strip_prefix('?') {
        Some(inner) => format!("{inner}|null"),
        None => ty.to_owned(),
    }
}

/// Lines of the generated block for the declaration after `offset`.
pub fn stub(text: &str, offset: usize) -> Vec<String> {
    match declaration(text, offset) {
        Some(Declaration::Function { params, returns, constructor }) => {
            let types: Vec<String> =
                params.iter().map(|(_, t)| t.as_deref().map_or("mixed".into(), doc_type)).collect();
            let width = types.iter().map(String::len).max().unwrap_or(0);
            let mut lines: Vec<String> = params
                .iter()
                .zip(&types)
                .map(|((name, _), ty)| format!("@param {ty:width$} ${name}"))
                .collect();
            if let Some(r) = returns.filter(|r| !constructor && r != "void" && r != "never") {
                lines.push(format!("@return {}", doc_type(&r)));
            }
            lines
        }
        Some(Declaration::Property(Some(ty))) => vec![format!("@var {}", doc_type(&ty))],
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/docblock.rs"]
mod tests;
