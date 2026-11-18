//! A project's requests as the HTTP window and a collection's tab list them (plan 77i): a
//! tree of collections — folders, at any depth, the empty ones too — and their requests
//! (each file's, its name else its method and URL; the file itself isn't a level). The
//! files are read on a worker from the project's file list (the path index); the folders
//! of [`COLLECTIONS`] from the disk (a bounded walk: an empty collection has no file to be
//! indexed by); kept until the project's files change or the list made something.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A request of a file, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub label: String,
    pub method: String,
    /// Its request line (0-based).
    pub line: u32,
    /// Its block's fingerprint (`draft::block_hash`): what finds it again in its file before
    /// it is moved, copied or removed, wherever it went meanwhile.
    pub hash: u64,
}

/// A file of requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFile {
    /// Relative to the project's root, with `/`.
    pub path: String,
    pub requests: Vec<Listed>,
}

impl RequestFile {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(dir, _)| dir)
    }
}

/// The most files read (a project with more lists these: the rest by searching).
pub const MOST: usize = 500;

/// The project's request files (`list`: its files, relative), each read by `read` and
/// parsed, by path.
pub fn read(
    list: &[String],
    extensions: &[&str],
    read: &dyn Fn(&Path) -> Option<String>,
) -> Vec<RequestFile> {
    let mut out: Vec<RequestFile> = list
        .iter()
        .filter(|f| {
            Path::new(f.as_str())
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
        })
        .take(MOST)
        .filter_map(|path| {
            let text = read(&PathBuf::from(path))?;
            let parsed = http_requests::parse(&text);
            let requests = parsed
                .requests
                .iter()
                .map(|r| Listed {
                    label: r.label(),
                    method: r.method.clone(),
                    line: r.line,
                    hash: http_requests::draft::block_hash(
                        &text[http_requests::draft::block_of(&text, r)],
                    ),
                })
                .collect();
            Some(RequestFile { path: path.clone(), requests })
        })
        .collect();
    // By folder, then name: a folder's files together (`a/b`, `a/e` before `a/c/d`).
    out.sort_by(|a, b| (a.folder(), a.name()).cmp(&(b.folder(), b.name())));
    out
}

/// The folder collections are made in (at the project's root).
pub const COLLECTIONS: &str = "http";

/// The most entries [`walk`] lists, and how deep it goes.
pub const MOST_FOLDERS: usize = 2_000;
pub const DEEPEST: usize = 8;

/// What's under folder `dir` (relative to `root`, with `/`): its folders (itself first, an
/// empty one too: a collection that holds nothing yet) and its request files (one just made
/// isn't indexed yet). Links aren't followed, hidden entries skipped; at most
/// [`MOST_FOLDERS`] entries, [`DEEPEST`] levels down. On a worker.
pub fn walk(root: &Path, dir: &str) -> (Vec<String>, Vec<String>) {
    let (mut folders, mut files) = (Vec::new(), Vec::new());
    if !root.join(dir).is_dir() {
        return (folders, files);
    }
    folders.push(dir.to_owned());
    let mut next = vec![(dir.to_owned(), 0usize)];
    while let Some((at, depth)) = next.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&at)) else { continue };
        for entry in entries.flatten() {
            if folders.len() + files.len() >= MOST_FOLDERS {
                break;
            }
            // A link (to a folder or not) isn't walked into.
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_symlink() || name.starts_with('.') {
                continue;
            }
            let path = format!("{at}/{name}");
            if kind.is_dir() {
                if depth + 1 < DEEPEST {
                    next.push((path.clone(), depth + 1));
                }
                folders.push(path);
            } else if Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| crate::EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
            {
                files.push(path);
            }
        }
    }
    folders.sort();
    files.sort();
    (folders, files)
}

/// What the list shows: the request files read, and the folders known (the files' and the
/// empty collections').
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub files: Vec<RequestFile>,
    pub folders: Vec<String>,
}

/// A folder's parent (`""`: the project's root).
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// A folder's own name.
pub fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The collection a file's folder is in: `http/<name>` for one under [`COLLECTIONS`], else
/// the folder itself.
pub fn collection_of(folder: &str) -> String {
    match folder.strip_prefix(&format!("{COLLECTIONS}/")) {
        Some(rest) => format!("{COLLECTIONS}/{}", rest.split('/').next().unwrap_or(rest)),
        None => folder.to_owned(),
    }
}

/// What a row of the list is: a folder (a collection, a sub-collection) or a request, at its
/// depth in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Folder { path: String, depth: u16, expanded: bool },
    Request { file: usize, request: usize, depth: u16 },
}

impl Row {
    pub fn depth(&self) -> u16 {
        match self {
            Self::Folder { depth, .. } | Self::Request { depth, .. } => *depth,
        }
    }
}

/// The key a folder is collapsed by (in the set [`rows`] takes).
pub fn collapse_key(path: &str) -> String {
    format!("d:{path}")
}

/// The rows shown under folder `within` (`""`: the whole project): each folder, its
/// sub-folders first, then its requests (its files' in their order: the file isn't a level),
/// as expanded; only what matches `filter` (a request's label or method, a folder's name —
/// a match opens what holds it).
pub fn rows(
    listing: &Listing,
    collapsed: &BTreeSet<String>,
    filter: &str,
    within: &str,
) -> Vec<Row> {
    let filter = filter.trim().to_lowercase();
    let inside =
        |path: &str| within.is_empty() || path == within || path.starts_with(&format!("{within}/"));
    // Every folder (with what holds it), under `within`.
    let mut folders: BTreeSet<String> = BTreeSet::new();
    let known =
        listing.folders.iter().map(String::as_str).chain(listing.files.iter().map(|f| f.folder()));
    for folder in known.filter(|f| inside(f)) {
        let mut at = folder;
        while !at.is_empty() && at != within && folders.insert(at.to_owned()) {
            at = parent(at);
        }
    }
    let mut out = Vec::new();
    let found = (!filter.is_empty()).then(|| Found::of(&folders, listing, &filter));
    Tree { folders: &folders, listing, collapsed, found: found.as_ref() }.walk(within, 0, &mut out);
    out
}

/// What a search keeps, found in one pass (not each folder's whole subtree searched again
/// for each row, each name lowercased at each look): each request kept (it matches, or a
/// folder holding it does), and the folders shown (one matches, or holds something kept).
struct Found {
    /// By file, by request.
    kept: Vec<Vec<bool>>,
    shown: std::collections::HashSet<String>,
}

impl Found {
    fn of(folders: &BTreeSet<String>, listing: &Listing, filter: &str) -> Self {
        let matches = |text: &str| text.to_lowercase().contains(filter);
        let named: std::collections::HashSet<&str> =
            folders.iter().map(String::as_str).filter(|f| matches(base_name(f))).collect();
        let mut shown = std::collections::HashSet::new();
        let mut show = |mut at: &str| {
            while !at.is_empty() && shown.insert(at.to_owned()) {
                at = parent(at);
            }
        };
        for folder in &named {
            show(folder);
        }
        let held = |folder: &str| {
            let mut at = folder;
            while !at.is_empty() && folders.contains(at) {
                if named.contains(at) {
                    return true;
                }
                at = parent(at);
            }
            false
        };
        let kept = listing
            .files
            .iter()
            .map(|file| {
                let in_named = held(file.folder());
                let kept: Vec<bool> = file
                    .requests
                    .iter()
                    .map(|r| in_named || matches(&r.label) || matches(&r.method))
                    .collect();
                if kept.iter().any(|k| *k) {
                    show(file.folder());
                }
                kept
            })
            .collect();
        Self { kept, shown }
    }
}

/// A tree being listed: its folders, its requests, what's collapsed, what a search found.
struct Tree<'a> {
    folders: &'a BTreeSet<String>,
    listing: &'a Listing,
    collapsed: &'a BTreeSet<String>,
    /// `None`: nothing searched.
    found: Option<&'a Found>,
}

impl Tree<'_> {
    fn searching(&self) -> bool {
        self.found.is_some()
    }

    /// Request `r` of file `file` kept by the search (all a folder found holds is shown).
    fn keeps(&self, file: usize, r: usize) -> bool {
        self.found.is_none_or(|f| f.kept.get(file).and_then(|k| k.get(r)).is_some_and(|k| *k))
    }

    /// A folder shown: nothing searched, or it (or something in it) matches.
    fn shows(&self, path: &str) -> bool {
        self.found.is_none_or(|f| f.shown.contains(path))
    }

    /// Folder `at`'s rows: its folders (each with what it holds, when open) by name, then
    /// its requests.
    fn walk(&self, at: &str, depth: u16, out: &mut Vec<Row>) {
        for folder in self.folders.iter().filter(|f| parent(f) == at) {
            if !self.shows(folder) {
                continue;
            }
            let expanded = self.searching() || !self.collapsed.contains(&collapse_key(folder));
            out.push(Row::Folder { path: folder.clone(), depth, expanded });
            if expanded {
                self.walk(folder, depth + 1, out);
            }
        }
        for (i, file) in self.listing.files.iter().enumerate().filter(|(_, f)| f.folder() == at) {
            for r in 0..file.requests.len() {
                if self.keeps(i, r) {
                    out.push(Row::Request { file: i, request: r, depth });
                }
            }
        }
    }
}

/// What a name typed in the list is for, as the list places it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameFor<'a> {
    /// A new entry at the list's top.
    Top,
    /// A new entry in folder `path` (just under its row).
    In(&'a str),
    /// Folder `path` renamed (on its row).
    Folder(&'a str),
}

/// A slot of the list: a row, or where a name is typed (at its depth).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Slot {
    Row(usize),
    Naming(u16),
}

/// The list's slots: its rows, the name typed among them where it goes — on the row it
/// renames, else as a new row (at the top, or first in its folder); not found (filtered
/// out): at the top.
pub fn slots(rows: &[Row], naming: Option<NameFor<'_>>) -> Vec<Slot> {
    let mut out: Vec<Slot> = (0..rows.len()).map(Slot::Row).collect();
    let Some(naming) = naming else { return out };
    let folder = |path: &str| {
        rows.iter().position(|r| matches!(r, Row::Folder { path: p, .. } if p == path))
    };
    match naming {
        NameFor::Top => out.insert(0, Slot::Naming(0)),
        NameFor::In(path) => match folder(path) {
            Some(at) => out.insert(at + 1, Slot::Naming(rows[at].depth() + 1)),
            None => out.insert(0, Slot::Naming(0)),
        },
        NameFor::Folder(path) => match folder(path) {
            Some(at) => out[at] = Slot::Naming(rows[at].depth()),
            None => out.insert(0, Slot::Naming(0)),
        },
    }
    out
}

/// A name the user typed for a collection (a folder) or a file, as it goes on disk: trimmed,
/// one part of a path only (no `/`, `\\`, `..`, nor what Windows refuses in a name).
pub fn valid_name(typed: &str) -> Option<String> {
    let name = typed.trim().trim_end_matches('.').trim_end();
    let refused = |c: char| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    };
    (!name.is_empty() && name != "." && name != ".." && !name.chars().any(refused))
        .then(|| name.to_owned())
}

/// A new file's name where `taken` says none is: `stem.http`, else `stem-2.http`…
pub fn free_name(stem: &str, folder: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let join = |name: String| if folder.is_empty() { name } else { format!("{folder}/{name}") };
    let first = join(format!("{stem}.http"));
    if !taken(&first) {
        return first;
    }
    (2..1000).map(|n| join(format!("{stem}-{n}.http"))).find(|name| !taken(name)).unwrap_or(first)
}

#[cfg(test)]
#[path = "../tests/unit/collections.rs"]
mod tests;
