//! New diagrams and folders in the project's diagrams folder (`<project dir>/diagrams/`):
//! under a name not taken ("Untitled.drawio", "Untitled 2.drawio"…), the folder made if
//! missing and the project folder's `.gitignore` letting it through (the diagrams are the
//! project's: committed). Disk work, for a worker.

use std::io;
use std::path::{Path, PathBuf};

use crate::window::FOLDER;

/// The project's diagrams folder.
pub fn folder(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join(FOLDER)
}

/// A path in `dir` not taken: `{base}{ext}`, else `{base} 2{ext}`, `{base} 3{ext}`…
fn free(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{base}{ext}"));
    if !first.exists() {
        return first;
    }
    (2u32..).map(|n| dir.join(format!("{base} {n}{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// The project folder's `.gitignore` lets the diagrams through.
fn let_through(root: &Path) -> io::Result<()> {
    let project_dir = root.join(kernel::brand::PROJECT_DIR);
    std::fs::create_dir_all(&project_dir)?;
    files::ignore::let_through(&project_dir, &[&format!("!{FOLDER}/"), &format!("!{FOLDER}/**")])
}

/// A new, empty diagram (one page) in `dir` (in the project at `root`): its path.
pub fn new_diagram(root: &Path, dir: &Path) -> io::Result<PathBuf> {
    let_through(root)?;
    std::fs::create_dir_all(dir)?;
    let path = free(dir, "Untitled", ".drawio");
    let id = format!("{:032x}", kernel::id::new_uuid());
    let doc = diagram_model::Document::new("Page-1", &id[..20]);
    files::write_atomic(&path, doc.write().as_bytes())?;
    Ok(path)
}

/// A new folder in `dir` (in the project at `root`): its path.
pub fn new_folder(root: &Path, dir: &Path) -> io::Result<PathBuf> {
    let_through(root)?;
    let path = free(dir, "New folder", "");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(test)]
#[path = "../tests/unit/create.rs"]
mod tests;
