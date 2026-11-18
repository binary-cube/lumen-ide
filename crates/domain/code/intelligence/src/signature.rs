//! Parameter help while typing a call: the call around the cursor, which argument is being
//! typed, and the callee's signature split into parameters.

use std::ops::Range;

use crate::engine::{Project, hover};
use crate::{LanguageIntelligence, SymbolIndex};

/// How far back the enclosing call is looked for (bytes); calls are rarely longer.
const WINDOW: usize = 4096;

/// The call around a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The callee's name (its last character is right before the `(`, spaces aside).
    pub name: Range<usize>,
    /// Where its `(` is.
    pub open: usize,
    /// The argument being typed (0-based: the commas before the cursor).
    pub argument: usize,
}

/// A signature split into parameters, with the one being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    pub code: String,
    /// The parameters' ranges in `code`.
    pub params: Vec<Range<usize>>,
    /// The parameter being typed (variadic parameters take the rest), if any.
    pub active: Option<usize>,
}

/// The innermost unclosed call around `offset` (strings and comments skipped), if the
/// cursor is in a call's parentheses; its name has letters, digits, `_` and the language's
/// own name characters (`name_chars`: `$` and `\\` in PHP).
pub fn enclosing_call(text: &str, offset: usize, name_chars: &str) -> Option<Call> {
    enclosing_call_in(text, offset, name_chars, false)
}

/// [`enclosing_call`] in a language whose line comments start with `--` too (`dashes`: SQL,
/// where `-- the customers' names` mustn't open a string).
pub fn enclosing_call_in(
    text: &str,
    offset: usize,
    name_chars: &str,
    dashes: bool,
) -> Option<Call> {
    let offset = offset.min(text.len());
    let mut from = offset.saturating_sub(WINDOW);
    // Start at a line start (less likely to be inside a string or comment).
    if from > 0 {
        from = text[..from].rfind('\n').map_or(0, |i| i + 1);
    }
    let bytes = text.as_bytes();
    // Open brackets: (byte, position, commas at its level).
    let mut stack: Vec<(u8, usize, usize)> = Vec::new();
    let mut i = from;
    while i < offset {
        let b = bytes[i];
        match b {
            b'\'' | b'"' | b'`' => {
                i += 1;
                while i < offset && bytes[i] != b {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = text[i..offset].find('\n').map_or(offset, |n| i + n);
            }
            b'-' if dashes && bytes.get(i + 1) == Some(&b'-') => {
                i = text[i..offset].find('\n').map_or(offset, |n| i + n);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..offset].find("*/").map_or(offset, |n| i + 2 + n + 1);
            }
            b'(' | b'[' | b'{' => stack.push((b, i, 0)),
            b')' | b']' | b'}' => {
                stack.pop();
            }
            b',' => {
                if let Some(top) = stack.last_mut() {
                    top.2 += 1;
                }
            }
            // A statement ends: no call spans it (PHP / JS / CSS).
            b';' => {
                while stack.last().is_some_and(|t| t.0 != b'{') {
                    stack.pop();
                }
            }
            _ => {}
        }
        i += 1;
    }
    let &(b'(', open, argument) = stack.last()? else { return None };
    let before = text[..open].trim_end();
    let end = before.len();
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || name_chars.contains(*c))
        .last()
        .map(|(i, _)| i)?;
    Some(Call { name: start..end, open, argument })
}

/// The parameters of `code` (`name(a, b = [1, 2]): type`): their ranges, without spaces.
pub fn parameters(code: &str) -> Option<Vec<Range<usize>>> {
    let open = code.find('(')?;
    let bytes = code.as_bytes();
    let mut params = Vec::new();
    let mut depth = 0;
    let mut start = open + 1;
    let mut i = open + 1;
    let push = |params: &mut Vec<Range<usize>>, s: usize, e: usize| {
        let part = &code[s..e];
        let lead = part.len() - part.trim_start().len();
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            params.push(s + lead..s + lead + trimmed.len());
        }
    };
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'\'' | b'"') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            // An arrow (`=>`, TypeScript function types) isn't a bracket.
            b'>' if bytes[i - 1] == b'=' => {}
            b')' | b']' | b'}' | b'>' if depth > 0 => depth -= 1,
            b')' => {
                push(&mut params, start, i);
                return Some(params);
            }
            b',' if depth == 0 => {
                push(&mut params, start, i);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The parameter help at `offset`: the enclosing call's signature (a project function or
/// method, or one the language provides), with the argument being typed.
pub fn signature_help(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &std::path::Path,
    text: &str,
    offset: usize,
    project: &Project<'_>,
) -> Option<SignatureHelp> {
    let call = enclosing_call(text, offset, intelligence.word_chars())?;
    // `hover` looks the name up as a call (it is followed by `(`).
    let mut info = hover(intelligence, index, path, text, call.name.end - 1, project)?;
    // `new X(`: the card's class line above the constructor's signature isn't the call's.
    if let Some((head, rest)) = info.code.split_once('\n')
        && !head.contains('(')
        && rest.contains('(')
    {
        info.code = rest.to_owned();
    }
    let params = parameters(&info.code)?;
    let named = named_parameter(intelligence, text, &call, offset, &info.code, &params);
    Some(SignatureHelp {
        active: named.or_else(|| active_parameter(&info.code, &params, call.argument)),
        code: info.code,
        params,
    })
}

/// A parameter's name as a named argument writes it: without the sigil, a variadic's `...`
/// or a reference's `&` (`$name` → `name`); `None` for a variadic (it can't be named).
fn argument_name(intelligence: &dyn LanguageIntelligence, param: &str) -> Option<String> {
    let (_, name) = intelligence.split_param(param);
    let name = name.trim().trim_start_matches('&');
    if name.starts_with("...") {
        return None;
    }
    let sigil = intelligence.naming().sigil;
    let name = sigil.and_then(|s| name.strip_prefix(s)).unwrap_or(name);
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .then(|| name.to_owned())
}

/// The named arguments written so far in the call's parentheses (`name:`, not `A::B`), at
/// their own level (not in a nested call or array).
fn named_so_far(text: &str, open: usize, offset: usize) -> Vec<String> {
    let args = &text[open + 1..offset.max(open + 1)];
    let mut depth = 0usize;
    let mut at_start = true;
    let mut out = Vec::new();
    let mut quote = None;
    let mut chars = args.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        // Inside a string nothing counts (`f('a, b: c', |`).
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                at_start = false;
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => at_start = true,
            c if c.is_whitespace() => {}
            c if at_start && depth == 0 && (c.is_alphabetic() || c == '_') => {
                at_start = false;
                let end = args[i..]
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .map_or(args.len(), |e| i + e);
                let rest = args[end..].trim_start();
                if rest.starts_with(':') && !rest.starts_with("::") {
                    out.push(args[i..end].to_owned());
                }
                while chars.peek().is_some_and(|&(j, _)| j < end) {
                    chars.next();
                }
            }
            _ => at_start = false,
        }
    }
    out
}

/// A callee's card kept: the file, the text up to its name (its length, its hash), the
/// index's version, its code.
type Callee = (std::path::PathBuf, usize, u64, u64, Option<String>);

thread_local! {
    /// The last callee's card: the file, the text up to the callee's name (its length and
    /// hash), the index's version it was found in, and its code.
    static CALLEE: std::cell::RefCell<Option<Callee>> =
        const { std::cell::RefCell::new(None) };
}

/// The code of the declaration the call whose name ends at `name_end` goes to: found by a
/// hover, kept while the text up to the name, the file and the index stay the same.
fn callee_code(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &std::path::Path,
    text: &str,
    name_end: usize,
    project: &Project<'_>,
) -> Option<String> {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text[..name_end].hash(&mut h);
    let (hash, of) = (h.finish(), index.version());
    let kept = CALLEE.with_borrow(|c| {
        c.as_ref()
            .filter(|(p, end, h, i, _)| p == path && *end == name_end && *h == hash && *i == of)
            .map(|(.., code)| code.clone())
    });
    if let Some(code) = kept {
        return code;
    }
    let code = hover(intelligence, index, path, text, name_end - 1, project).map(|info| info.code);
    CALLEE.with_borrow_mut(|c| *c = Some((path.to_path_buf(), name_end, hash, of, code.clone())));
    code
}

/// The named arguments to offer where an argument starts (`f(|`, `f(a, na|`): the
/// callee's parameters not yet named, in their order (with their type), as `name: `.
pub fn named_arguments(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &std::path::Path,
    text: &str,
    replace: Range<usize>,
    project: &Project<'_>,
) -> Vec<crate::CompletionItem> {
    let Some(separator) = intelligence.named_argument() else { return Vec::new() };
    let before = text[..replace.start].trim_end();
    if !(before.ends_with('(') || before.ends_with(',')) {
        return Vec::new();
    }
    let Some(call) = enclosing_call(text, replace.start, intelligence.word_chars()) else {
        return Vec::new();
    };
    // A declaration's own parameters (`function save(User $user, |`) take no arguments.
    let head = text[..call.name.start].trim_end();
    let declared = intelligence
        .source_rules()
        .is_some_and(|rules| rules.declares_function.iter().any(|w| head.ends_with(w)));
    if declared {
        return Vec::new();
    }
    // The callee's card found once for its call (each key typed in its first argument asks
    // again: a whole hover, its file maybe read from disk), kept for the text up to its name.
    let Some(code) = callee_code(intelligence, index, path, text, call.name.end, project) else {
        return Vec::new();
    };
    let Some(params) = parameters(&code) else { return Vec::new() };
    let written = named_so_far(text, call.open, replace.start);
    let typed = &text[replace];
    params
        .iter()
        .enumerate()
        .filter_map(|(i, range)| {
            let param = &code[range.clone()];
            let name = argument_name(intelligence, param)?;
            if written.contains(&name) {
                return None;
            }
            let score = crate::fuzzy_score(typed, &name)?;
            let (ty, _) = intelligence.split_param(param);
            Some(crate::CompletionItem {
                label: format!("{name}{separator}"),
                kind: crate::CompletionKind::Variable,
                detail: ty,
                insert: format!("{name}{separator} "),
                // Above the other names, in the signature's order.
                score: score + 1000 - i as i32,
                ..crate::CompletionItem::default()
            })
        })
        .collect()
}

/// The parameter a named argument being written goes to (`f(b: |`): `None` when the
/// argument isn't named (then it's by position).
fn named_parameter(
    intelligence: &dyn LanguageIntelligence,
    text: &str,
    call: &Call,
    offset: usize,
    code: &str,
    params: &[Range<usize>],
) -> Option<usize> {
    intelligence.named_argument()?;
    let written = named_so_far(text, call.open, offset);
    // The argument at the cursor: what's after the last `,` at the call's level.
    let args = &text[call.open + 1..offset];
    let mut depth = 0usize;
    let mut last = 0;
    let mut quote = None;
    let mut chars = args.char_indices();
    while let Some((i, c)) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => last = i + 1,
            _ => {}
        }
    }
    let current = args[last..].trim_start();
    // Its whole name then `:` (`userId:` isn't `user`).
    let name = written.last().filter(|n| {
        current.strip_prefix(n.as_str()).is_some_and(|rest| {
            let rest = rest.trim_start();
            rest.starts_with(':') && !rest.starts_with("::")
        })
    })?;
    params.iter().position(|r| argument_name(intelligence, &code[r.clone()]).as_ref() == Some(name))
}

/// The parameter that argument `argument` goes to (a variadic one takes all the rest).
pub fn active_parameter(code: &str, params: &[Range<usize>], argument: usize) -> Option<usize> {
    if argument < params.len() {
        return Some(argument);
    }
    let last = params.last()?;
    code[last.clone()].contains("...").then(|| params.len() - 1)
}

/// A signature with two or more parameters (or a long one), one parameter per line:
/// `f(\n    A $a,\n    B $b\n): R`. A code already on several lines is kept as it is.
pub fn multiline_signature(code: &str) -> String {
    // Already laid out on its lines by who wrote it.
    if code.contains('\n') {
        return code.to_owned();
    }
    let Some(params) = parameters(code).filter(|p| !p.is_empty()) else {
        return code.to_owned();
    };
    if params.len() < 2 && code.len() <= 70 {
        return code.to_owned();
    }
    let (Some(first), Some(last)) = (params.first(), params.last()) else {
        return code.to_owned();
    };
    let open = code[..first.start].rfind('(').unwrap_or(first.start);
    let close = code[last.end..].find(')').map_or(code.len(), |i| last.end + i);
    let mut out = code[..=open].to_owned();
    for (i, p) in params.iter().enumerate() {
        out.push_str("\n    ");
        out.push_str(&code[p.clone()]);
        if i + 1 < params.len() {
            out.push(',');
        }
    }
    out.push('\n');
    out.push_str(&code[close..]);
    out
}

#[cfg(test)]
#[path = "../tests/unit/signature.rs"]
mod tests;
