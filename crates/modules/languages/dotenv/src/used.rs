//! The keys a project's code reads, file by file — for the unused keys' notes: its files
//! walked (not its libraries', not what's never listed), those whose language reads
//! variables read, each again only once it changed on disk (its size or time). Run off the
//! UI thread.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use workspace::marks::PathMarks;

use code_intelligence::env::EnvReads;
use workbench_model::LanguageOf;

use crate::reads::{FileReads, reads};

/// A file as it was read: its stamp on disk and the keys it reads.
#[derive(Debug, Clone)]
struct Seen {
    stamp: (u64, Option<SystemTime>),
    keys: Vec<String>,
}

/// What the project's files read, as last walked.
#[derive(Debug, Default)]
pub struct UsedKeys {
    files: HashMap<PathBuf, Seen>,
}

impl UsedKeys {
    /// Walks the project at `root` again: what's new or changed read, what's gone dropped.
    pub fn refresh(
        &mut self,
        root: &Path,
        marks: &PathMarks,
        language_of: &LanguageOf,
        registered: &[EnvReads],
    ) {
        let mut found = Vec::new();
        walk(root, root, marks, &mut found);
        let mut files = HashMap::with_capacity(found.len());
        for (path, stamp) in found {
            let name =
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(language) = language_of(&path) else { continue };
            let file = FileReads::of(registered, &language, &name);
            if file.is_empty() {
                continue;
            }
            let seen = match self.files.remove(&path) {
                Some(seen) if seen.stamp == stamp => seen,
                _ => {
                    let Ok(text) = std::fs::read_to_string(&path) else { continue };
                    let keys = reads(&text, &file.accesses).into_iter().map(|r| text[r].to_owned());
                    Seen { stamp, keys: keys.collect() }
                }
            };
            files.insert(path, seen);
        }
        self.files = files;
    }

    /// Every key some file reads.
    pub fn keys(&self) -> HashSet<&str> {
        self.files.values().flat_map(|s| s.keys.iter().map(String::as_str)).collect()
    }
}

/// The files under `dir` (with their stamps): not in folders never listed, not the
/// libraries', none too big to be code.
fn walk(
    root: &Path,
    dir: &Path,
    marks: &PathMarks,
    out: &mut Vec<(PathBuf, (u64, Option<SystemTime>))>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if out.len() >= workspace::search::FILE_LIMIT {
            return;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if kernel::in_dependencies_or_git(relative)
            || marks.is_excluded(&relative.to_string_lossy().replace('\\', "/"))
        {
            continue;
        }
        if kind.is_dir() {
            if !workspace::search::skipped_dir(&entry.file_name().to_string_lossy()) {
                walk(root, &path, marks, out);
            }
        } else if let Ok(meta) = entry.metadata()
            && meta.len() <= workspace::MAX_SOURCE_FILE
        {
            out.push((path, (meta.len(), meta.modified().ok())));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/used.rs"]
mod tests;
