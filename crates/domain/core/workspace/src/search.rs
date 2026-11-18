//! File search in the project: the path index and approximate (fuzzy) matching,
//! as "Go to File" uses: `uctrl` finds `UserController.php`.

use std::io;
use std::path::Path;

use crate::application::FileSystem;
use crate::domain::NodeKind;
use crate::ignore::IgnoreRules;
use kernel::paths::{METADATA_DIRS, is_dependency_dir, is_unlisted_dir};

/// A directory that isn't indexed: metadata (and ours), and what modules say isn't listed
/// (npm's packages — very many files nothing resolves into; Composer's `vendor` is indexed:
/// its classes are used).
pub fn skipped_dir(name: &str) -> bool {
    METADATA_DIRS.contains(&name) || is_unlisted_dir(name) || name == kernel::brand::PROJECT_DIR
}

/// The most files a project's path index holds (past it, the rest isn't listed).
pub const FILE_LIMIT: usize = 200_000;

/// All files under `root`, as relative paths with `/`, up to `limit` files.
pub fn index_files(root: &Path, fs: &dyn FileSystem, limit: usize) -> io::Result<Vec<String>> {
    index_project(root, fs, limit).map(|index| index.files)
}

/// Which of the project's files a search / listing looks at: by extension, whether
/// dependencies (`vendor/`, `node_modules/`) count, and under which folder. One filter for
/// every consumer (path search, usages, refactorings, the canvas).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileFilter {
    /// The extensions (lowercase, without the dot); empty: every file.
    pub extensions: &'static [&'static str],
    /// Files in dependency directories count too.
    pub dependencies: bool,
    /// Only under this folder (relative, with `/`); `None`: the whole project.
    pub under: Option<String>,
}

impl FileFilter {
    /// The project's own files with these extensions (no dependencies).
    pub fn code(extensions: &'static [&'static str]) -> Self {
        Self { extensions, dependencies: false, under: None }
    }

    /// `relative` (a project path, with `/`) passes.
    pub fn accepts(&self, relative: &str) -> bool {
        let path = Path::new(relative);
        if !self.extensions.is_empty() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                return false;
            }
        }
        if !self.dependencies && kernel::in_dependencies_or_git(path) {
            return false;
        }
        self.under
            .as_deref()
            .is_none_or(|dir| relative.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/')))
    }
}

/// The project's paths: its files and its directories (relative, with `/`, sorted).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathIndex {
    pub files: Vec<String>,
    pub folders: Vec<String>,
    /// The files `.gitignore` leaves out, in the folders walked anyway (`.env.local`):
    /// found by their name, never searched in (their content is often generated).
    pub ignored: Vec<String>,
}

/// Files and directories under `root`, up to `limit` files. Skipped: [`skipped_dir`] and
/// what `.gitignore` files exclude (build output…), except dependency directories
/// (`vendor/` stays searchable even when ignored).
pub fn index_project(root: &Path, fs: &dyn FileSystem, limit: usize) -> io::Result<PathIndex> {
    index_marked(root, fs, limit, &crate::marks::PathMarks::default())
}

/// [`index_project`], by the user's `marks`: what's excluded isn't listed, what's marked
/// normal is, whatever the rules say.
pub fn index_marked(
    root: &Path,
    fs: &dyn FileSystem,
    limit: usize,
    marks: &crate::marks::PathMarks,
) -> io::Result<PathIndex> {
    let mut index = PathIndex::default();
    let mut ignore = IgnoreRules::default();
    let mut stack = vec![String::new()];
    'walk: while let Some(dir) = stack.pop() {
        let entries = match fs.read_dir(&root.join(&dir)) {
            Ok(entries) => entries,
            // An unreadable directory (permissions) doesn't stop indexing.
            Err(_) if !dir.is_empty() => continue,
            Err(e) => return Err(e),
        };
        if entries.iter().any(|e| e.name == ".gitignore" && e.kind == NodeKind::File)
            && let Ok(content) = fs.read_file(&root.join(&dir).join(".gitignore"))
        {
            ignore.add(&dir, &String::from_utf8_lossy(&content));
        }
        for entry in entries {
            let path =
                if dir.is_empty() { entry.name.clone() } else { format!("{dir}/{}", entry.name) };
            let is_dir = entry.kind == NodeKind::Directory;
            if marks.is_excluded(&path) {
                continue;
            }
            let wanted = is_dependency_dir(&entry.name) || marks.is_normal(&path);
            if !wanted && !ignore.is_empty() && ignore.is_ignored(&path, is_dir) {
                // An ignored file of a folder walked anyway: by its name only (an ignored
                // folder isn't walked: build output, caches).
                if !is_dir && index.ignored.len() < limit {
                    index.ignored.push(path);
                }
                continue;
            }
            match entry.kind {
                NodeKind::Directory if !skipped_dir(&entry.name) || marks.is_normal(&path) => {
                    index.folders.push(path.clone());
                    stack.push(path);
                }
                NodeKind::Directory => {}
                NodeKind::File => {
                    index.files.push(path);
                    if index.files.len() >= limit {
                        break 'walk;
                    }
                }
            }
        }
    }
    index.files.sort_unstable();
    index.folders.sort_unstable();
    index.ignored.sort_unstable();
    Ok(index)
}

/// A result: the relative path, the score and the matched positions (bytes) in the file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatch {
    pub path: String,
    pub score: i32,
    /// Offsets in the file name (the last path component), for highlighting.
    pub name_positions: Vec<usize>,
}

impl FileMatch {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The directory (without the file name); empty for files in the root.
    pub fn dir(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(dir, _)| dir)
    }
}

/// What a path search looks for: the text, and at most how many folders / files (0: none).
/// Each place that searches (the project panel, the home screen, "Search in Project")
/// asks for what it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathQuery<'a> {
    pub text: &'a str,
    pub folders: usize,
    pub files: usize,
}

/// What a path search found: folders (by their own name) and files, each best first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PathResults {
    pub folders: Vec<FileMatch>,
    pub files: Vec<FileMatch>,
}

impl PathResults {
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.files.is_empty()
    }
}

/// A search text ending in a place, `User.php:12` or `User.php:12:5` (1-based, as
/// tools print them): the text to search and the 0-based line and column.
pub fn split_location(text: &str) -> (&str, Option<(u32, u32)>) {
    let number = |part: &str| part.parse::<u32>().ok().filter(|_| !part.is_empty());
    let mut parts = text.rsplitn(3, ':');
    let last = parts.next().unwrap_or("");
    let middle = parts.next();
    let rest = parts.next();
    match (rest, middle.and_then(number), number(last)) {
        // `name:line:column`
        (Some(name), Some(line), Some(column)) if !name.is_empty() => {
            (name, Some((line.saturating_sub(1), column.saturating_sub(1))))
        }
        // `name:line`
        (_, _, Some(line)) => {
            let name = &text[..text.len() - last.len() - 1];
            if name.is_empty() { (text, None) } else { (name, Some((line.saturating_sub(1), 0))) }
        }
        _ => (text, None),
    }
}

/// The one path search of the project (folders and files), used everywhere.
pub fn search_paths(files: &[String], folders: &[String], query: &PathQuery<'_>) -> PathResults {
    search_paths_with(files, &[], folders, query)
}

/// [`search_paths`], the files `.gitignore` leaves out (`ignored`) found by name too.
pub fn search_paths_with(
    files: &[String],
    ignored: &[String],
    folders: &[String],
    query: &PathQuery<'_>,
) -> PathResults {
    PathResults {
        folders: if query.folders > 0 {
            search_folders(folders, query.text, query.folders)
        } else {
            Vec::new()
        },
        files: if query.files > 0 {
            search_files_in(files.iter().chain(ignored), query.text, query.files)
        } else {
            Vec::new()
        },
    }
}

/// The best `limit` matches for `query`, in score order.
pub fn search_files(files: &[String], query: &str, limit: usize) -> Vec<FileMatch> {
    search_files_in(files, query, limit)
}

fn search_files_in<'a>(
    files: impl IntoIterator<Item = &'a String>,
    query: &str,
    limit: usize,
) -> Vec<FileMatch> {
    let query: Vec<char> =
        query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<FileMatch> =
        files.into_iter().filter_map(|path| score_path(path, &query)).collect();
    matches.sort_by(|a, b| {
        b.score.cmp(&a.score).then(a.path.len().cmp(&b.path.len())).then(a.path.cmp(&b.path))
    });
    matches.truncate(limit);
    matches
}

/// The best `limit` folders whose own name matches `query` (a match spread over the
/// path alone would list every subfolder of a matching one).
pub fn search_folders(folders: &[String], query: &str, limit: usize) -> Vec<FileMatch> {
    let query: Vec<char> =
        query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<FileMatch> = folders
        .iter()
        .filter_map(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            let (score, positions) = fuzzy(name, &query)?;
            let prefix = if positions.first() == Some(&0) { 30 } else { 0 };
            let depth = path.matches('/').count() as i32;
            Some(FileMatch {
                path: path.clone(),
                score: score + prefix - name.len() as i32 - depth * 2,
                name_positions: positions,
            })
        })
        .collect();
    matches.sort_by(|a, b| b.score.cmp(&a.score).then(a.path.cmp(&b.path)));
    matches.truncate(limit);
    matches
}

/// The folders a path typed so far goes on to, as a shell completes one: in the folder
/// before its last `/`, those whose name starts with what follows it (any case), by name;
/// then, while there's room, those under that folder whose own name matches it loosely
/// (`src/ctl` → `src/Controller`; from two letters on). Each once, at most `limit`.
pub fn complete_folders(folders: &[String], typed: &str, limit: usize) -> Vec<String> {
    let typed = typed.trim().trim_start_matches('/');
    let (parent, partial) = typed.rsplit_once('/').unwrap_or(("", typed));
    let partial = partial.to_lowercase();
    let in_parent = |path: &str| match path.rsplit_once('/') {
        Some((dir, _)) => dir.eq_ignore_ascii_case(parent),
        None => parent.is_empty(),
    };
    let name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let mut found: Vec<&String> =
        folders.iter().filter(|f| in_parent(f) && name(f).starts_with(&partial)).collect();
    found.sort_by_key(|f| name(f));
    let mut out: Vec<String> = found.into_iter().take(limit).cloned().collect();
    // A single letter would match loosely almost anything.
    if out.len() < limit && partial.chars().count() >= 2 {
        let under = |path: &str| {
            parent.is_empty()
                || path.len() > parent.len()
                    && path[..parent.len()].eq_ignore_ascii_case(parent)
                    && path.as_bytes()[parent.len()] == b'/'
        };
        let loose: Vec<String> = folders.iter().filter(|f| under(f)).cloned().collect();
        for m in search_folders(&loose, &partial, limit) {
            if out.len() == limit {
                break;
            }
            if !out.contains(&m.path) {
                out.push(m.path);
            }
        }
    }
    out
}

/// Matching first in the file name (counts much more), then in the whole path.
fn score_path(path: &str, query: &[char]) -> Option<FileMatch> {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    let name = &path[name_start..];
    if let Some((score, positions)) = fuzzy(name, query) {
        // Bonus for a match at the start of the name and for short names.
        let prefix = if positions.first() == Some(&0) { 30 } else { 0 };
        let score = 1000 + score + prefix - name.len() as i32;
        return Some(FileMatch { path: path.to_owned(), score, name_positions: positions });
    }
    let (score, positions) = fuzzy(path, query)?;
    let name_positions =
        positions.iter().filter(|&&p| p >= name_start).map(|p| p - name_start).collect();
    Some(FileMatch { path: path.to_owned(), score: score - path.len() as i32 / 4, name_positions })
}

/// Paths: words start after `/ . _ -` and spaces; word starts are preferred when they
/// follow shortly; gaps cost.
const PATHS: kernel::FuzzyStyle = kernel::FuzzyStyle {
    separators: &['/', '.', '_', '-', ' '],
    consecutive_bonus: 15,
    boundary_bonus: 10,
    gap_penalty: true,
    prefer_boundaries: true,
};

fn fuzzy(text: &str, query: &[char]) -> Option<(i32, Vec<usize>)> {
    kernel::fuzzy_match(text, query, &PATHS)
}

#[cfg(test)]
#[path = "../tests/unit/search.rs"]
mod tests;
