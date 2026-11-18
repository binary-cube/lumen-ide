//! A framework's own names (a Symfony route's) in Find Usages and Rename: what its support
//! says — the name at the cursor, the files that may write it, where a file writes it, where
//! a new name is declared already — made into the [`RenameContext`] the refactoring module
//! plans from (its preview, applying and undo are any rename's). Asked after the modules'
//! names ([`crate::CursorSymbols`]), before the language servers and the built-in engine.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use code_intelligence::{FrameworkName, FrameworkSupport};
use refactor::NameStyle;
use refactor::application::{
    Conflict, ConflictCheck, RenameContext, RenameTarget, Resolution, Resolver, Scope, Sources,
};

use crate::ProjectQueries;

/// The characters a framework's name holds besides a code identifier's (`blog.show`,
/// `admin-users`): a match must not be part of a longer one. Not `:` — a YAML key ends with
/// it (`blog:`); where a name really is is the framework's to say ([`Places`]).
const NAME_CHARS: &str = ".-";

/// What renaming `name` (found by `support` at the cursor) renames: the project's files
/// (`queries`; the open ones as in the editor, `open`) that may write it, its places in
/// each as the support says.
pub fn rename_context(
    support: Arc<dyn FrameworkSupport>,
    name: FrameworkName,
    queries: Arc<dyn ProjectQueries>,
    open: Arc<HashMap<PathBuf, String>>,
) -> RenameContext {
    let files = Arc::new(Files { root: queries.root(), queries, open, support, kind: name.kind });
    RenameContext {
        target: RenameTarget {
            name: name.name.clone(),
            style: NameStyle { extra: NAME_CHARS, sigil: None },
            scope: Scope::Project,
            file: None,
        },
        sources: Arc::clone(&files) as Arc<dyn Sources + Send + Sync>,
        resolver: Arc::new(Places {
            files: Arc::clone(&files),
            name: name.name,
            last: Mutex::default(),
        }),
        conflicts: Arc::new(Taken { files }),
    }
}

/// The project's files that may write a name of `kind`.
struct Files {
    root: PathBuf,
    queries: Arc<dyn ProjectQueries>,
    open: Arc<HashMap<PathBuf, String>>,
    support: Arc<dyn FrameworkSupport>,
    kind: &'static str,
}

impl Files {
    /// `path` relative to the root, with `/`.
    fn relative(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }
}

impl Sources for Files {
    fn root(&self) -> Option<&Path> {
        Some(&self.root)
    }

    fn candidates(&self) -> Vec<PathBuf> {
        self.queries
            .file_list()
            .iter()
            .filter(|relative| !kernel::in_dependencies_or_git(Path::new(relative.as_str())))
            .filter(|relative| self.support.names_in(self.kind, relative))
            .map(|relative| self.root.join(relative))
            .collect()
    }

    fn read(&self, path: &Path) -> Option<String> {
        if let Some(text) = self.open.get(path) {
            return Some(text.clone());
        }
        self.queries.read_source(path.strip_prefix(&self.root).ok()?)
    }
}

/// A name found is the one renamed where the support places it; another elsewhere. The last
/// file's places are kept (its names are asked in turn).
struct Places {
    files: Arc<Files>,
    name: String,
    last: Mutex<Option<(PathBuf, Vec<Range<usize>>)>>,
}

impl Resolver for Places {
    fn resolve(&self, path: &Path, text: &str, range: Range<usize>) -> Resolution {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if last.as_ref().is_none_or(|(p, _)| p != path) {
            let relative = self.files.relative(path).unwrap_or_default();
            let places =
                self.files.support.name_places(self.files.kind, &self.name, &relative, text);
            *last = Some((path.to_path_buf(), places));
        }
        let places = last.as_ref().map_or(&[][..], |(_, p)| p.as_slice());
        if places.contains(&range) { Resolution::Target } else { Resolution::Other }
    }
}

/// A new name the framework declares already (its line read as the editor shows it).
struct Taken {
    files: Arc<Files>,
}

impl ConflictCheck for Taken {
    fn conflicts(&self, new_name: &str) -> Vec<Conflict> {
        self.files
            .support
            .name_taken(self.files.kind, new_name)
            .into_iter()
            .map(|at| {
                let text = self.files.read(&at.path).unwrap_or_default();
                let line_text =
                    text.lines().nth(at.line as usize).unwrap_or_default().trim().to_owned();
                Conflict { path: at.path, line: at.line, line_text }
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/framework_names.rs"]
mod tests;
