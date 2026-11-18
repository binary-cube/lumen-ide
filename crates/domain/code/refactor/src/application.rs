//! The refactoring use cases, over two ports: the project's [`Sources`] (which files, and
//! their text as in the editor) and a [`Resolver`] that says what a found name refers to
//! (the app answers with each language's own navigation rules). What a rename at the
//! cursor renames ([`rename_target`]) asks the [`Languages`] how they write names.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_intelligence::rename::Subject;
use code_intelligence::{FileSymbols, Naming, SymbolKind};

use crate::{Certainty, FilePlan, FileRename, NameStyle, Occurrence, Plan, find_word, line_of};

/// Where a renamed symbol can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Anywhere in the project (a class, a function, a method, a CSS class…).
    Project,
    /// Only in `path`, within `range` (a local variable, a parameter).
    Local { path: PathBuf, range: Range<usize> },
}

/// What is renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    /// The name as written (`$total` for a PHP variable, `card` for a CSS class).
    pub name: String,
    pub style: NameStyle,
    pub scope: Scope,
    /// The file named after the symbol (a PHP class, PSR-4), renamed with it.
    pub file: Option<PathBuf>,
}

/// The project's files.
pub trait Sources {
    /// The project's root (paths are shown relative to it).
    fn root(&self) -> Option<&Path> {
        None
    }
    /// The files that may use the symbol (by language).
    fn candidates(&self) -> Vec<PathBuf>;
    /// A file's text: the editor's when it's open (unsaved changes included).
    fn read(&self, path: &Path) -> Option<String>;
}

/// What a name found in a file refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The renamed symbol.
    Target,
    /// Another symbol with the same name: left alone.
    Other,
    /// Unknown (e.g. a method called on a value of unknown type).
    Unknown,
}

/// Says what a name at `range` of a file's `text` refers to.
pub trait Resolver {
    fn resolve(&self, path: &Path, text: &str, range: Range<usize>) -> Resolution;
}

/// Something already named like the new name, where the renamed symbol lives (another
/// function in the namespace, a method of the same class, a variable in the function…):
/// renaming would clash with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: PathBuf,
    /// 0-based.
    pub line: u32,
    /// The clashing line, trimmed (`function bla()`).
    pub line_text: String,
}

/// Finds what a new name would clash with (cheap: asked as the name is typed).
pub trait ConflictCheck {
    fn conflicts(&self, new_name: &str) -> Vec<Conflict>;
}

/// A rename ready to be planned on a background thread: what is renamed and the ports
/// answering for the project (snapshots: they don't borrow the editor).
#[derive(Clone)]
pub struct RenameContext {
    pub target: RenameTarget,
    pub sources: Arc<dyn Sources + Send + Sync>,
    pub resolver: Arc<dyn Resolver + Send + Sync>,
    pub conflicts: Arc<dyn ConflictCheck + Send + Sync>,
}

impl std::fmt::Debug for RenameContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenameContext").field("target", &self.target).finish_non_exhaustive()
    }
}

/// The places to change to rename `target`: resolved ones chosen, unresolved ones shown but
/// not chosen; within a local scope, everything with the name is the symbol. `cancelled`
/// stops early (a newer request).
pub fn plan_rename(
    target: &RenameTarget,
    sources: &dyn Sources,
    resolver: &dyn Resolver,
    cancelled: &dyn Fn() -> bool,
) -> Plan {
    let mut plan = Plan::default();
    let files = match &target.scope {
        Scope::Project => sources.candidates(),
        Scope::Local { path, .. } => vec![path.clone()],
    };
    for path in files {
        if cancelled() {
            break;
        }
        let Some(text) = sources.read(&path) else { continue };
        if !text.contains(&target.name) {
            continue;
        }
        let mut occurrences = Vec::new();
        for range in find_word(&text, &target.name, target.style) {
            let certainty = match &target.scope {
                Scope::Local { range: scope, .. } => {
                    if !(scope.start <= range.start && range.end <= scope.end) {
                        continue;
                    }
                    match resolver.resolve(&path, &text, range.clone()) {
                        Resolution::Other => continue,
                        _ => Certainty::Certain,
                    }
                }
                Scope::Project => match resolver.resolve(&path, &text, range.clone()) {
                    Resolution::Target => Certainty::Certain,
                    Resolution::Unknown => Certainty::Uncertain,
                    Resolution::Other => continue,
                },
            };
            let (line, line_text, in_line) = line_of(&text, &range);
            occurrences.push(Occurrence {
                original: text[range.clone()].to_owned(),
                range,
                replacement: target.name.clone(),
                certainty,
                chosen: certainty == Certainty::Certain,
                line,
                line_text,
                in_line,
            });
        }
        if !occurrences.is_empty() {
            plan.files.push(FilePlan { path, occurrences });
        }
    }
    if let Some(file) = &target.file {
        plan.renames.push(FileRename { from: file.clone(), to: file.clone(), chosen: true });
    }
    plan
}

/// What the languages say about the names in their files (their modules answer).
pub trait Languages {
    /// How the language of `path`'s file writes names (the default without one).
    fn naming(&self, path: &Path) -> Naming;
    /// A language keeps one type per file with `extension`, named after it (PSR-4).
    fn type_per_file(&self, extension: &str) -> bool;
}

/// Why a symbol isn't renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotRenamed {
    /// A library's symbol (vendor, node_modules): not the project's to rename.
    InLibrary,
}

/// `path` is a library's (vendor, node_modules) or Git's: not the project's to change.
pub fn in_library(root: Option<&Path>, path: &Path) -> bool {
    let relative = root.and_then(|r| path.strip_prefix(r).ok());
    relative.is_some_and(kernel::in_dependencies_or_git)
}

/// What renaming `subject` (found at the cursor in `path`, of the project at `root`)
/// renames: how its name is written, where it's used, and the file named after it.
pub fn rename_target(
    subject: &Subject,
    path: &Path,
    root: Option<&Path>,
    languages: &dyn Languages,
) -> Result<RenameTarget, NotRenamed> {
    if in_library(root, &subject.declared.at.path) {
        return Err(NotRenamed::InLibrary);
    }
    let kind = subject.declared.kind;
    // Names with characters of their own (CSS's `-`) take no sigil.
    let style = match kind.map_or("", SymbolKind::name_chars) {
        "" => NameStyle::CODE.with_sigil(languages.naming(path).sigil),
        extra => NameStyle { extra, sigil: None },
    };
    let scope = match &subject.local {
        Some(range) => Scope::Local { path: path.to_path_buf(), range: range.clone() },
        None => Scope::Project,
    };
    // A type in a file named after it (PSR-4): the file is renamed with it.
    let decl = &subject.declared.at.path;
    let named_after = decl.file_stem().is_some_and(|s| s.to_string_lossy() == subject.name)
        && languages.type_per_file(decl.extension().and_then(|e| e.to_str()).unwrap_or_default());
    let file = (kind.is_some_and(SymbolKind::is_type) && named_after).then(|| decl.clone());
    Ok(RenameTarget { name: subject.name.clone(), style, scope, file })
}

/// Where the type named after file `path` is declared in it (its name's offset), when its
/// language keeps one type per file (PSR-4): renaming the file renames it.
pub fn type_named_after(
    path: &Path,
    symbols: &FileSymbols,
    languages: &dyn Languages,
) -> Option<usize> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    if !languages.type_per_file(extension) {
        return None;
    }
    let stem = path.file_stem().and_then(|s| s.to_str())?;
    symbols.symbols.iter().find(|s| s.kind.is_type() && s.name == stem.into()).map(|s| s.offset)
}

#[cfg(test)]
#[path = "../tests/unit/application.rs"]
mod tests;
