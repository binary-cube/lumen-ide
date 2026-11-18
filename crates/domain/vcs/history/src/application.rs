//! The local history's use cases, over the [`HistoryStore`] port: keeping a version
//! (skipping a repeat of the last one, a file too large or not text), listing a file's
//! versions, comparing one with the current text, and pruning by the retention rules.

use std::path::Path;

use diff::CompareInput;

use crate::{HistoryStore, Retention, Revision, RevisionKind, text_hash};

/// Keeps `text` as `path`'s newest version, unless it's the same as the last one, too large
/// or not text (a NUL byte); the file's versions the retention then expires (past its most
/// per file, too old) dropped with it — a long session saving often stays within it.
/// Returns the version kept.
pub fn keep(
    store: &dyn HistoryStore,
    retention: &Retention,
    path: &str,
    text: &[u8],
    kind: RevisionKind,
    now: u64,
) -> std::io::Result<Option<Revision>> {
    if text.len() as u64 > retention.max_file_size || text.contains(&0) {
        return Ok(None);
    }
    let hash = text_hash(text);
    let revisions = store.revisions(path);
    if revisions.first().is_some_and(|last| last.hash == hash) {
        return Ok(None);
    }
    // Two versions in the same millisecond: the later one a millisecond on.
    let at = revisions.first().map_or(now, |last| now.max(last.at + 1));
    let revision = Revision { at, kind, size: text.len() as u64, hash };
    store.put(path, &revision, text)?;
    let mut all = Vec::with_capacity(revisions.len() + 1);
    all.push(revision.clone());
    all.extend(revisions);
    for old in retention.expired(&all, now) {
        let _ = store.remove(path, old.at);
    }
    Ok(Some(revision))
}

/// Drops what the retention rules expire, in every file. Returns how many versions went.
pub fn prune(store: &dyn HistoryStore, retention: &Retention, now: u64) -> usize {
    let mut dropped = 0;
    for path in store.paths() {
        let revisions = store.revisions(&path);
        for revision in retention.expired(&revisions, now) {
            if store.remove(&path, revision.at).is_ok() {
                dropped += 1;
            }
        }
    }
    dropped
}

/// Version `at` of `path` (left, read-only) compared with the current text (right, the
/// file at `file`). `label`: how the version is named (its date).
pub fn compare_with_current(
    store: &dyn HistoryStore,
    path: &str,
    at: u64,
    file: &Path,
    current: &str,
    label: &str,
) -> Option<CompareInput> {
    let old = String::from_utf8_lossy(&store.read(path, at)?).into_owned();
    let name = file.file_name()?.to_string_lossy().into_owned();
    Some(CompareInput {
        left: (file.to_path_buf(), old),
        right: (file.to_path_buf(), current.to_owned()),
        labels: [Some(format!("{name} · {label}")), Some(name)],
        read_only: [true, false],
    })
}

/// The versions of `path` worth listing (newest first).
pub fn versions(store: &dyn HistoryStore, path: &str) -> Vec<Revision> {
    store.revisions(path)
}

#[cfg(test)]
#[path = "../tests/unit/application.rs"]
mod tests;
