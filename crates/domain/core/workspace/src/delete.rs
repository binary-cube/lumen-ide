//! Deleting project entries: which of the chosen paths are actually deleted (never the
//! project's root or anything outside it; an entry inside a chosen folder goes with it),
//! and whether an open file was among them.

use std::path::{Path, PathBuf};

/// The entries to delete: inside `root` (not the root itself), without the ones already
/// inside another chosen folder; in the order given.
pub fn deletion_roots(paths: &[PathBuf], root: &Path) -> Vec<PathBuf> {
    let inside: Vec<&PathBuf> =
        paths.iter().filter(|p| p.starts_with(root) && p.as_path() != root).collect();
    let mut out: Vec<PathBuf> = Vec::new();
    for path in &inside {
        let covered = inside.iter().any(|other| other != path && path.starts_with(other));
        if !covered && !out.contains(path) {
            out.push((*path).clone());
        }
    }
    out
}

/// `file` was deleted with one of `deleted` (it, or a folder containing it).
pub fn was_deleted(file: &Path, deleted: &[PathBuf]) -> bool {
    deleted.iter().any(|d| file.starts_with(d))
}

#[cfg(test)]
#[path = "../tests/unit/delete.rs"]
mod tests;
