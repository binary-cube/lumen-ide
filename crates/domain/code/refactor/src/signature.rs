//! Change Signature: a function's (method's, constructor's) parameters edited — added,
//! removed, reordered, renamed, typed, given defaults — and every call rewritten to match
//! (arguments moved, dropped, the new ones given the value chosen for the calls).
//!
//! Pure: the lists are split with C-like rules (brackets, strings, comments), which PHP
//! and JS share; how a parameter is written comes through [`SignatureSyntax`].

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::{Resolution, Resolver, Sources};
use crate::{Certainty, FilePlan, NameStyle, Occurrence, Plan, find_word, line_of};

/// A parameter as declared.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Param {
    /// Written before the type (`private readonly`, `&`, `...` go in `prefix` / `suffix`
    /// as the language's syntax decides).
    pub prefix: String,
    pub type_hint: String,
    /// Without a PHP variable's `$` (the syntax adds it).
    pub name: String,
    /// The default value's text (empty: none).
    pub default: String,
    /// Takes the remaining arguments (`...$rest`).
    pub variadic: bool,
    /// Passed by reference (`&$out`).
    pub by_ref: bool,
}

/// A parameter of the new signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewParam {
    /// An existing parameter (`from`: its old position), maybe renamed / retyped.
    Kept { from: usize, param: Param },
    /// A new one; `value` is what the existing calls pass for it (empty: its default).
    Added { param: Param, value: String },
}

impl NewParam {
    pub fn param(&self) -> &Param {
        match self {
            Self::Kept { param, .. } | Self::Added { param, .. } => param,
        }
    }

    pub fn param_mut(&mut self) -> &mut Param {
        match self {
            Self::Kept { param, .. } | Self::Added { param, .. } => param,
        }
    }
}

/// How a language writes calls (what tells a call of the changed function apart).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallRules {
    /// Before a method's name (`->`, `::`, `.`).
    pub member_access: &'static [&'static str],
    /// Words before a function's name where it's declared, not called (`function`).
    pub declares: &'static [&'static str],
    /// Before the name of a type being constructed (`new`).
    pub constructs: Option<&'static str>,
    /// What a variable's name starts with (`$name(` calls a variable, not the function).
    pub sigil: Option<char>,
}

/// How a language writes and reads a parameter, and its calls.
pub trait SignatureSyntax: std::fmt::Debug {
    /// How it writes calls (none known: no call is found).
    fn calls(&self) -> CallRules {
        CallRules::default()
    }
    /// The files whose declarations it reads (their extensions).
    fn extensions(&self) -> &'static [&'static str];
    fn parse(&self, text: &str) -> Param;
    fn render(&self, param: &Param) -> String;
    /// A named argument's name and value (`limit: 10` in PHP 8), if `arg` is one.
    fn named_argument<'a>(&self, _arg: &'a str) -> Option<(&'a str, &'a str)> {
        None
    }
    /// Writes a named argument (when an argument can't be positional).
    fn render_named(&self, _name: &str, value: &str) -> Option<String> {
        let _ = value;
        None
    }
}

/// The split at the first `=` outside brackets and strings (not `=>`, `==`).
pub fn split_default(text: &str) -> (&str, &str) {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut quote = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (quote, b) {
            (Some(q), _) if b == q && bytes.get(i.wrapping_sub(1)) != Some(&b'\\') => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'(' | b'[' | b'{') => depth += 1,
            (None, b')' | b']' | b'}') => depth -= 1,
            (None, b'=')
                if depth == 0
                    && bytes.get(i + 1) != Some(&b'>')
                    && bytes.get(i + 1) != Some(&b'=')
                    && !matches!(bytes.get(i.wrapping_sub(1)), Some(b'=' | b'!' | b'<' | b'>')) =>
            {
                return (text[..i].trim(), text[i + 1..].trim());
            }
            _ => {}
        }
    }
    (text.trim(), "")
}

/// The bracketed list starting at `open` (a `(`): its inside and its items' ranges
/// (trimmed; none for `()`), in `text`'s offsets.
pub fn split_list(text: &str, open: usize) -> Option<(Range<usize>, Vec<Range<usize>>)> {
    let bytes = text.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut items = Vec::new();
    let mut start = open + 1;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    push_item(text, start..i, &mut items);
                    return Some((open + 1..i, items));
                }
            }
            b',' if depth == 1 => {
                push_item(text, start..i, &mut items);
                start = i + 1;
            }
            quote @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(bytes.len(), |e| i + 2 + e + 1);
            }
            b'/' | b'#' if bytes[i] == b'#' || bytes.get(i + 1) == Some(&b'/') => {
                i = text[i..].find('\n').map_or(bytes.len(), |e| i + e);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn push_item(text: &str, range: Range<usize>, items: &mut Vec<Range<usize>>) {
    let raw = &text[range.clone()];
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let start = range.start + (raw.len() - raw.trim_start().len());
    items.push(start..start + trimmed.len());
}

/// A call's new arguments: the old ones (`args`, as written) moved to their new places,
/// the removed ones dropped, the added ones given their value. A missing argument before a
/// given one gets the parameter's default (or is passed by name, when the language can).
pub fn rewrite_args(
    args: &[&str],
    old: &[Param],
    new: &[NewParam],
    syntax: &dyn SignatureSyntax,
) -> String {
    // Positional arguments first, then named ones (PHP 8).
    let mut positional = Vec::new();
    let mut named = Vec::new();
    for arg in args {
        match syntax.named_argument(arg) {
            Some((name, value)) => named.push((name, value)),
            None if named.is_empty() => positional.push(*arg),
            None => positional.push(*arg),
        }
    }
    let variadic_extra: Vec<&str> = match old.last() {
        Some(last) if last.variadic && positional.len() > old.len() => {
            positional[old.len()..].to_vec()
        }
        _ => Vec::new(),
    };
    let old_value = |i: usize| -> Option<String> {
        positional
            .get(i)
            .map(|v| (*v).to_owned())
            .or_else(|| named.iter().find(|(n, _)| *n == old[i].name).map(|(_, v)| (*v).to_owned()))
    };
    // Each new parameter: its argument, if the call passes one.
    let values: Vec<Option<String>> = new
        .iter()
        .map(|p| match p {
            NewParam::Kept { from, .. } => old_value(*from),
            NewParam::Added { value, .. } if !value.trim().is_empty() => {
                Some(value.trim().to_owned())
            }
            NewParam::Added { .. } => None,
        })
        .collect();
    let last = values.iter().rposition(Option::is_some);
    let mut out: Vec<String> = Vec::new();
    let mut by_name = false;
    for (i, value) in values.iter().enumerate().take(last.map_or(0, |l| l + 1)) {
        let param = new[i].param();
        match value {
            Some(value) if by_name => {
                out.push(syntax.render_named(&param.name, value).unwrap_or_else(|| value.clone()));
            }
            Some(value) => out.push(value.clone()),
            None => {
                // A gap: the default it had (kept) or has (added), or the rest by name.
                let default = match &new[i] {
                    NewParam::Kept { from, .. } => old[*from].default.clone(),
                    NewParam::Added { param, .. } => param.default.clone(),
                };
                if !default.is_empty() {
                    out.push(default);
                } else if syntax.render_named("x", "y").is_some() {
                    by_name = true;
                } else {
                    out.push("null".to_owned());
                }
            }
        }
    }
    // The variadic parameter's extra arguments follow it, if it's kept last.
    let variadic_kept =
        matches!(new.last(), Some(NewParam::Kept { from, .. }) if old[*from].variadic);
    if variadic_kept {
        out.extend(variadic_extra.into_iter().map(str::to_owned));
    }
    out.join(", ")
}

/// The declaration's new parameter list, laid out like the old one: on one line, or one
/// per line with the old indentation.
pub fn render_params(
    params: &[NewParam],
    old_inside: &str,
    syntax: &dyn SignatureSyntax,
) -> String {
    let rendered: Vec<String> = params.iter().map(|p| syntax.render(p.param())).collect();
    if !old_inside.contains('\n') || rendered.is_empty() {
        return rendered.join(", ");
    }
    // `(\n    $a,\n    $b\n)`: each on its line, the closing bracket's indentation kept.
    let indent: String = old_inside
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| c.is_whitespace()).collect())
        .unwrap_or_else(|| "    ".to_owned());
    let closing: String = old_inside
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let trailing_comma = old_inside.trim_end().ends_with(',');
    let mut out = String::from("\n");
    for (i, p) in rendered.iter().enumerate() {
        out.push_str(&indent);
        out.push_str(p);
        if i + 1 < rendered.len() || trailing_comma {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&closing);
    out
}

/// What is changed: the function and where its parameters are declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureTarget {
    /// The name the calls use (a constructor's: its class).
    pub name: String,
    pub kind: CallKind,
    /// The declaration's file and its parameter list's inside.
    pub path: PathBuf,
    pub params: Range<usize>,
    pub old: Vec<Param>,
}

/// How the calls look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    /// `name(`
    Function,
    /// `->name(`, `::name(`, `.name(`
    Method,
    /// `new Name(`
    Constructor,
}

/// A call found: its arguments' inside and each argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub inside: Range<usize>,
    /// The inside's text as found (checked before applying).
    pub text: String,
    pub args: Vec<String>,
    pub certainty: Certainty,
    pub line: u32,
    pub line_text: String,
    pub in_line: Range<usize>,
}

/// The calls, per file, and the declaration's text (to rewrite it).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Calls {
    pub files: Vec<(PathBuf, Vec<Call>)>,
    pub declaration: Option<(String, u32, String, Range<usize>)>,
}

/// Everything a Change Signature needs on a background thread.
#[derive(Clone)]
pub struct SignatureContext {
    pub target: SignatureTarget,
    pub sources: Arc<dyn Sources + Send + Sync>,
    pub resolver: Arc<dyn Resolver + Send + Sync>,
    pub syntax: Arc<dyn SignatureSyntax + Send + Sync>,
}

impl std::fmt::Debug for SignatureContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignatureContext").field("target", &self.target).finish_non_exhaustive()
    }
}

/// The calls of `target` in the project (resolved like a rename's places: sure ones
/// chosen, unknown ones shown).
pub fn find_calls(
    target: &SignatureTarget,
    syntax: &dyn SignatureSyntax,
    sources: &dyn Sources,
    resolver: &dyn Resolver,
    cancelled: &dyn Fn() -> bool,
) -> Calls {
    let mut calls = Calls::default();
    let rules = syntax.calls();
    if let Some(text) = sources.read(&target.path) {
        let inside = &text[target.params.clone()];
        let (line, line_text, in_line) = line_of(&text, &target.params);
        calls.declaration = Some((inside.to_owned(), line, line_text, in_line));
    }
    for path in sources.candidates() {
        if cancelled() {
            break;
        }
        let Some(text) = sources.read(&path) else { continue };
        if !text.contains(&target.name) {
            continue;
        }
        let mut found = Vec::new();
        for range in find_word(&text, &target.name, NameStyle::CODE) {
            if !is_call(&text, &range, target.kind, &rules)
                || is_declaration(target, &path, &text, &range)
            {
                continue;
            }
            let certainty = match resolver.resolve(&path, &text, range.clone()) {
                Resolution::Target => Certainty::Certain,
                Resolution::Unknown => Certainty::Uncertain,
                Resolution::Other => continue,
            };
            let open = range.end + (text[range.end..].len() - text[range.end..].trim_start().len());
            let Some((inside, items)) = split_list(&text, open) else { continue };
            let (line, line_text, in_line) = line_of(&text, &inside);
            found.push(Call {
                args: items.iter().map(|r| text[r.clone()].to_owned()).collect(),
                text: text[inside.clone()].to_owned(),
                inside,
                certainty,
                line,
                line_text,
                in_line,
            });
        }
        if !found.is_empty() {
            calls.files.push((path, found));
        }
    }
    calls
}

fn is_call(text: &str, range: &Range<usize>, kind: CallKind, rules: &CallRules) -> bool {
    let tail = text[range.end..].trim_start();
    let head = text[..range.start].trim_end();
    if !tail.starts_with('(') {
        return false;
    }
    let member = rules.member_access.iter().any(|a| head.ends_with(a));
    match kind {
        CallKind::Function => {
            !member
                && !rules.declares.iter().any(|w| head.ends_with(w))
                && !rules.sigil.is_some_and(|s| head.ends_with(s))
        }
        CallKind::Method => member,
        CallKind::Constructor => rules.constructs.is_some_and(|w| head.ends_with(w)),
    }
}

/// The declaration's own name (`function name(`) isn't a call.
fn is_declaration(target: &SignatureTarget, path: &Path, text: &str, range: &Range<usize>) -> bool {
    path == target.path && text[range.end..].trim_start().starts_with('(') && {
        let open = range.end + (text[range.end..].len() - text[range.end..].trim_start().len());
        open + 1 == target.params.start
    }
}

impl Calls {
    /// The refactoring's plan for `new`: the declaration's list and each call's arguments.
    /// `chosen` keeps the user's choices between rebuilds (`(file, call)` → chosen).
    pub fn plan(
        &self,
        target: &SignatureTarget,
        new: &[NewParam],
        syntax: &dyn SignatureSyntax,
        chosen: &dyn Fn(usize, usize) -> Option<bool>,
    ) -> Plan {
        let mut plan = Plan::default();
        let mut declared = false;
        for (f, (path, calls)) in self.files.iter().enumerate() {
            let mut occurrences = Vec::new();
            if *path == target.path
                && let Some((inside, line, line_text, in_line)) = &self.declaration
            {
                occurrences.push(Occurrence {
                    range: target.params.clone(),
                    original: inside.clone(),
                    replacement: render_params(new, inside, syntax),
                    certainty: Certainty::Certain,
                    chosen: true,
                    line: *line,
                    line_text: line_text.clone(),
                    in_line: in_line.clone(),
                });
                declared = true;
            }
            for (c, call) in calls.iter().enumerate() {
                let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
                occurrences.push(Occurrence {
                    range: call.inside.clone(),
                    original: call.text.clone(),
                    replacement: rewrite_args(&args, &target.old, new, syntax),
                    certainty: call.certainty,
                    chosen: chosen(f, c).unwrap_or(call.certainty == Certainty::Certain),
                    line: call.line,
                    line_text: call.line_text.clone(),
                    in_line: call.in_line.clone(),
                });
            }
            occurrences.sort_by_key(|o| o.range.start);
            plan.files.push(FilePlan { path: path.clone(), occurrences });
        }
        if !declared && let Some((inside, line, line_text, in_line)) = &self.declaration {
            plan.files.insert(
                0,
                FilePlan {
                    path: target.path.clone(),
                    occurrences: vec![Occurrence {
                        range: target.params.clone(),
                        original: inside.clone(),
                        replacement: render_params(new, inside, syntax),
                        certainty: Certainty::Certain,
                        chosen: true,
                        line: *line,
                        line_text: line_text.clone(),
                        in_line: in_line.clone(),
                    }],
                },
            );
        }
        plan
    }
}

#[cfg(test)]
#[path = "../tests/unit/signature.rs"]
mod tests;
