//! Completion and "go to definition", over the context given by the language and the index.

use std::path::Path;

use kernel::LanguageId;

use crate::index::{Location, SymbolIndex};
use crate::symbols::{FileSymbols, Symbol, SymbolKind};
use crate::{CompletionKind, LanguageIntelligence};

mod completer;
mod completion;
mod definition;
mod hover;

pub use completion::complete;
pub use definition::{definition, definition_of};
pub use hover::{hover, hover_of, signature_plainly, split_param_plainly};

/// Project access for completion and definitions: the root, reading files and
/// listing directories (the domain does no I/O; the application provides the functions).
pub struct Project<'a> {
    pub root: Option<&'a Path>,
    pub read: &'a dyn Fn(&Path) -> Option<String>,
    /// A directory's entries: (name, is directory).
    pub list: &'a dyn Fn(&Path) -> Vec<(String, bool)>,
    /// When a file was last changed on disk (to re-read only what changed).
    pub modified: &'a dyn Fn(&Path) -> Option<std::time::SystemTime>,
    /// The language level chosen for the project, per language (e.g. `php` → `8.2`).
    pub levels: &'a [(LanguageId, String)],
    /// Every language's intelligence: what a file of another language says of itself (its
    /// doc comments, a CSS rule) is read by its own.
    pub languages: &'a [std::sync::Arc<dyn LanguageIntelligence>],
}

impl std::fmt::Debug for Project<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project").field("root", &self.root).finish_non_exhaustive()
    }
}

impl Project<'_> {
    /// A project without files (tests, unsaved documents).
    pub fn none() -> Project<'static> {
        Project {
            root: None,
            read: &|_| None,
            list: &|_| Vec::new(),
            modified: &|_| None,
            levels: &[],
            languages: &[],
        }
    }

    /// The intelligence of `path`'s language (by its extension), if one is known.
    pub fn intelligence_for(&self, path: &Path) -> Option<&dyn LanguageIntelligence> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        self.languages
            .iter()
            .find(|i| i.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
            .map(AsRef::as_ref)
    }

    /// The level chosen for the language, if any.
    pub fn level(&self, language: &LanguageId) -> Option<&str> {
        self.levels.iter().find(|(l, _)| l == language).map(|(_, v)| v.as_str())
    }

    /// The language word exists at the project's chosen level (no level: yes).
    pub fn available(&self, intelligence: &dyn LanguageIntelligence, word: &str) -> bool {
        let (Some(level), Some(since)) =
            (self.level(&intelligence.language()), intelligence.since(word))
        else {
            return true;
        };
        let levels = intelligence.levels();
        let position = |v: &str| levels.iter().position(|l| *l == v);
        match (position(level), position(since)) {
            (Some(level), Some(since)) => since <= level,
            _ => true,
        }
    }

    /// The file exists (from its metadata; without it, e.g. in tests, by reading it).
    pub fn exists(&self, path: &Path) -> bool {
        (self.modified)(path).is_some() || (self.read)(path).is_some()
    }

    /// The path of a file included from `from`: relative to `from`'s directory (with `/…` after
    /// `__DIR__ .`), then to the project root.
    pub fn resolve_include(&self, from: &Path, include: &str) -> Option<std::path::PathBuf> {
        let relative = include.trim_start_matches('/');
        let dir = from.parent()?;
        std::iter::once(dir.join(relative))
            .chain(self.root.map(|r| r.join(relative)))
            .find(|p| self.exists(p))
    }

    /// The entries of directory `dir` (as in the code), relative to file `from`.
    fn entries(&self, from: &Path, dir: &str) -> Vec<(String, bool)> {
        let relative = dir.trim_start_matches('/');
        let Some(base) = from.parent() else { return Vec::new() };
        let entries = (self.list)(&base.join(relative));
        if !entries.is_empty() {
            return entries;
        }
        self.root.map(|r| (self.list)(&r.join(relative))).unwrap_or_default()
    }
}

fn location(path: &Path, s: &Symbol) -> Location {
    Location { path: path.to_path_buf(), offset: s.offset, line: s.line }
}

/// Files in `vendor/` (libraries) are less relevant than the project's own.
fn is_vendor(path: &Path) -> bool {
    kernel::in_dependencies(path)
}

fn kind_of(s: &Symbol) -> CompletionKind {
    match s.kind {
        SymbolKind::Class | SymbolKind::Trait | SymbolKind::Enum => CompletionKind::Class,
        SymbolKind::Interface => CompletionKind::Interface,
        SymbolKind::Function => CompletionKind::Function,
        SymbolKind::Method => CompletionKind::Method,
        SymbolKind::Property => CompletionKind::Property,
        SymbolKind::Constant | SymbolKind::ClassConstant | SymbolKind::EnumCase => {
            CompletionKind::Constant
        }
        SymbolKind::CssClass | SymbolKind::CssId | SymbolKind::CssVariable => {
            CompletionKind::Selector
        }
        SymbolKind::Mixin => CompletionKind::Function,
        SymbolKind::Variable => CompletionKind::Variable,
        SymbolKind::Section => CompletionKind::Namespace,
    }
}

/// The symbol is from a file of the given language (nothing unpacked: plan 74).
fn in_language(index: &SymbolIndex, path: &Path, language: &str) -> bool {
    index.language_of(path).is_some_and(|l| l.is_empty() || l == language)
}

/// The file is in `language`, or in none (suggested everywhere).
fn of_language(file: &FileSymbols, language: &str) -> bool {
    file.language.is_empty() || &*file.language == language
}

#[cfg(test)]
#[path = "../../tests/unit/engine.rs"]
mod tests;
