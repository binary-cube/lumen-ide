//! Inlay hints: what the editor shows inside the code without it being there — a
//! parameter's name before an argument whose meaning isn't plain from it (`total(a: 1,
//! b: 2)`), from the callee's signature in the index or the language's own
//! functions (PHP's stubs). Only the calls in a range are looked at (the visible lines).

use std::ops::Range;
use std::path::Path;

use crate::engine::{Project, definition};
use crate::{LanguageIntelligence, SymbolIndex, SymbolKind, parameters};

/// What a hint is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InlayKind {
    /// A parameter's name, before its argument.
    #[default]
    Parameter,
    /// A type, after a name.
    Type,
}

/// A hint: where it's shown (before the byte at `offset`) and what it says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InlayHint {
    pub offset: usize,
    pub label: String,
    pub kind: InlayKind,
}

/// The words before `(` that aren't calls.
const NOT_CALLS: &[&str] = &[
    "if",
    "elseif",
    "else",
    "while",
    "for",
    "foreach",
    "switch",
    "catch",
    "match",
    "function",
    "fn",
    "array",
    "list",
    "isset",
    "empty",
    "unset",
    "echo",
    "print",
    "return",
    "declare",
    "use",
    "require",
    "require_once",
    "include",
    "include_once",
    "exit",
    "die",
    "eval",
    "new",
    "typeof",
    "await",
    "yield",
    "with",
    "sizeof",
    "constructor",
];

/// At most this many calls are looked at in one range (a minified line stays cheap).
const MAX_CALLS: usize = 400;

/// The parameter names to show at the calls whose `(` is in `range` of `text` (the file at
/// `path`): before each literal argument (a number, a string, `true` / `false` / `null`, an
/// array), not where the argument is named or the callee's name ends with the parameter's
/// (`setTotal(5)`).
pub fn parameter_hints(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    range: Range<usize>,
    project: &Project<'_>,
) -> Vec<InlayHint> {
    let mut out = Vec::new();
    let name_chars = intelligence.word_chars();
    for call in calls(text, range, name_chars).into_iter().take(MAX_CALLS) {
        let Some(params) = callee_parameters(intelligence, index, path, text, &call, project)
        else {
            continue;
        };
        let callee = text[call.name.clone()].to_ascii_lowercase();
        for (i, argument) in call.arguments.iter().enumerate() {
            let Some(param) = params.get(i).or_else(|| params.last().filter(|p| p.1)) else {
                break;
            };
            let (name, variadic) = (param.0.as_str(), param.1);
            if variadic && i >= params.len() {
                break;
            }
            let written = text[argument.clone()].trim();
            if !is_literal(written) || named(written) {
                continue;
            }
            if i == 0 && callee.ends_with(&name.to_ascii_lowercase()) {
                continue;
            }
            let label = if variadic { format!("...{name}:") } else { format!("{name}:") };
            out.push(InlayHint { offset: argument.start, label, kind: InlayKind::Parameter });
        }
    }
    out
}

/// A call: its callee's name and its arguments (trimmed ranges).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CallSite {
    name: Range<usize>,
    arguments: Vec<Range<usize>>,
}

/// The calls whose `(` is in `range` (strings and comments skipped).
fn calls(text: &str, range: Range<usize>, name_chars: &str) -> Vec<CallSite> {
    let bytes = text.as_bytes();
    let end = range.end.min(text.len());
    // From the range's line start: less likely to begin in a string.
    let mut i = text[..range.start.min(text.len())].rfind('\n').map_or(0, |n| n + 1);
    let mut out = Vec::new();
    while i < end {
        match bytes[i] {
            b'\'' | b'"' | b'`' => i = skip_string(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = text[i..].find('\n').map_or(text.len(), |n| i + n);
            }
            b'#' if bytes.get(i + 1) != Some(&b'[') => {
                i = text[i..].find('\n').map_or(text.len(), |n| i + n);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(text.len(), |n| i + 2 + n + 2);
            }
            b'(' if i >= range.start => {
                if let Some(call) = call_at(text, i, name_chars) {
                    out.push(call);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Past the string starting at `i` (its closing quote), escapes skipped.
fn skip_string(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() && bytes[j] != quote {
        j += if bytes[j] == b'\\' { 2 } else { 1 };
    }
    j + 1
}

/// The call whose `(` is at `open`: a name right before it (not a keyword, not a
/// declaration's), and its arguments up to the matching `)`.
fn call_at(text: &str, open: usize, name_chars: &str) -> Option<CallSite> {
    let before = text[..open].trim_end();
    let name_end = before.len();
    let name_start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || name_chars.contains(*c))
        .last()
        .map(|(i, _)| i)?;
    let name = &text[name_start..name_end];
    let plain = name.trim_start_matches(['$', '\\']);
    if plain.is_empty()
        || name.starts_with('$')
        || plain.chars().next().is_some_and(|c| c.is_ascii_digit())
        || NOT_CALLS.iter().any(|k| k.eq_ignore_ascii_case(plain))
    {
        return None;
    }
    // A declaration's parameters (`function total(`), not a call's arguments.
    let head = text[..name_start].trim_end();
    let word_before =
        head.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
    if ["function", "fn", "def", "func"].contains(&word_before.to_ascii_lowercase().as_str()) {
        return None;
    }
    let arguments = arguments(text, open)?;
    Some(CallSite { name: name_start..name_end, arguments })
}

/// The arguments between `(` at `open` and its `)` (`None` if it isn't closed nearby).
fn arguments(text: &str, open: usize) -> Option<Vec<Range<usize>>> {
    const LIMIT: usize = 4096;
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut start = open + 1;
    let mut out = Vec::new();
    let push = |out: &mut Vec<Range<usize>>, s: usize, e: usize| {
        let part = &text[s..e];
        let lead = part.len() - part.trim_start().len();
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            out.push(s + lead..s + lead + trimmed.len());
        }
    };
    let mut i = open + 1;
    while i < bytes.len() && i < open + LIMIT {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth > 0 => depth -= 1,
            b')' => {
                push(&mut out, start, i);
                return Some(out);
            }
            b',' if depth == 0 => {
                push(&mut out, start, i);
                start = i + 1;
            }
            b';' if depth == 0 => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// The callee's parameters (each one's name as an argument names it, and whether it's
/// variadic): from its declaration in the index, else what its language provides.
fn callee_parameters(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    call: &CallSite,
    project: &Project<'_>,
) -> Option<Vec<(String, bool)>> {
    let at = call.name.end - 1;
    let declared =
        definition(intelligence, index, path, text, at, project).into_iter().find_map(|l| {
            let file = index.file(&l.path)?;
            file.symbols.iter().find(|s| s.offset == l.offset).cloned()
        });
    let naming = intelligence.naming();
    let code = match declared {
        Some(s) if matches!(s.kind, SymbolKind::Method | SymbolKind::Function) => s.detail?,
        // `new X(`: its constructor's.
        Some(s) if s.kind.is_type() => {
            let ctor = index
                .members_of(&s.fqn)
                .into_iter()
                .find(|(_, m)| naming.is_constructor(&m.name))?;
            ctor.1.detail.clone()?
        }
        Some(_) => return None,
        None => (intelligence.hover(text, at)?.code).into_boxed_str(),
    };
    let ranges = parameters(&code)?;
    let sigil = naming.sigil;
    let params = ranges
        .into_iter()
        .filter_map(|r| {
            let (_, name) = intelligence.split_param(&code[r]);
            let name = name.trim().trim_start_matches('&');
            let variadic = name.starts_with("...");
            let name = name.trim_start_matches("...");
            let name = sigil.and_then(|s| name.strip_prefix(s)).unwrap_or(name);
            let name = name.split(['=', ':', ' ']).next().unwrap_or(name).trim();
            (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .then(|| (name.to_owned(), variadic))
        })
        .collect();
    Some(params)
}

/// An argument whose meaning isn't plain from it: a number, a string, a constant of the
/// language (`true`, `false`, `null`), an array written out.
fn is_literal(argument: &str) -> bool {
    let a = argument.trim_start_matches(['-', '+']);
    let Some(first) = a.chars().next() else { return false };
    first.is_ascii_digit()
        || matches!(first, '\'' | '"' | '`' | '[')
        || ["true", "false", "null", "undefined", "nullptr"]
            .iter()
            .any(|k| a.eq_ignore_ascii_case(k))
        || a.to_ascii_lowercase().starts_with("array(")
}

/// A named argument (`name: 1`), already saying its parameter.
fn named(argument: &str) -> bool {
    let end = argument.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(0);
    end > 0
        && argument[end..].trim_start().starts_with(':')
        && !argument[end..].trim_start().starts_with("::")
}

#[cfg(test)]
#[path = "../tests/unit/inlay.rs"]
mod tests;
