//! The local history's store on disk ([`HistoryStore`]): under the project's data folder
//! (outside the project: nothing to commit or ignore), one folder per file, named by a
//! hash of its path (the path itself in a `path` file inside), its versions gzip-compressed
//! and named `<time>-<kind>-<hash>.gz` — listing reads only names. Writes go through a
//! temporary file (a crash never leaves half a version).

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use history::{HistoryStore, Revision, RevisionKind, text_hash};

#[derive(Debug, Clone)]
pub struct DiskHistory {
    dir: PathBuf,
    /// The bytes it wrote to the disk (compressed versions, their markers), counted as it
    /// goes (the Task Manager's).
    written: Arc<AtomicU64>,
}

impl DiskHistory {
    /// The store in `dir` (created when the first version is kept).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), written: Arc::default() }
    }

    /// The bytes it wrote so far (shared by its clones).
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    fn write(&self, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
        write_atomic(target, bytes)?;
        self.written.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    fn file_dir(&self, path: &str) -> PathBuf {
        self.dir.join(format!("{:016x}", text_hash(path.as_bytes())))
    }

    fn version_path(&self, path: &str, revision: &Revision) -> PathBuf {
        self.file_dir(path).join(name_of(revision))
    }
}

/// `<time>-<kind>-<hash>.gz`.
pub fn name_of(revision: &Revision) -> String {
    format!("{}-{}-{:016x}-{}.gz", revision.at, revision.kind.code(), revision.hash, revision.size)
}

/// A version from its file name.
pub fn parse_name(name: &str) -> Option<Revision> {
    let stem = name.strip_suffix(".gz")?;
    let mut parts = stem.split('-');
    let at = parts.next()?.parse().ok()?;
    let kind = RevisionKind::from_code(parts.next()?.chars().next()?)?;
    let hash = u64::from_str_radix(parts.next()?, 16).ok()?;
    let size = parts.next()?.parse().ok()?;
    Some(Revision { at, kind, size, hash })
}

fn write_atomic(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    files::write_atomic(target, bytes)
}

impl HistoryStore for DiskHistory {
    fn put(&self, path: &str, revision: &Revision, text: &[u8]) -> std::io::Result<()> {
        let dir = self.file_dir(path);
        std::fs::create_dir_all(&dir)?;
        let marker = dir.join("path");
        if !marker.exists() {
            self.write(&marker, path.as_bytes())?;
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(text)?;
        self.write(&self.version_path(path, revision), &encoder.finish()?)
    }

    fn revisions(&self, path: &str) -> Vec<Revision> {
        let Ok(entries) = std::fs::read_dir(self.file_dir(path)) else { return Vec::new() };
        let mut revisions: Vec<Revision> = entries
            .flatten()
            .filter_map(|e| parse_name(&e.file_name().to_string_lossy()))
            .collect();
        revisions.sort_by_key(|r| std::cmp::Reverse(r.at));
        revisions
    }

    fn read(&self, path: &str, at: u64) -> Option<Vec<u8>> {
        let revision = self.revisions(path).into_iter().find(|r| r.at == at)?;
        let compressed = std::fs::read(self.version_path(path, &revision)).ok()?;
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(compressed.as_slice()).read_to_end(&mut text).ok()?;
        Some(text)
    }

    fn remove(&self, path: &str, at: u64) -> std::io::Result<()> {
        let Some(revision) = self.revisions(path).into_iter().find(|r| r.at == at) else {
            return Ok(());
        };
        std::fs::remove_file(self.version_path(path, &revision))?;
        // The file's last version gone: its folder too.
        if self.revisions(path).is_empty() {
            let _ = std::fs::remove_dir_all(self.file_dir(path));
        }
        Ok(())
    }

    fn paths(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        entries
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path().join("path")).ok())
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
