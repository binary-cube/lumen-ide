//! Symfony's YAML configuration: the keys of its service and route files (they join what
//! the YAML server knows) and, as values, `@service` and `%parameter%` written bare.

use std::ops::Range;
use std::sync::OnceLock;

use code_intelligence::template::name_start;
use code_intelligence::{CompletionItem, CompletionKind, FrameworkCompletion, Vocabulary};

/// Where the words come from: replacing this file (by a generated one) is all it takes.
const DATA: &str = include_str!("../data/yaml.data");

/// What's written at the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// A key, of the file's top level or nested.
    Key { nested: bool },
    /// `@name`.
    Service,
    /// `%name`.
    Parameter,
}

/// What's at `offset` and the name typed there (without its sign).
pub(crate) fn place_at(text: &str, offset: usize) -> Option<(Place, Range<usize>)> {
    let start = name_start(text, offset, ".-_\\@%");
    let line = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let lead = &text[line..start];
    let token = &text[start..offset];
    if lead.contains('#') {
        return None;
    }
    if let Some(name) = token.strip_prefix('@') {
        return Some((Place::Service, offset - name.len()..offset));
    }
    if let Some(name) = token.strip_prefix('%') {
        return Some((Place::Parameter, offset - name.len()..offset));
    }
    let nested = !lead.is_empty();
    (lead.trim().is_empty() && !token.is_empty()).then_some((Place::Key { nested }, start..offset))
}

/// The top-level key a key starting at `start` sits directly under; deeper keys have none.
pub(crate) fn parent_at(text: &str, start: usize) -> Option<&str> {
    let line = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let indent = start - line;
    let mut children = None;
    for above in text[..line].lines().rev() {
        let body = above.trim_start();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        let depth = above.len() - body.len();
        if depth == 0 {
            return (children.unwrap_or(indent) == indent).then(|| body.split(':').next())?;
        }
        children = Some(depth.min(children.unwrap_or(depth)));
    }
    None
}

/// `relative` is a YAML file of the project's configuration.
pub(crate) fn is_config(relative: &str) -> bool {
    relative.starts_with("config/") && (relative.ends_with(".yaml") || relative.ends_with(".yml"))
}

/// The file's kind, as far as its keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum File {
    Services,
    Routes,
    /// `config/packages/*`: a bundle's configuration.
    Package,
    Other,
}

impl File {
    pub(crate) fn of(relative: &str, routes: bool) -> Self {
        if routes {
            Self::Routes
        } else if relative.starts_with("config/packages/") {
            Self::Package
        } else if relative.contains("services") {
            Self::Services
        } else {
            Self::Other
        }
    }
}

static WORDS: OnceLock<Vocabulary> = OnceLock::new();

/// The keys for a key's place: nested or not and, in a package, under which top-level key.
pub(crate) fn keys(
    file: File,
    nested: bool,
    parent: Option<&str>,
    query: &str,
) -> Vec<CompletionItem> {
    let group = match (file, nested) {
        (File::Services, false) => "top",
        (File::Services, true) => "service",
        (File::Routes, true) => "route",
        (File::Package, false) => "package",
        (File::Package, true) => match parent {
            Some(parent) if parent != "package" => parent,
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let words = WORDS.get_or_init(|| Vocabulary::parse(DATA));
    words.suggest(&[(group, CompletionKind::Property)], query)
}

/// `items` for `replace`, joining the YAML server's when `merge`.
pub(crate) fn answer(
    replace: Range<usize>,
    items: Vec<CompletionItem>,
    merge: bool,
) -> Option<FrameworkCompletion> {
    (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge })
}

#[cfg(test)]
#[path = "../tests/unit/config_yaml.rs"]
mod tests;
