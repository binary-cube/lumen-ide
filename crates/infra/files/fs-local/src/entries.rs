//! The `workspace` domain's [`EntryStore`] port over the local disk: copies (with their
//! progress and a stop), moves (a rename, or a copy then a removal across disks), the
//! system's trash, new files and folders.

use std::io;
use std::path::{Path, PathBuf};

use workspace::entries::EntryStore;

/// The local disk.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalEntries;

impl EntryStore for LocalEntries {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn count_files(&self, path: &Path) -> u64 {
        crate::measure(path).0
    }

    fn copy(
        &self,
        from: &Path,
        to: &Path,
        copied: &mut dyn FnMut(u64),
        stop: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        crate::copy_recursive_with(from, to, copied, stop)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        crate::move_entry(from, to)
    }

    fn trash(&self, paths: &[PathBuf]) -> io::Result<()> {
        crate::move_to_trash(paths)
    }

    fn create_file(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::OpenOptions::new().write(true).create_new(true).open(path).map(drop)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }
}
