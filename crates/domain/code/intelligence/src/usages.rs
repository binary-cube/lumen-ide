//! Where a symbol is used ("Find Usages"): its name searched through the project's files,
//! by the rules of its kind and of its language's code ([`SourceRules`]: a method follows a
//! member access, `->` / `.`, a function is followed by `(`…),
//! in the files where a symbol of its language can be used — said by its language's module
//! ([`LanguageIntelligence::usage_extensions`]: a stylesheet's selectors across languages).
//! Pure: the files are read through the caller.

use std::path::{Path, PathBuf};

use crate::{LanguageIntelligence, SourceRules, SymbolKind};

/// At most this many usages are collected.
pub const LIMIT: usize = 200;

/// What is searched: the name, kind, language and declaration (excluded from results).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageQuery {
    pub name: String,
    pub kind: SymbolKind,
    pub language: String,
    /// Relative to the project root.
    pub declared_in: Option<PathBuf>,
    pub declared_line: u32,
}

impl UsageQuery {
    /// The same declaration, regardless of its line (it moved: typing above it).
    pub fn same_symbol(&self, other: &Self) -> bool {
        self.name == other.name
            && self.kind == other.kind
            && self.language == other.language
            && self.declared_in == other.declared_in
    }
}

/// A usage: the file (relative to the project root) and the line (0-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageAt {
    pub path: PathBuf,
    pub line: u32,
}

/// The extensions of the files a symbol of `kind` can be used in, by its language
/// (`intelligence`; none: nowhere): a CSS selector's too (the stylesheets' module says).
pub fn usage_extensions(
    intelligence: Option<&dyn LanguageIntelligence>,
    kind: SymbolKind,
) -> &'static [&'static str] {
    intelligence.map_or(&[], |i| i.usage_extensions(kind))
}

/// The usages of `query` in `files` (relative to the project; already filtered to the
/// right extensions) by its language's `rules`, read through `read` (`None`: unreadable or
/// too big, skipped): one per line, not the declaration itself, at most [`LIMIT`]; stops
/// when `cancelled`.
pub fn find_usages(
    files: &[String],
    query: &UsageQuery,
    rules: &SourceRules,
    read: &dyn Fn(&Path) -> Option<String>,
    cancelled: &dyn Fn() -> bool,
) -> Vec<UsageAt> {
    let mut out = Vec::new();
    for file in files {
        if out.len() >= LIMIT || cancelled() {
            break;
        }
        let path = Path::new(file);
        let Some(source) = read(path) else { continue };
        let own = query.declared_in.as_deref() == Some(path);
        let mut last_line = None;
        // Lines counted on from the previous match (the matches come in order), not from
        // the file's start each time.
        let (mut counted_to, mut lines) = (0, 0u32);
        for at in matches(&source, query, rules) {
            if at < counted_to {
                (counted_to, lines) = (0, 0);
            }
            lines += source[counted_to..at].bytes().filter(|&b| b == b'\n').count() as u32;
            counted_to = at;
            let line = lines;
            if (own && line == query.declared_line) || last_line == Some(line) {
                continue;
            }
            last_line = Some(line);
            out.push(UsageAt { path: path.to_path_buf(), line });
            if out.len() >= LIMIT {
                break;
            }
        }
    }
    out
}

/// How many lines use each of `queries` (of one language: `rules`) in `files` (each read
/// once, through `read`), the declarations' own lines left out — at most [`LIMIT`] each
/// (code lenses: `200+`). Stops when `cancelled` (what's counted so far).
pub fn count_usages(
    files: &[String],
    queries: &[UsageQuery],
    rules: &SourceRules,
    read: &dyn Fn(&Path) -> Option<String>,
    cancelled: &dyn Fn() -> bool,
) -> Vec<usize> {
    let mut counts = vec![0usize; queries.len()];
    for file in files {
        if cancelled() {
            break;
        }
        let path = Path::new(file);
        let Some(source) = read(path) else { continue };
        for (i, query) in queries.iter().enumerate() {
            if counts[i] >= LIMIT || !source.contains(query.name.as_str()) {
                continue;
            }
            let own = query.declared_in.as_deref() == Some(path);
            let mut last_line = None;
            let (mut counted_to, mut lines) = (0, 0u32);
            for at in matches(&source, query, rules) {
                lines += source[counted_to..at].bytes().filter(|&b| b == b'\n').count() as u32;
                counted_to = at;
                if (own && lines == query.declared_line) || last_line == Some(lines) {
                    continue;
                }
                last_line = Some(lines);
                counts[i] += 1;
                if counts[i] >= LIMIT {
                    break;
                }
            }
        }
    }
    counts
}

/// The positions in `source` where the symbol appears (by the rules of its kind and of its
/// language's code, `rules`).
pub fn matches(source: &str, query: &UsageQuery, rules: &SourceRules) -> Vec<usize> {
    let name = query.name.as_str();
    let css = crate::is_css_kind(query.kind);
    let sigil = rules.naming.sigil;
    let is_word =
        |c: char| c.is_alphanumeric() || c == '_' || (css && c == '-') || Some(c) == sigil;
    let after_member = |head: &str| rules.member_access.iter().any(|a| head.ends_with(a));
    let after_static = |head: &str| rules.static_access.is_some_and(|a| head.ends_with(a));
    // A static property: `Type::$name`.
    let after_static_property = |head: &str| {
        let head = match sigil {
            Some(sigil) => head.strip_suffix(sigil),
            None => Some(head),
        };
        head.is_some_and(after_static)
    };
    let mut out = Vec::new();
    if name.is_empty() {
        return out;
    }
    let mut from = 0;
    while let Some(i) = source[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before = source[..at].chars().next_back();
        let after = source[from..].chars().next();
        // Its sigil before it is another name's, except a static property's (`Type::$name`).
        let static_property =
            query.kind == SymbolKind::Property && after_static_property(&source[..at]);
        if (before.is_some_and(is_word) && !static_property) || after.is_some_and(is_word) {
            continue;
        }
        let head = source[..at].trim_end();
        let tail = source[from..].trim_start();
        let ok = match query.kind {
            SymbolKind::Method => after_member(head) && tail.starts_with('('),
            SymbolKind::Property => {
                (after_member(head) && !after_static(head)) || after_static_property(head)
            }
            SymbolKind::ClassConstant | SymbolKind::EnumCase => after_static(head),
            SymbolKind::Function => {
                tail.starts_with('(')
                    && !after_member(head)
                    && !rules.declares_function.iter().any(|w| head.ends_with(w))
            }
            SymbolKind::Class | SymbolKind::Interface | SymbolKind::Trait | SymbolKind::Enum => {
                !after_member(head)
                    && !sigil.is_some_and(|s| head.ends_with(s))
                    && !rules.declares_type.iter().any(|k| head.ends_with(k))
            }
            SymbolKind::Variable | SymbolKind::Constant => !after_member(head),
            SymbolKind::CssClass
            | SymbolKind::CssId
            | SymbolKind::CssVariable
            | SymbolKind::Mixin => true,
            // A section's title names nothing used elsewhere.
            SymbolKind::Section => false,
        };
        // A comment's text names nothing: only what it writes as code is a usage.
        if ok && !rules.comments.is_some_and(|c| c.is_text(source, at)) {
            out.push(at);
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/usages.rs"]
mod tests;
