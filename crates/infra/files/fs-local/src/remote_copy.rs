//! A remote project's local copy on disk: the `remote` domain's [`LocalFiles`] port over a
//! folder (paths relative to it, with `/`). The project's data folder (project data folder, the
//! brand's) and the exclusions are left to the caller.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use remote::LocalEntry;
use remote::application::LocalFiles;

#[derive(Debug, Clone)]
pub struct LocalCopy {
    pub root: PathBuf,
    /// Folders never listed (the project's own data folder).
    pub skip: Vec<String>,
}

fn seconds(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

impl LocalCopy {
    /// `relative` in the copy; refused when it would lead out of it: by its name, or through
    /// a link on the way (a folder linked to `~/.ssh` is never read from nor written into).
    fn path(&self, relative: &str) -> std::io::Result<PathBuf> {
        let outside = || {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("outside the project: {relative}"),
            )
        };
        if !remote::is_inside(relative) {
            return Err(outside());
        }
        let mut path = self.root.clone();
        for part in relative.split('/').filter(|p| !p.is_empty()) {
            path.push(part);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(outside());
            }
        }
        Ok(path)
    }

    fn walk(&self, dir: &Path, prefix: &str, out: &mut Vec<LocalEntry>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative =
                if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if self.skip.contains(&relative) {
                continue;
            }
            // Links aren't followed: what they lead to isn't the project's.
            if entry.file_type().is_ok_and(|t| t.is_symlink()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                self.walk(&entry.path(), &relative, out);
            } else if meta.is_file() {
                out.push(LocalEntry { path: relative, size: meta.len(), modified: seconds(&meta) });
            }
        }
    }
}

impl LocalFiles for LocalCopy {
    fn list(&self) -> Vec<LocalEntry> {
        let mut out = Vec::new();
        self.walk(&self.root, "", &mut out);
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.path(path)?)
    }

    fn write(&self, path: &str, data: &[u8]) -> std::io::Result<LocalEntry> {
        // Beside it first (a name of its own: parallel downloads never share it), then in
        // its place (an editor reading it never sees half).
        files::write_atomic(&self.path(path)?, data)?;
        self.stat(path).ok_or_else(|| std::io::ErrorKind::NotFound.into())
    }

    fn remove(&self, path: &str) -> std::io::Result<()> {
        std::fs::remove_file(self.path(path)?)
    }

    fn stat(&self, path: &str) -> Option<LocalEntry> {
        let meta = std::fs::metadata(self.path(path).ok()?).ok()?;
        meta.is_file().then(|| LocalEntry {
            path: path.to_owned(),
            size: meta.len(),
            modified: seconds(&meta),
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/remote_copy.rs"]
mod tests;
