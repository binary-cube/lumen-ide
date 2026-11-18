//! Which file a path names, as sources and opened files are matched by: its links resolved
//! through the file system's port ([`workspace::FileSystem::canonical`]), then compared by
//! the platform's rules ([`database::identity`]: on Windows without case nor `\\?\`). One
//! per project (its catalog's), kept in a [`GenerationalCache`] and let go on "Clear and
//! Rebuild Caches" ([`Purge`]). A file that isn't there is known by its nearest folder
//! that is and the rest as written (`.` and `..` resolved), never kept: once made, its
//! own links count.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use database::identity::FileKey;
use kernel::{GenerationalCache, Purge};
use workspace::FileSystem;

/// How many paths are kept (each a few hundred bytes).
const KEPT: usize = 256;

/// The files' identities (see the module's doc).
pub struct FileIdentities {
    fs: Arc<dyn FileSystem + Send + Sync>,
    known: Mutex<GenerationalCache<PathBuf, FileKey>>,
}

impl std::fmt::Debug for FileIdentities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileIdentities").finish_non_exhaustive()
    }
}

impl Default for FileIdentities {
    fn default() -> Self {
        Self::new(Arc::new(fs_local::LocalFileSystem))
    }
}

impl FileIdentities {
    /// Identities read through `fs`.
    pub fn new(fs: Arc<dyn FileSystem + Send + Sync>) -> Self {
        Self { fs, known: Mutex::new(GenerationalCache::new(KEPT)) }
    }

    /// The file `path` names: its links followed (those of its folders when it isn't there).
    pub fn key(&self, path: &Path) -> FileKey {
        if let Some(key) = self.lock().get(path) {
            return key.clone();
        }
        // As written first (`a/x/../b` names `a/b` even when there's no `x`).
        let lexical = FileKey::lexical(path);
        if let Ok(real) = self.fs.canonical(lexical.as_path()) {
            let key = FileKey::lexical(&real);
            self.lock().insert(path.to_path_buf(), key.clone());
            return key;
        }
        // Not there: its nearest folder that is, with the rest as written (never kept).
        let mut rest = Vec::new();
        let mut at = lexical.as_path();
        while let (Some(parent), Some(name)) = (at.parent(), at.file_name()) {
            rest.push(name);
            if let Ok(real) = self.fs.canonical(parent) {
                let mut full = real;
                full.extend(rest.iter().rev());
                return FileKey::lexical(&full);
            }
            at = parent;
        }
        lexical
    }

    /// `a` and `b` name one file.
    pub fn same(&self, a: &Path, b: &Path) -> bool {
        a == b || self.key(a) == self.key(b)
    }

    /// How many paths are kept.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Every path kept let go (shared: the catalog's, through `&`).
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GenerationalCache<PathBuf, FileKey>> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Purge for FileIdentities {
    fn purge(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[path = "../tests/unit/identity.rs"]
mod tests;
