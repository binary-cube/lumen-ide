//! What template languages' suggestions share, read from the text around the cursor so an
//! unfinished template answers too: where a delimiter is open, what's being written there,
//! and the variables a controller passes to a template.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::time::SystemTime;

use crate::engine::Project;
use crate::{CompletionItem, FrameworkCompletion};

/// What's written at the cursor inside a Twig delimiter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TwigPlace {
    /// The tag's name, right after `{%`.
    Tag,
    /// After `|`.
    Filter,
    /// After `is` or `is not`.
    Test,
    /// An expression.
    Expression,
    /// After `object.`.
    Member { object: String },
}

/// Where the name being typed at `offset` starts (`extra`: its characters besides a word's).
pub fn name_start(text: &str, offset: usize, extra: &str) -> usize {
    text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || extra.contains(*c))
        .last()
        .map_or(offset, |(i, _)| i)
}

/// Which of `pairs` (open, close) is open at the end of `before`, and where it starts.
pub fn open_delimiter(
    before: &str,
    pairs: &[(&'static str, &'static str)],
) -> Option<(usize, &'static str)> {
    pairs
        .iter()
        .filter_map(|(open, close)| {
            let at = before.rfind(open)?;
            before[at..].find(close).is_none().then_some((at, *open))
        })
        .max_by_key(|(at, _)| *at)
}

/// Whether the end of `code` is inside a string.
pub fn in_string(code: &str) -> bool {
    let mut quote = None;
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            _ => {}
        }
    }
    quote.is_some()
}

/// Whether `code` ends with `word`, whole.
pub fn ends_with_word(code: &str, word: &str) -> bool {
    code.strip_suffix(word)
        .is_some_and(|before| !before.ends_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// What's at `offset` in a Twig template and the name being typed there. In a string or a
/// comment, or outside the delimiters: nothing.
pub fn twig_place(text: &str, offset: usize) -> Option<(TwigPlace, Range<usize>)> {
    let start = name_start(text, offset, "");
    let pairs = [("{%", "%}"), ("{{", "}}"), ("{#", "#}")];
    let (open, kind) = open_delimiter(&text[..start], &pairs)?;
    if kind == "{#" {
        return None;
    }
    let code = &text[open + 2..start];
    if in_string(code) {
        return None;
    }
    let lead = code.trim_start_matches(['-', '~']).trim_end();
    let is_test = |l: &str| ends_with_word(l, "is");
    let place = if let Some(object) = lead.strip_suffix('.') {
        let object = object.trim_end();
        let name = &object[name_start(object, object.len(), "")..];
        TwigPlace::Member { object: name.to_owned() }
    } else if kind == "{%" && lead.is_empty() {
        TwigPlace::Tag
    } else if lead.ends_with('|') {
        TwigPlace::Filter
    } else if is_test(lead) || lead.strip_suffix("not").is_some_and(|l| is_test(l.trim_end())) {
        TwigPlace::Test
    } else {
        TwigPlace::Expression
    };
    Some((place, start..offset))
}

/// What's written at the cursor in a Blade template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BladePlace {
    /// A directive's name (`@if`), the `@` included.
    Directive,
    /// A variable (`$user`), the `$` included.
    Variable,
    /// After `$object->`.
    Member { object: String },
}

/// What's at `offset` in a Blade template and the name being typed there.
pub fn blade_place(text: &str, offset: usize) -> Option<(BladePlace, Range<usize>)> {
    let start = name_start(text, offset, "");
    let before = &text[..start];
    let echo = open_delimiter(before, &[("{{", "}}"), ("{!!", "!!}")]);
    if let Some(prefix) = before.strip_suffix('@') {
        let free = !prefix.ends_with(|c: char| c.is_alphanumeric() || c == '@');
        return (free && echo.is_none()).then_some((BladePlace::Directive, start - 1..offset));
    }
    if before.ends_with('$') {
        let line = before.rfind('\n').map_or(0, |i| i + 1);
        return (!in_string(&before[line..])).then_some((BladePlace::Variable, start - 1..offset));
    }
    let object = before.strip_suffix("->")?;
    let name = name_start(object, object.len(), "");
    let dollar = object[..name].ends_with('$');
    dollar.then(|| (BladePlace::Member { object: object[name..].to_owned() }, start..offset))
}

/// The names `@props([...])` declares.
pub fn declared_props(text: &str) -> Vec<String> {
    let Some(at) = text.find("@props(") else { return Vec::new() };
    let bytes = text.as_bytes();
    let first = skip_space(bytes, at + "@props(".len());
    if bytes.get(first) != Some(&b'[') {
        return Vec::new();
    }
    let Some(end) = after_closing(bytes, first) else { return Vec::new() };
    let (mut names, mut i, mut depth, mut item_start) = (Vec::new(), first, 0usize, false);
    while i < end {
        match bytes[i] {
            b'\'' | b'"' => {
                let Some((name, after)) = string_at(text, i) else { break };
                if depth == 1 && item_start {
                    names.push(name.to_owned());
                }
                item_start = false;
                i = after;
                continue;
            }
            b'[' | b'(' | b'{' => {
                depth += 1;
                item_start = depth == 1;
            }
            b']' | b')' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 1 => item_start = true,
            c if c.is_ascii_whitespace() => {}
            _ => item_start = false,
        }
        i += 1;
    }
    names
}

/// A module's suggestions and a framework's for the same place, as one list; for different
/// places, the module's.
pub fn join_suggestions(
    assisted: Option<FrameworkCompletion>,
    framework: Option<FrameworkCompletion>,
) -> Option<FrameworkCompletion> {
    match (assisted, framework) {
        (Some(a), Some(f)) if a.replace == f.replace => {
            let items: Vec<CompletionItem> = crate::servers::merge_completions(f.items, a.items);
            Some(FrameworkCompletion { replace: a.replace, items, merge: a.merge && f.merge })
        }
        (Some(a), _) => Some(a),
        (None, f) => f,
    }
}

/// The language's `items` for `replace` with what `help` adds there (a framework's words
/// joining the language's), best first; help for another place adds nothing.
pub fn add_to_language(
    items: &mut Vec<CompletionItem>,
    replace: &Range<usize>,
    help: Option<FrameworkCompletion>,
) {
    if let Some(help) = help.filter(|h| h.replace == *replace) {
        items.extend(help.items);
        items.sort_by(|a, b| b.score.cmp(&a.score).then(a.label.cmp(&b.label)));
    }
}

/// The index after the bracket closing the one at `open` (strings skipped).
fn after_closing(code: &[u8], open: usize) -> Option<usize> {
    let (mut depth, mut i, mut quote) = (0usize, open, None);
    while let Some(&c) = code.get(i) {
        match (quote, c) {
            (Some(_), b'\\') => i += 1,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, b'\'' | b'"') => quote = Some(c),
            (None, b'(' | b'[' | b'{') => depth += 1,
            (None, b')' | b']' | b'}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_space(code: &[u8], mut i: usize) -> usize {
    while code.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// The string literal at `i`: its text and the index after it.
fn string_at(code: &str, i: usize) -> Option<(&str, usize)> {
    let bytes = code.as_bytes();
    let quote = *bytes.get(i).filter(|c| matches!(c, b'\'' | b'"'))?;
    let mut end = i + 1;
    while *bytes.get(end)? != quote {
        end += if bytes[end] == b'\\' { 2 } else { 1 };
    }
    Some((code.get(i + 1..end)?, end + 1))
}

/// The keys of the array literal at `open` (its first level).
fn array_keys(code: &str, open: usize, end: usize, names: &mut Vec<String>) {
    let bytes = code.as_bytes();
    let (mut i, mut depth) = (open, 0usize);
    while i < end {
        match bytes[i] {
            b'\'' | b'"' => {
                let Some((text, after)) = string_at(code, i) else { return };
                let next = skip_space(bytes, after);
                if depth == 1 && code[next..].starts_with("=>") {
                    names.push(text.to_owned());
                }
                i = after;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
}

/// The strings given to the call whose `(` is at `open` (`compact('a', 'b')`).
fn call_strings(code: &str, open: usize, end: usize, names: &mut Vec<String>) {
    let mut i = open + 1;
    while i < end {
        if let Some((text, after)) = string_at(code, i) {
            names.push(text.to_owned());
            i = after;
        } else {
            i += 1;
        }
    }
}

/// The names given by the expression at `i` (an array, `compact(…)`).
fn given_names(code: &str, i: usize, names: &mut Vec<String>) {
    let bytes = code.as_bytes();
    let rest = &code[i..];
    let open = if rest.starts_with('[') {
        i
    } else if rest.starts_with("array(") {
        i + 5
    } else if rest.starts_with("compact(") {
        let open = i + 7;
        if let Some(end) = after_closing(bytes, open) {
            call_strings(code, open, end, names);
        }
        return;
    } else {
        return;
    };
    if let Some(end) = after_closing(bytes, open) {
        array_keys(code, open, end, names);
    }
}

/// The `->with(…)`s chained after index `i`: the names they give.
fn chained_with(code: &str, mut i: usize, names: &mut Vec<String>) {
    let bytes = code.as_bytes();
    const WITH: &str = "->with(";
    loop {
        i = skip_space(bytes, i);
        if !code[i..].starts_with(WITH) {
            return;
        }
        let open = i + WITH.len() - 1;
        let Some(end) = after_closing(bytes, open) else { return };
        let first = skip_space(bytes, open + 1);
        match string_at(code, first) {
            Some((name, _)) => names.push(name.to_owned()),
            None => given_names(code, first, names),
        }
        i = end;
    }
}

/// Each call of `calls` in `php` (with the bracket: `->render(`) naming a template: the
/// template and the variables it's given.
pub fn passed_variables(php: &str, calls: &[&str]) -> Vec<Rendered> {
    let bytes = php.as_bytes();
    let mut found = Vec::new();
    for call in calls {
        for (at, _) in php.match_indices(call) {
            let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
            if call.starts_with(word) && php[..at].ends_with(word) {
                continue;
            }
            let open = at + call.len() - 1;
            let first = skip_space(bytes, open + 1);
            let Some((template, after)) = string_at(php, first) else { continue };
            let Some(end) = after_closing(bytes, open) else { continue };
            let mut names = Vec::new();
            let next = skip_space(bytes, after);
            if bytes.get(next) == Some(&b',') {
                given_names(php, skip_space(bytes, next + 1), &mut names);
            }
            chained_with(php, end, &mut names);
            found.push((template.to_owned(), names));
        }
    }
    found
}

/// A template named by a call, with the variables it's given.
type Rendered = (String, Vec<String>);

/// The variables passed to each template.
pub type Passed = HashMap<String, Vec<String>>;

/// The calls found in each file, read again only when the file changed.
#[derive(Debug, Default)]
pub struct PassedFiles {
    files: HashMap<String, (Option<SystemTime>, Vec<Rendered>)>,
}

impl PassedFiles {
    /// `files` (relative to the project's root) brought up to date: what they pass.
    pub fn update(&mut self, project: &Project<'_>, files: &[String], calls: &[&str]) -> Passed {
        let Some(root) = project.root else { return Passed::new() };
        // Thousands of sources: each looked for in a set, not in the list.
        let listed: std::collections::HashSet<&str> = files.iter().map(String::as_str).collect();
        self.files.retain(|file, _| listed.contains(file.as_str()));
        let mut passed = Passed::new();
        for file in files {
            let path = root.join(file);
            let modified = (project.modified)(Path::new(&path));
            let known = self.files.get(file).filter(|(m, _)| m.is_some() && *m == modified);
            let found = match known {
                Some((_, found)) => found.clone(),
                None => {
                    let text = (project.read)(&path).unwrap_or_default();
                    let found = passed_variables(&text, calls);
                    self.files.insert(file.clone(), (modified, found.clone()));
                    found
                }
            };
            for (template, names) in found {
                let all = passed.entry(template).or_default();
                all.extend(names);
                all.sort();
                all.dedup();
            }
        }
        passed
    }
}

#[cfg(test)]
#[path = "../tests/unit/template.rs"]
mod tests;
