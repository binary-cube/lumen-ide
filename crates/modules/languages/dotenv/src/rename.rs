//! Find Usages and Rename of a key — at the cursor in a `.env` file or where code reads it:
//! the [`RenameContext`] the refactoring module plans from (its preview, applying and undo
//! are the same as any rename's). The places are every `.env` file's definitions and
//! interpolations of the key and every read of it in the code (by the ways the languages'
//! modules contributed); a new name already a key is a conflict.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use code_intelligence::env::EnvReads;
use refactor::NameStyle;
use refactor::application::{
    Conflict, ConflictCheck, RenameContext, RenameTarget, Resolution, Resolver, Scope, Sources,
};
use workbench_model::{CursorContext, CursorSymbols, ProjectQueries};

use crate::catalog::{self, Catalog};
use crate::keys;
use crate::reads::FileReads;

#[derive(Debug, Default)]
pub struct Keys;

impl CursorSymbols for Keys {
    fn rename_context(&self, at: &CursorContext<'_>) -> Option<RenameContext> {
        let registered = code_intelligence::env::reads();
        let name = at.path.file_name()?.to_string_lossy();
        let key = keys::key_at(&registered, at.language, &name, at.text, at.offset)?;
        let queries = at.queries.clone()?;
        let catalog = at.project.and_then(catalog::of)?;
        let root = queries.root();
        let files =
            Arc::new(ProjectFiles { root, queries, open: Arc::clone(&at.open), registered });
        Some(RenameContext {
            target: RenameTarget {
                name: at.text[key].to_owned(),
                style: NameStyle::CODE,
                scope: Scope::Project,
                file: None,
            },
            sources: Arc::clone(&files) as Arc<dyn Sources + Send + Sync>,
            resolver: Arc::new(KeyPlaces { files, last: Mutex::default() }),
            conflicts: Arc::new(DefinedKeys { catalog }),
        })
    }
}

/// The project's files a key may be written in: its `.env` files and the files whose
/// language reads variables (not its libraries'), as in the editor when open.
struct ProjectFiles {
    root: PathBuf,
    queries: Arc<dyn ProjectQueries>,
    open: Arc<HashMap<PathBuf, String>>,
    registered: Vec<EnvReads>,
}

impl ProjectFiles {
    /// The language of `path`, and its name.
    fn language(&self, path: &Path) -> (String, String) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        (self.queries.language_of(path), name)
    }

    /// Whether a key may be written in `path`.
    fn may_hold_keys(&self, path: &Path) -> bool {
        let (language, name) = self.language(path);
        language == crate::ID || !FileReads::of(&self.registered, &language, &name).is_empty()
    }
}

impl Sources for ProjectFiles {
    fn root(&self) -> Option<&Path> {
        Some(&self.root)
    }

    fn candidates(&self) -> Vec<PathBuf> {
        self.queries
            .file_list()
            .iter()
            .filter(|relative| !kernel::in_dependencies_or_git(Path::new(relative.as_str())))
            .map(|relative| self.root.join(relative))
            .filter(|path| self.may_hold_keys(path))
            .collect()
    }

    fn read(&self, path: &Path) -> Option<String> {
        if let Some(text) = self.open.get(path) {
            return Some(text.clone());
        }
        let relative = path.strip_prefix(&self.root).ok()?;
        self.queries.read_source(relative)
    }
}

/// A name found is the key where a key is written (a definition, an interpolation, a read);
/// another name elsewhere. The last file's places are kept (its names are asked in turn).
struct KeyPlaces {
    files: Arc<ProjectFiles>,
    last: Mutex<Option<(PathBuf, Vec<Range<usize>>)>>,
}

impl Resolver for KeyPlaces {
    fn resolve(&self, path: &Path, text: &str, range: Range<usize>) -> Resolution {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if last.as_ref().is_none_or(|(p, _)| p != path) {
            let (language, name) = self.files.language(path);
            let places = keys::places(&self.files.registered, &language, &name, text);
            *last = Some((path.to_path_buf(), places));
        }
        let places = last.as_ref().map_or(&[][..], |(_, p)| p.as_slice());
        match places.contains(&range) {
            true => Resolution::Target,
            false => Resolution::Other,
        }
    }
}

/// A new name already a key of the project's `.env` files.
struct DefinedKeys {
    catalog: Arc<Catalog>,
}

impl ConflictCheck for DefinedKeys {
    fn conflicts(&self, new_name: &str) -> Vec<Conflict> {
        self.catalog
            .all()
            .iter()
            .flat_map(|docs| docs.iter())
            .filter_map(|doc| {
                let (line, _) = doc.defined(new_name)?;
                let line_text = doc.text[line.span.clone()].trim().to_owned();
                Some(Conflict { path: doc.path.clone(), line: line.first, line_text })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/rename.rs"]
mod tests;
