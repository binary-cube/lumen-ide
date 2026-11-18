//! A project's recently opened files (`<project dir>/recent-files`: one path per line,
//! relative to the project, the latest last), for "Recent Files".

use std::io;
use std::path::{Path, PathBuf};

/// The recent files kept in each project's folder (the [`workspace::RecentFiles`] port).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecentFilesFile;

impl workspace::RecentFiles for RecentFilesFile {
    fn load(&self, root: &Path) -> Vec<PathBuf> {
        load(root)
    }

    fn save(&self, root: &Path, files: &[PathBuf]) -> io::Result<()> {
        save(root, files)
    }
}

fn file(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join("recent-files")
}

/// The project's recent files (absolute), the latest last; none when never saved.
pub fn load(root: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(file(root)) else { return Vec::new() };
    text.lines().filter(|l| !l.is_empty()).map(|l| root.join(l)).collect()
}

/// Keeps the project's recent files (those inside it, relative), written whole at once.
pub fn save(root: &Path, files: &[PathBuf]) -> io::Result<()> {
    let lines: Vec<String> = files
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.display().to_string())
        .collect();
    let path = file(root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::write_atomic(&path, &lines.join("\n"))
}

#[cfg(test)]
#[path = "../tests/unit/recent_files.rs"]
mod tests;
