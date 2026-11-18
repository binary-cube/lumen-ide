//! Searching the project's declarations by name (Go to Symbol, a query from a module or an
//! agent): fuzzy on the name, filtered by kind, language, dependencies, limited.

use std::path::{Path, PathBuf};

use crate::{SymbolIndex, SymbolKind, fuzzy_score};

/// What is searched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolQuery<'a> {
    /// Part of the name (fuzzy: `uctrl` finds `UserController`).
    pub text: &'a str,
    /// The kinds wanted (empty: every kind).
    pub kinds: &'a [SymbolKind],
    /// The languages wanted, by id (empty: every language).
    pub languages: &'a [&'a str],
    /// Declarations in dependency directories (`vendor/`, `node_modules/`) count too.
    pub dependencies: bool,
    /// The project's root: dependencies are recognized relative to it.
    pub root: Option<&'a Path>,
    pub limit: usize,
}

/// A declaration found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolHit {
    pub path: PathBuf,
    pub name: String,
    pub kind: SymbolKind,
    /// The type a member belongs to (its FQN).
    pub container: Option<String>,
    pub fqn: String,
    pub offset: usize,
    pub line: u32,
    pub score: i32,
}

/// The declarations matching `query`, best first (then shorter names, then by path).
pub fn search_symbols(index: &SymbolIndex, query: &SymbolQuery<'_>) -> Vec<SymbolHit> {
    let mut hits = Vec::new();
    if query.limit == 0 {
        return hits;
    }
    for (path, kept) in index.files() {
        if !query.languages.is_empty() && !query.languages.contains(&&*kept.language) {
            continue;
        }
        if !query.dependencies {
            let relative = query.root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path);
            if kernel::in_dependencies_or_git(relative) {
                continue;
            }
        }
        // All its symbols, a library file's members too (unpacked for the search only).
        let Some(file) = index.file_once(path) else { continue };
        for symbol in &file.symbols {
            if !query.kinds.is_empty() && !query.kinds.contains(&symbol.kind) {
                continue;
            }
            let Some(score) = fuzzy_score(query.text, &symbol.name) else { continue };
            hits.push(SymbolHit {
                path: path.to_path_buf(),
                name: symbol.name.to_string(),
                kind: symbol.kind,
                container: symbol.container.as_deref().map(str::to_owned),
                fqn: symbol.fqn.to_string(),
                offset: symbol.offset,
                line: symbol.line,
                score,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score.cmp(&a.score).then(a.name.len().cmp(&b.name.len())).then(a.path.cmp(&b.path))
    });
    hits.truncate(query.limit);
    hits
}

#[cfg(test)]
#[path = "../tests/unit/query.rs"]
mod tests;
