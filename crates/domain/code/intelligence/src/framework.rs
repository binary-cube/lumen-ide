//! Frameworks (Symfony, Laravel, Yii, React, Vue, htmx…), each contributed by its module.
//!
//! A [`Framework`] is only a detector: cheap, always registered. When a project uses it
//! (detected, or chosen in the project's settings), its [`FrameworkSupport`] is created for
//! that project — with whatever it caches — and dropped with the project. Frameworks of the
//! same family exclude each other (one PHP framework per project).
//!
//! **Its own generation.** A framework says which files its model is made from
//! ([`FrameworkSupport::watches`]: `config/**`, `src/**/*.php`); its generation
//! ([`FrameworkContext::own`]) changes only when one of them does, so what it caches under it
//! (its routes) survives every other change, and the app makes it again in the background
//! ([`FrameworkSupport::prepare`]) before it's asked. Its names (a route's) take part in Find
//! Usages and Rename ([`FrameworkSupport::name_at`] and the rest of `name_*`: pure, the app
//! turns them into a rename), its problems (a route that doesn't exist) are shown like any
//! module's ([`FrameworkSupport::diagnostics`]).

use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::engine::Project;
use crate::{CompletionItem, HoverInfo, Location};

pub trait Framework: Send + Sync + Debug {
    /// Stable id, used in `<project dir>/project.toml` (e.g. `symfony`).
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// Frameworks of a family exclude each other (e.g. `php`: Symfony, Laravel, Yii).
    fn family(&self) -> Option<&'static str> {
        None
    }
    /// Whether the project uses it, and from what (e.g. `composer.json`). Reads a few
    /// small files at most.
    fn detect(&self, project: &Project<'_>) -> Option<String>;
    /// Its help for one project (created only when the project uses it), e.g. for the
    /// framework version the project uses.
    fn activate(&self, project: &Project<'_>) -> Arc<dyn FrameworkSupport>;
}

/// Where the framework is asked for help: the file, its language at the cursor, the text.
#[derive(Debug)]
pub struct FrameworkContext<'a> {
    pub project: &'a Project<'a>,
    pub path: &'a Path,
    /// The language at the cursor (e.g. `php`, `twig`, `html`).
    pub language: &'a str,
    pub text: &'a str,
    pub offset: usize,
    /// Changes whenever a project file changes on disk (or is saved): what a framework
    /// gathers from the files' text (routes) is cached under it (a [`kernel::Memo`] key).
    pub generation: u64,
    /// Changes only when project files are made, removed or renamed (not saved): lists of
    /// files (templates, the folders walked) are cached under it.
    pub structure: u64,
    /// The framework's own: changes only when one of the files it [watches] does (on its
    /// own, it changes with `generation`).
    ///
    /// [watches]: FrameworkSupport::watches
    pub own: u64,
}

/// A framework's own name at the cursor (a route's): what Find Usages and Rename act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkName {
    /// What kind of name (the framework's: `route`), asked back with the name.
    pub kind: &'static str,
    pub name: String,
    /// Where it's written at the cursor.
    pub range: Range<usize>,
}

/// How serious a framework's problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemLevel {
    Error,
    Warning,
}

/// A problem a framework finds in a document (a route that doesn't exist): where (bytes),
/// how serious, what it says (translated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkProblem {
    pub range: Range<usize>,
    pub level: ProblemLevel,
    pub message: String,
}

/// Whether file `relative` (to the project's root, with `/`) is one of `patterns`' (globs,
/// [`kernel::paths::glob_match`]); none: every file is.
pub fn watched(patterns: &[&str], relative: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|p| kernel::paths::glob_match(p, relative))
}

/// A framework's suggestions: the range they replace, and whether they join the
/// language's own (e.g. htmx attributes among HTML's) or replace them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkCompletion {
    pub replace: Range<usize>,
    pub items: Vec<CompletionItem>,
    pub merge: bool,
}

/// A framework's help for a project. Everything defaults to "nothing to add".
pub trait FrameworkSupport: Send + Sync + Debug {
    fn complete(&self, _at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        None
    }
    /// Where what's at the cursor leads (e.g. a template name → its file, a route name →
    /// its declaration).
    fn definition(&self, _at: &FrameworkContext<'_>) -> Option<Location> {
        None
    }
    fn hover(&self, _at: &FrameworkContext<'_>) -> Option<HoverInfo> {
        None
    }

    /// The files its model is made from (globs relative to the project's root): its
    /// generation changes only when one of them does. None: any file.
    fn watches(&self) -> &'static [&'static str] {
        &[]
    }

    /// Makes its model for `at.own` now (in the background, after one of its files changed
    /// or as the project opens), so what's asked on the UI thread is already there.
    fn prepare(&self, _at: &FrameworkContext<'_>) {}

    /// Its name at the cursor (a route's in `path('…')`), for Find Usages and Rename.
    fn name_at(&self, _at: &FrameworkContext<'_>) -> Option<FrameworkName> {
        None
    }

    /// Whether names of `kind` may be written in file `relative` (by its kind: `.php`,
    /// `.twig`, `.yaml`) — the files a rename reads.
    fn names_in(&self, _kind: &str, _relative: &str) -> bool {
        false
    }

    /// Where `name` of `kind` is written in `text` (file `relative`): the name's own ranges.
    fn name_places(
        &self,
        _kind: &str,
        _name: &str,
        _relative: &str,
        _text: &str,
    ) -> Vec<Range<usize>> {
        Vec::new()
    }

    /// Where `name` of `kind` is declared already (a rename to it would clash), from its
    /// model as last made.
    fn name_taken(&self, _kind: &str, _name: &str) -> Vec<Location> {
        Vec::new()
    }

    /// The problems it finds in the document `at` (its whole text; `at.offset` unused).
    fn diagnostics(&self, _at: &FrameworkContext<'_>) -> Vec<FrameworkProblem> {
        Vec::new()
    }

    /// The names it knows the class of, by table (a service's id → its class), for the
    /// language's inference ([`crate::calls::ClassArgumentCall::names`]): put in the
    /// project's index once its model is made (asked in the background, after
    /// [`Self::prepare`]).
    fn named_types(
        &self,
        _at: &FrameworkContext<'_>,
    ) -> Vec<(&'static str, HashMap<String, String>)> {
        Vec::new()
    }
}

/// The frameworks a project uses: the detected ones, corrected by the project's choices
/// (`true` forces one on, `false` off), at most one per family (a forced one wins, then
/// the first in registration order). Ids, in registration order.
/// The project's choices after turning framework `id` on or off (`detected`: what the
/// detectors found): turning one on takes the others of its family off; a choice the same
/// as detected, with no family to win against, isn't kept (nothing to choose).
pub fn choose_framework(
    frameworks: &[Arc<dyn Framework>],
    detected: &[(String, String)],
    choices: &[(String, bool)],
    id: &str,
    on: bool,
) -> Vec<(String, bool)> {
    let family = frameworks.iter().find(|f| f.id() == id).and_then(|f| f.family());
    let mut choices: Vec<(String, bool)> =
        choices.iter().filter(|(c, _)| c != id).cloned().collect();
    if on && let Some(family) = family {
        let rivals: Vec<&str> =
            frameworks.iter().filter(|f| f.family() == Some(family)).map(|f| f.id()).collect();
        choices.retain(|(c, v)| !(*v && rivals.contains(&c.as_str())));
    }
    let found = detected.iter().any(|(d, _)| d == id);
    if on != found || family.is_some() {
        choices.push((id.to_owned(), on));
    }
    choices
}

pub fn active_frameworks(
    frameworks: &[Arc<dyn Framework>],
    detected: &[(String, String)],
    choices: &[(String, bool)],
) -> Vec<&'static str> {
    let choice = |id: &str| choices.iter().find(|(c, _)| c == id).map(|(_, on)| *on);
    let on: Vec<&Arc<dyn Framework>> = frameworks
        .iter()
        .filter(|f| choice(f.id()).unwrap_or_else(|| detected.iter().any(|(d, _)| d == f.id())))
        .collect();
    let mut active: Vec<&'static str> = Vec::new();
    for f in &on {
        let Some(family) = f.family() else {
            active.push(f.id());
            continue;
        };
        let rivals: Vec<_> = on.iter().filter(|o| o.family() == Some(family)).collect();
        let winner = rivals
            .iter()
            .find(|o| choice(o.id()) == Some(true))
            .or_else(|| rivals.first())
            .map(|o| o.id());
        if winner == Some(f.id()) {
            active.push(f.id());
        }
    }
    active
}

/// The content of the innermost quoted string (`'…'` or `"…"`, on one line) around
/// `offset` — in `href="{{ path('x') }}"`, the `'x'`. A string still being typed runs to
/// the end of the line.
pub fn quoted_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let offset = offset.min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    let line = &text[line_start..line_end];
    // Each kind of quote on its own: the other kind is just text inside.
    let around = |quote: u8| -> Option<Range<usize>> {
        let mut open: Option<usize> = None;
        for (i, b) in line.bytes().enumerate() {
            let at = line_start + i;
            if b != quote || (i > 0 && line.as_bytes()[i - 1] == b'\\') {
                continue;
            }
            match open.take() {
                Some(start) if (start + 1..=at).contains(&offset) => return Some(start + 1..at),
                Some(_) => {}
                None if at < offset => open = Some(at),
                None => return None,
            }
        }
        open.map(|start| start + 1..line_end)
    };
    [around(b'\''), around(b'"')].into_iter().flatten().min_by_key(|r| r.len())
}

/// The text right before the string at `content` (its opening quote excluded), without
/// trailing spaces, e.g. `$this->render(` or `{% extends`.
pub fn before_string<'a>(text: &'a str, content: &Range<usize>) -> &'a str {
    text[..content.start.saturating_sub(1)].trim_end()
}

impl Project<'_> {
    /// The files under `dir` (relative to the project root) whose names end with one of
    /// `suffixes`, as paths relative to `dir` with `/`; hidden directories, `vendor` and
    /// `node_modules` skipped; at most `limit`.
    pub fn files_under(&self, dir: &str, suffixes: &[&str], limit: usize) -> Vec<String> {
        let Some(root) = self.root else { return Vec::new() };
        let mut out = Vec::new();
        let mut stack = vec![String::new()];
        while let Some(relative) = stack.pop() {
            let path =
                if relative.is_empty() { root.join(dir) } else { root.join(dir).join(&relative) };
            let mut entries = (self.list)(&path);
            entries.sort();
            for (name, is_dir) in entries {
                let child =
                    if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
                if is_dir {
                    if !name.starts_with('.') && !kernel::paths::is_dependency_dir(&name) {
                        stack.push(child);
                    }
                } else if suffixes.iter().any(|s| name.ends_with(s)) {
                    out.push(child);
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// The text of a file relative to the project root.
    pub fn read_relative(&self, relative: &str) -> Option<String> {
        (self.read)(&self.root?.join(relative))
    }
}

/// The component tag name at `offset` (`<UserCard`, `</UserCard`, `<user-card`): its
/// range. A component's name starts with an uppercase letter or holds a `-`.
pub fn component_tag_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let is_name = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.');
    let offset = offset.min(text.len());
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = offset + text[offset..].find(|c: char| !is_name(c)).unwrap_or(text.len() - offset);
    let lead = &text[..start];
    if !(lead.ends_with('<') || lead.ends_with("</")) || start == end {
        return None;
    }
    let name = &text[start..end];
    let component = name.starts_with(|c: char| c.is_uppercase()) || name.contains('-');
    component.then_some(start..end)
}

/// `user-card` → `UserCard` (Vue writes components either way).
pub fn pascal_case(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        })
        .collect()
}

/// Suggestions for `names` matching `query` (fuzzy), best first, at most 80.
pub fn suggest<'a>(
    query: &str,
    names: impl IntoIterator<Item = &'a str>,
    kind: crate::CompletionKind,
    detail: Option<&str>,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = names
        .into_iter()
        .filter_map(|name| {
            let mut item = CompletionItem::word(name, kind, detail);
            item.score = crate::fuzzy_score(query, name)?;
            Some(item)
        })
        .collect();
    items.sort_by(|a, b| {
        b.score.cmp(&a.score).then(a.label.len().cmp(&b.label.len())).then(a.label.cmp(&b.label))
    });
    items.truncate(80);
    items
}

/// The line (0-based) of `offset` in `text`.
/// The attribute name being typed in a tag (`<li v-f|`, `<button hx-sw|`): its range. A
/// name holds letters, digits and `extra` (`-`, `:`, `@`…, as the framework writes them).
pub fn attribute_name_at(text: &str, offset: usize, extra: &[char]) -> Option<Range<usize>> {
    let before = &text[..offset];
    let tag = before.rfind('<')?;
    if before[tag..].contains('>') || quoted_at(text, offset).is_some() {
        return None;
    }
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || extra.contains(c))
        .last()
        .map_or(offset, |(i, _)| i);
    (start < offset && before[..start].ends_with(char::is_whitespace)).then_some(start..offset)
}

pub fn line_of(text: &str, offset: usize) -> u32 {
    kernel::text::line_of(text, offset)
}

/// `name` is among a `composer.json` / `package.json`'s dependencies (`require`,
/// `require-dev`, `dependencies`, `devDependencies`…): a quoted key followed by `:`.
pub fn has_dependency(manifest: &str, name: &str) -> bool {
    let key = format!("\"{name}\"");
    manifest
        .match_indices(&key)
        .any(|(i, _)| manifest[i + key.len()..].trim_start().starts_with(':'))
}

/// The content of the closed string around `offset` when it reads as a path or a dotted
/// name (no spaces, a `.` or `/` inside, not too long, nothing interpolated: no braces,
/// calls, `$` — PHP's, JS's, Blade's and the shell's variables — or the language's `sigil`).
pub fn path_in_quotes(
    source: &str,
    offset: usize,
    sigil: Option<char>,
) -> Option<std::ops::Range<usize>> {
    let content = quoted_at(source, offset)?;
    let closed = source[content.end..].starts_with(['\'', '"']);
    let text = &source[content.clone()];
    let path_like = text.len() <= 300
        && text.contains(['.', '/'])
        && !text.contains(char::is_whitespace)
        && !text.contains(['{', '}', '(', ')', '$'])
        && sigil.is_none_or(|s| !text.contains(s));
    (closed && path_like).then_some(content)
}

#[cfg(test)]
#[path = "../tests/unit/framework.rs"]
mod tests;
