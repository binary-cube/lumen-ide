//! A folder measured (on a worker): every file's size added up, folder by folder. Links
//! are never followed (a link counts as itself), a folder that can't be read is skipped and
//! counted; the walk has limits — so deep (deeper folders add to the one at the limit, not
//! listed apart), so many entries (then it stops, marked partial) — and stops when asked
//! (its tab closed). What's kept per folder: its sub-folders, biggest first (the smallest
//! past a count summed as one "other" entry), and its biggest files (the rest summed).

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// The deepest folder listed apart (its JSON stays under the reader's nesting limit).
pub const DEPTH_MOST: usize = 40;
/// The most entries looked at in one measure.
pub const ENTRIES_MOST: u64 = 20_000_000;
/// The most sub-folders and files listed in a folder (the rest summed).
pub const FOLDERS_MOST: usize = 300;
pub const FILES_MOST: usize = 25;

/// A folder's or a file's size, and what's under it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub dir: bool,
    /// Several entries summed as one (past the counts listed).
    #[serde(default)]
    pub other: bool,
    /// Bytes, everything under it.
    pub size: u64,
    /// Files under it.
    pub files: u64,
    #[serde(default)]
    pub children: Vec<Node>,
}

/// How far a measure has come (read by its tab as it draws) and whether to stop.
#[derive(Debug, Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub entries: AtomicU64,
    /// Folders that couldn't be read.
    pub unreadable: AtomicU64,
    pub stop: AtomicBool,
}

impl Progress {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.entries.load(Ordering::Relaxed) >= ENTRIES_MOST
    }
}

/// `root` measured. `None`: it can't be read at all.
pub fn measure(root: &Path, progress: &Progress) -> Option<Node> {
    std::fs::read_dir(root).ok()?;
    let name = kernel::paths::file_name(root);
    Some(folder(root, name, 0, progress))
}

fn folder(path: &Path, name: String, depth: usize, progress: &Progress) -> Node {
    let mut node = Node { name, dir: true, ..Node::default() };
    let Ok(entries) = std::fs::read_dir(path) else {
        progress.unreadable.fetch_add(1, Ordering::Relaxed);
        return node;
    };
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        if progress.stopped() {
            break;
        }
        progress.entries.fetch_add(1, Ordering::Relaxed);
        // The entry itself (a link isn't followed).
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            let child = folder(
                &entry.path(),
                entry.file_name().to_string_lossy().into_owned(),
                depth + 1,
                progress,
            );
            node.size = node.size.saturating_add(child.size);
            node.files = node.files.saturating_add(child.files);
            // Past the depth listed: summed into this one only.
            if depth < DEPTH_MOST {
                folders.push(child);
            }
        } else {
            let size = entry.metadata().map_or(0, |m| m.len());
            progress.files.fetch_add(1, Ordering::Relaxed);
            progress.bytes.fetch_add(size, Ordering::Relaxed);
            node.size = node.size.saturating_add(size);
            node.files = node.files.saturating_add(1);
            files.push(Node {
                name: entry.file_name().to_string_lossy().into_owned(),
                size,
                files: 1,
                ..Node::default()
            });
        }
    }
    node.children = kept(folders, FOLDERS_MOST, true);
    node.children.extend(kept(files, FILES_MOST, false));
    node.children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    node
}

/// The biggest `most` of `nodes`, the rest summed as one entry.
fn kept(mut nodes: Vec<Node>, most: usize, dir: bool) -> Vec<Node> {
    nodes.sort_by_key(|n| std::cmp::Reverse(n.size));
    if nodes.len() <= most {
        return nodes;
    }
    let rest = nodes.split_off(most);
    let other = Node {
        name: rest.len().to_string(),
        dir,
        other: true,
        size: rest.iter().map(|n| n.size).fold(0, u64::saturating_add),
        files: rest.iter().map(|n| n.files).fold(0, u64::saturating_add),
        children: Vec::new(),
    };
    nodes.push(other);
    nodes
}

#[cfg(test)]
#[path = "../tests/unit/scan.rs"]
pub(crate) mod tests;
