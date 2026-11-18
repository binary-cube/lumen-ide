//! The project's file tree.
//!
//! Directories are loaded lazily (on first expansion), and the list of visible rows
//! is recomputed only after a change, not on every frame.

use std::path::{Path, PathBuf};

/// Names always hidden from the tree (until the exclusion settings arrive).
const ALWAYS_HIDDEN: &[&str] = &[".git", ".DS_Store"];

/// A node of the tree: its place in the arena and which of the nodes that place held it is
/// (a place freed by an entry gone is taken by another, under a new generation: an id kept
/// from before isn't mistaken for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Directory,
    File,
}

/// Where an entry's copy stands against another one a module keeps it with (a remote
/// project's server), drawn beside its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mark {
    /// Nothing to say (the same on both sides, or no other side).
    #[default]
    None,
    /// Only here: never sent.
    Local,
    /// Changed here since the last transfer: to send.
    Changed,
    /// Changed on the other side since the last transfer: to bring.
    Behind,
    /// Changed on both sides.
    Conflict,
}

/// What a module says of an entry, shown muted on its row (and in its tooltip): a remote
/// file's permissions and owner (`rw-r--r-- www-data`); `warn`: something to look at (the
/// web server can't read it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub text: std::sync::Arc<str>,
    pub warn: bool,
}

/// An entry in a directory, as reported by the file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: NodeKind,
    /// Known to be there without being on disk yet (a remote project's file, not fetched):
    /// shown, brought when opened.
    pub absent: bool,
    pub mark: Mark,
    pub note: Option<Note>,
}

impl DirEntry {
    pub fn new(name: impl Into<String>, kind: NodeKind) -> Self {
        Self { name: name.into(), kind, absent: false, mark: Mark::None, note: None }
    }

    /// An entry that isn't on disk yet.
    pub fn absent(name: impl Into<String>, kind: NodeKind) -> Self {
        Self { absent: true, ..Self::new(name, kind) }
    }

    pub fn marked(self, mark: Mark) -> Self {
        Self { mark, ..self }
    }

    pub fn noted(self, note: Option<Note>) -> Self {
        Self { note, ..self }
    }
}

/// A displayed row: the node and its depth (the root has 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRow {
    pub id: NodeId,
    pub depth: u16,
}

#[derive(Debug)]
struct Node {
    /// Which of the nodes its place held it is (see [`NodeId`]).
    generation: u32,
    /// Its place is free (the entry gone), to be taken by the next one.
    free: bool,
    name: String,
    kind: NodeKind,
    parent: Option<NodeId>,
    /// `None` = directory not read yet.
    children: Option<Vec<NodeId>>,
    expanded: bool,
    /// Not on disk yet (see [`DirEntry::absent`]).
    absent: bool,
    mark: Mark,
    note: Option<Note>,
}

#[derive(Debug)]
pub struct FileTree {
    root_path: PathBuf,
    nodes: Vec<Node>,
    /// The places free (their entries gone), taken first by new ones.
    free: Vec<u32>,
    visible: Vec<VisibleRow>,
    visible_dirty: bool,
}

impl FileTree {
    /// A tree rooted at `root_path`, expanded but not read yet.
    pub fn new(root_path: PathBuf) -> Self {
        let name = root_path
            .file_name()
            .map_or_else(|| root_path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let root = Node {
            generation: 0,
            free: false,
            name,
            kind: NodeKind::Directory,
            parent: None,
            children: None,
            expanded: true,
            absent: false,
            mark: Mark::None,
            note: None,
        };
        Self {
            root_path,
            nodes: vec![root],
            free: Vec::new(),
            visible: Vec::new(),
            visible_dirty: true,
        }
    }

    /// `id` is one of the tree's entries now (not one gone since it was handed out).
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.get(id.index as usize).is_some_and(|n| !n.free && n.generation == id.generation)
    }

    /// The id of the node in place `index`.
    fn id_at(&self, index: usize) -> NodeId {
        NodeId { index: index as u32, generation: self.nodes[index].generation }
    }

    /// A new node: in a free place if there's one (under a new generation), else at the end.
    fn add(&mut self, mut node: Node) -> NodeId {
        match self.free.pop() {
            Some(index) => {
                let slot = &mut self.nodes[index as usize];
                let generation = slot.generation;
                node.generation = generation;
                *slot = node;
                NodeId { index, generation }
            }
            None => {
                self.nodes.push(node);
                self.id_at(self.nodes.len() - 1)
            }
        }
    }

    /// `id` and everything under it gone: their places freed (what they held let go), their
    /// ids no longer the tree's.
    fn remove(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            if !self.contains(id) {
                continue;
            }
            let node = &mut self.nodes[id.index as usize];
            if let Some(children) = node.children.take() {
                stack.extend(children);
            }
            node.free = true;
            node.generation = node.generation.wrapping_add(1);
            node.name = String::new();
            node.note = None;
            node.parent = None;
            self.free.push(id.index);
        }
    }

    /// Child `name` of directory `dir` (if the directory was read).
    pub fn child_named(&self, dir: NodeId, name: &str) -> Option<NodeId> {
        if !self.contains(dir) {
            return None;
        }
        let children = self.nodes.get(dir.index as usize)?.children.as_ref()?;
        children.iter().copied().find(|c| self.nodes[c.index as usize].name == name)
    }

    pub fn root(&self) -> NodeId {
        self.id_at(0)
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn name(&self, id: NodeId) -> &str {
        &self.node(id).name
    }

    pub fn kind(&self, id: NodeId) -> NodeKind {
        self.node(id).kind
    }

    /// Not on disk yet (a remote project's entry, not fetched).
    pub fn is_absent(&self, id: NodeId) -> bool {
        self.node(id).absent
    }

    pub fn mark(&self, id: NodeId) -> Mark {
        self.node(id).mark
    }

    /// What a module says of it (see [`Note`]).
    pub fn note(&self, id: NodeId) -> Option<&Note> {
        self.node(id).note.as_ref()
    }

    pub fn is_expanded(&self, id: NodeId) -> bool {
        self.node(id).expanded
    }

    /// The directory is expanded but its contents haven't been read yet.
    pub fn needs_loading(&self, id: NodeId) -> bool {
        let node = self.node(id);
        node.kind == NodeKind::Directory && node.children.is_none()
    }

    /// The node's absolute path (one gone from the tree: an empty path, which nothing is).
    pub fn path(&self, id: NodeId) -> PathBuf {
        if !self.contains(id) {
            return PathBuf::new();
        }
        let mut names = Vec::new();
        let mut current = Some(id);
        while let Some(n) = current {
            let node = self.node(n);
            if node.parent.is_some() {
                names.push(node.name.as_str());
            }
            current = node.parent;
        }
        let mut path = self.root_path.clone();
        path.extend(names.iter().rev());
        path
    }

    /// Sets a directory's contents: directories first, then files, each group sorted
    /// alphabetically, case-insensitively. On a re-read, the entries that already existed
    /// (same name and kind) keep their node, and therefore their state (expanded, children
    /// read).
    pub fn set_children(&mut self, dir: NodeId, mut entries: Vec<DirEntry>) {
        entries.retain(|e| !ALWAYS_HIDDEN.contains(&e.name.as_str()));
        // Each name lowercased once (not twice per comparison: a folder of thousands).
        entries.sort_by_cached_key(|e| {
            (e.kind == NodeKind::File, e.name.to_lowercase(), e.name.clone())
        });
        if !self.contains(dir) {
            return;
        }
        let old = self.nodes[dir.index as usize].children.take().unwrap_or_default();
        // The children kept, found by name (a folder of thousands re-read: not a search of
        // all of them for each).
        let mut kept: std::collections::HashMap<(&str, NodeKind), NodeId> = old
            .iter()
            .map(|&id| {
                let node = &self.nodes[id.index as usize];
                ((node.name.as_str(), node.kind), id)
            })
            .collect();
        let found: Vec<Option<NodeId>> =
            entries.iter().map(|e| kept.remove(&(e.name.as_str(), e.kind))).collect();
        // What's no longer there, with all under it: their places freed first (the new
        // entries take them).
        let gone: Vec<NodeId> = kept.into_values().collect();
        for id in gone {
            self.remove(id);
        }
        let mut ids = Vec::with_capacity(entries.len());
        for (entry, existing) in entries.into_iter().zip(found) {
            if let Some(id) = existing {
                // Brought to disk meanwhile (or gone from it).
                let node = &mut self.nodes[id.index as usize];
                node.absent = entry.absent;
                node.mark = entry.mark;
                node.note = entry.note;
                ids.push(id);
                continue;
            }
            ids.push(self.add(Node {
                generation: 0,
                free: false,
                name: entry.name,
                kind: entry.kind,
                parent: Some(dir),
                children: None,
                expanded: false,
                absent: entry.absent,
                mark: entry.mark,
                note: entry.note,
            }));
        }
        self.nodes[dir.index as usize].children = Some(ids);
        self.visible_dirty = true;
    }

    /// The node at the given path, if already loaded in the tree.
    pub fn find(&self, path: &Path) -> Option<NodeId> {
        let relative = path.strip_prefix(&self.root_path).ok()?;
        let mut node = self.root();
        for component in relative.components() {
            node = self.child_named(node, &component.as_os_str().to_string_lossy())?;
        }
        Some(node)
    }

    /// The directories already read (their entries known), the root first: what a look at
    /// everything again reads (the others are read when opened).
    pub fn loaded_dirs(&self) -> Vec<PathBuf> {
        (0..self.nodes.len())
            .map(|i| self.id_at(i))
            .filter(|id| self.is_loaded(*id) && self.kind(*id) == NodeKind::Directory)
            .map(|id| self.path(id))
            .collect()
    }

    /// The directory has already been read (has known children).
    pub fn is_loaded(&self, dir: NodeId) -> bool {
        self.contains(dir) && self.nodes[dir.index as usize].children.is_some()
    }

    pub fn set_expanded(&mut self, dir: NodeId, expanded: bool) {
        if !self.contains(dir) {
            return;
        }
        let node = &mut self.nodes[dir.index as usize];
        if node.kind == NodeKind::Directory && node.expanded != expanded {
            node.expanded = expanded;
            self.visible_dirty = true;
        }
    }

    /// Collapses all directories (the root stays open).
    pub fn collapse_all(&mut self) {
        for node in self.nodes.iter_mut().skip(1) {
            node.expanded = false;
        }
        self.visible_dirty = true;
    }

    /// Expands all directories already read (no new disk reads, so it's fast even on
    /// large projects: unread `vendor/` or `node_modules/` stay closed).
    /// `true` if one was opened.
    pub fn expand_loaded(&mut self) -> bool {
        let mut opened = false;
        for node in &mut self.nodes {
            if node.kind == NodeKind::Directory && node.children.is_some() && !node.expanded {
                node.expanded = true;
                opened = true;
            }
        }
        self.visible_dirty |= opened;
        opened
    }

    /// The visible rows, in display order (recomputed only after a change).
    pub fn visible_rows(&mut self) -> &[VisibleRow] {
        self.refresh_visible();
        &self.visible
    }

    /// Recomputes the visible rows if the tree changed since the last call.
    pub fn refresh_visible(&mut self) {
        if !self.visible_dirty {
            return;
        }
        self.visible.clear();
        let mut stack = vec![VisibleRow { id: self.root(), depth: 0 }];
        while let Some(row) = stack.pop() {
            self.visible.push(row);
            let node = &self.nodes[row.id.index as usize];
            if let (true, Some(children)) = (node.expanded, &node.children) {
                stack.extend(
                    children.iter().rev().map(|&id| VisibleRow { id, depth: row.depth + 1 }),
                );
            }
        }
        self.visible_dirty = false;
    }

    /// The rows computed at the last [`FileTree::refresh_visible`]; allows reading them
    /// together with the rest of the tree, without a mutable borrow.
    pub fn rows(&self) -> &[VisibleRow] {
        &self.visible
    }

    /// The node `id` is; one gone from the tree reads as nothing (no name, a file with no
    /// parent): never another entry that took its place.
    fn node(&self, id: NodeId) -> &Node {
        match self.nodes.get(id.index as usize) {
            Some(node) if !node.free && node.generation == id.generation => node,
            _ => &GONE,
        }
    }
}

/// What an id no longer the tree's reads as.
static GONE: Node = Node {
    generation: 0,
    free: true,
    name: String::new(),
    kind: NodeKind::File,
    parent: None,
    children: None,
    expanded: false,
    absent: false,
    mark: Mark::None,
    note: None,
};

#[cfg(test)]
#[path = "../tests/unit/domain.rs"]
mod tests;
