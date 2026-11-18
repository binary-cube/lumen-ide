//! Use cases of the Workspace context and their ports.

use std::io;
use std::path::{Path, PathBuf};

use crate::domain::{DirEntry, FileTree, NodeId, NodeKind};

/// The port to the file system. Implemented in infrastructure (`fs-local`).
pub trait FileSystem {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// What is at `path`, a link followed (`None`: nothing, or it can't be told). By default
    /// from its folder's listing; an adapter that can ask directly answers it itself.
    fn kind(&self, path: &Path) -> Option<NodeKind> {
        let name = path.file_name()?.to_string_lossy();
        let entries = self.read_dir(path.parent()?).ok()?;
        entries.into_iter().find(|e| e.name == name && !e.absent).map(|e| e.kind)
    }

    /// `path`'s size and modification time (`None`: not there, or not known: it then never
    /// looks changed).
    fn stamp(&self, _path: &Path) -> Option<Stamp> {
        None
    }

    /// `path` absolute, its links and `..` resolved (an error: nothing there). By default
    /// as given.
    fn canonical(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

/// A file's size and modification time, as the file system tells them.
pub type Stamp = (u64, std::time::SystemTime);

/// A folder as listed: what's on disk (`on_disk`; `None`: not there) and what's known to be
/// there without being on disk yet (`known`, e.g. a remote project's files not fetched;
/// `None`: nothing known of this folder — `Some` empty: known, its contents still coming),
/// those added as absent. The disk wins for a name in both, taking the known entry's
/// [`Mark`](crate::domain::Mark) and [`Note`](crate::domain::Note). `None`: neither has it.
pub fn merge_listing(
    on_disk: Option<Vec<DirEntry>>,
    known: Option<Vec<DirEntry>>,
) -> Option<Vec<DirEntry>> {
    if on_disk.is_none() && known.is_none() {
        return None;
    }
    let mut entries = on_disk.unwrap_or_default();
    // The disk's entries by name (a remote folder of thousands: not a search per known one).
    let mut at: std::collections::HashMap<String, usize> =
        entries.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
    for entry in known.unwrap_or_default() {
        match at.get(&entry.name) {
            Some(&i) => {
                entries[i].mark = entry.mark;
                entries[i].note = entry.note;
            }
            None => {
                at.insert(entry.name.clone(), entries.len());
                entries.push(DirEntry { absent: true, ..entry });
            }
        }
    }
    Some(entries)
}

/// Opens the project at `root` and reads the first level.
pub fn open_project(root: PathBuf, fs: &dyn FileSystem) -> io::Result<FileTree> {
    let entries = fs.read_dir(&root)?;
    let mut tree = FileTree::new(root);
    tree.set_children(tree.root(), entries);
    Ok(tree)
}

/// Expands or collapses a directory, reading it from disk on first expansion.
pub fn toggle_directory(tree: &mut FileTree, dir: NodeId, fs: &dyn FileSystem) -> io::Result<()> {
    if tree.kind(dir) != NodeKind::Directory {
        return Ok(());
    }
    if tree.is_expanded(dir) {
        tree.set_expanded(dir, false);
        return Ok(());
    }
    if tree.needs_loading(dir) {
        let entries = fs.read_dir(&tree.path(dir))?;
        tree.set_children(dir, entries);
    }
    tree.set_expanded(dir, true);
    Ok(())
}

/// "Expand all": opens the folders already read; when none is left closed, goes one level
/// deeper — the closed folders shown are read and opened. A level per click, so a large
/// `vendor/` is never read whole at once (and the button always does something while there
/// are folders to open). `true` if the tree changed.
pub fn expand_more(tree: &mut FileTree, fs: &dyn FileSystem) -> io::Result<bool> {
    if tree.expand_loaded() {
        return Ok(true);
    }
    let shown: Vec<NodeId> = tree.visible_rows().iter().map(|r| r.id).collect();
    let closed: Vec<NodeId> = shown
        .into_iter()
        .filter(|&id| tree.kind(id) == NodeKind::Directory && !tree.is_expanded(id))
        .collect();
    for &dir in &closed {
        toggle_directory(tree, dir, fs)?;
    }
    Ok(!closed.is_empty())
}

/// Re-reads the directory touched by a change on disk (`path` is the file or directory
/// created, modified or deleted; the directory containing it is re-read). Only if that
/// directory is already loaded. `true` if the tree changed.
pub fn refresh_path(tree: &mut FileTree, path: &Path, fs: &dyn FileSystem) -> io::Result<bool> {
    match path.parent() {
        Some(dir) => refresh_dir(tree, dir, fs),
        None => Ok(false),
    }
}

/// Re-reads the directories a batch of changes on disk touched, each once (a checkout or an
/// install can change thousands of files in the same folder): every path's directory, and
/// with `own`, a changed directory's own entries too. Only the loaded ones are read. `true`
/// if the tree changed; the first error is returned after all were tried.
pub fn refresh_paths(
    tree: &mut FileTree,
    paths: &[std::path::PathBuf],
    own: bool,
    fs: &dyn FileSystem,
) -> io::Result<bool> {
    let mut dirs: Vec<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
    if own {
        dirs.extend(paths.iter().map(std::path::PathBuf::as_path));
    }
    dirs.sort_unstable();
    dirs.dedup();
    let mut changed = false;
    let mut first_error = None;
    for dir in dirs {
        match refresh_dir(tree, dir, fs) {
            Ok(c) => changed |= c,
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    match first_error {
        Some(e) if !changed => Err(e),
        _ => Ok(changed),
    }
}

/// Re-reads directory `dir` itself (after entries were put into or taken out of it), only
/// if it's already loaded. `true` if the tree changed.
pub fn refresh_dir(tree: &mut FileTree, dir: &Path, fs: &dyn FileSystem) -> io::Result<bool> {
    let Some(node) = tree.find(dir) else { return Ok(false) };
    if tree.kind(node) != NodeKind::Directory || !tree.is_loaded(node) {
        return Ok(false);
    }
    let entries = fs.read_dir(dir)?;
    tree.set_children(node, entries);
    Ok(true)
}

/// Finds `path` in the tree, reading and expanding the directories along the way.
/// `None` if the file isn't in the project.
pub fn reveal_path(
    tree: &mut FileTree,
    path: &Path,
    fs: &dyn FileSystem,
) -> io::Result<Option<NodeId>> {
    let Ok(relative) = path.strip_prefix(tree.root_path()) else { return Ok(None) };
    let mut node = tree.root();
    for component in relative.components() {
        if tree.kind(node) == NodeKind::Directory && !tree.is_expanded(node) {
            toggle_directory(tree, node, fs)?;
        }
        let name = component.as_os_str().to_string_lossy();
        match tree.child_named(node, &name) {
            Some(child) => node = child,
            None => return Ok(None),
        }
    }
    Ok(Some(node))
}

#[cfg(test)]
#[path = "../tests/unit/application.rs"]
mod tests;
