//! Copying and moving project entries (the tree's Cut / Copy / Paste): where a pasted entry
//! lands, under a free name, and what can't be pasted where.

use std::path::{Path, PathBuf};

/// Where `source` is pasted into `dir`: its own name, or — when taken — `name copy.ext`,
/// `name copy 2.ext`… (a folder, `is_dir`: `name copy`).
pub fn paste_target(
    source: &Path,
    is_dir: bool,
    dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    let name = kernel::paths::file_name(source);
    let first = dir.join(&name);
    if !exists(&first) {
        return first;
    }
    // The last extension stays at the end; a dotfile (`.env`) or a folder has none.
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !is_dir => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    (1..)
        .map(|n| {
            let suffix = if n == 1 { " copy".to_owned() } else { format!(" copy {n}") };
            dir.join(format!("{stem}{suffix}{ext}"))
        })
        .find(|p| !exists(p))
        .unwrap_or(first)
}

/// Why `source` can't be pasted into `dir`, if it can't: a folder into itself or one of its
/// own folders.
pub fn paste_forbidden(source: &Path, dir: &Path) -> bool {
    dir.starts_with(source)
}

/// `path` after `from` was moved to `to` (itself, or something inside it); `None` if it's
/// elsewhere.
pub fn moved_path(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) })
}

#[cfg(test)]
#[path = "../tests/unit/clipboard.rs"]
mod tests;
