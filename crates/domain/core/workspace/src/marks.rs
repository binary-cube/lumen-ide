//! What the user decided about the project's folders and files: left out of the index, a
//! library, generated, or normal whatever the rules say. A path takes the mark of its nearest
//! marked ancestor; a mark goes before the modules' rules ([`kernel::ProjectDirs`]). Kept in
//! the project's own folder, as text, one mark per line: it's meant for version control.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock};

use crate::application::FileSystem;

/// The marks' file, in the project's own folder.
pub const FILE_NAME: &str = "path-marks";

/// What a path was marked as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathMark {
    /// Out of the index, the text search and the search by name.
    Excluded,
    /// Indexed as a dependency.
    Library,
    /// Indexed, not searched as text.
    Generated,
    /// As the project's own code, whatever the rules or a parent say.
    Normal,
}

impl PathMark {
    pub const ALL: [Self; 4] = [Self::Excluded, Self::Library, Self::Generated, Self::Normal];

    /// As written in the file.
    pub fn id(self) -> &'static str {
        match self {
            Self::Excluded => "excluded",
            Self::Library => "library",
            Self::Generated => "generated",
            Self::Normal => "normal",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }
}

/// The project's marks, by path (relative to the root, with `/`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathMarks {
    marks: BTreeMap<String, PathMark>,
}

impl PathMarks {
    /// The marks written in `text` (a line it can't read is left out).
    pub fn parse(text: &str) -> Self {
        let marks = text
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let (mark, path) = line.split_once(' ')?;
                let path = path.trim().trim_matches('/');
                (!path.is_empty()).then(|| Some((path.to_owned(), PathMark::from_id(mark)?)))?
            })
            .collect();
        Self { marks }
    }

    /// As the file holds them.
    pub fn to_text(&self) -> String {
        self.marks.iter().map(|(path, mark)| format!("{} {path}\n", mark.id())).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, PathMark)> {
        self.marks.iter().map(|(path, mark)| (path.as_str(), *mark))
    }

    /// `path` marked (`None`: not any more).
    pub fn set(&mut self, path: &str, mark: Option<PathMark>) {
        let path = path.trim_matches('/');
        match mark {
            Some(mark) => self.marks.insert(path.to_owned(), mark),
            None => self.marks.remove(path),
        };
    }

    /// What `path` was marked as itself.
    pub fn own(&self, path: &str) -> Option<PathMark> {
        self.marks.get(path.trim_matches('/')).copied()
    }

    /// `path`'s mark: its own, or its nearest marked ancestor's.
    pub fn of(&self, path: &str) -> Option<PathMark> {
        if self.marks.is_empty() {
            return None;
        }
        let mut at = path.trim_matches('/');
        loop {
            if let Some(mark) = self.marks.get(at) {
                return Some(*mark);
            }
            at = &at[..at.rfind('/')?];
        }
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        self.of(path) == Some(PathMark::Excluded)
    }

    /// A library folder (`vendor`) is excluded: what the code uses from it can't be known.
    pub fn excludes_dependencies(&self) -> bool {
        self.iter().any(|(path, mark)| {
            mark == PathMark::Excluded && path.split('/').any(kernel::paths::is_dependency_dir)
        })
    }

    /// `path` was marked normal: the rules leaving it out don't count.
    pub fn is_normal(&self, path: &str) -> bool {
        self.of(path) == Some(PathMark::Normal)
    }
}

/// A project's marks as they are now, shared with whoever reads them (the index's thread, the
/// tree): a look is a reference count.
#[derive(Debug, Clone, Default)]
pub struct ProjectMarks(Arc<RwLock<Arc<PathMarks>>>);

impl ProjectMarks {
    pub fn new(marks: PathMarks) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(marks))))
    }

    /// The marks in the project at `root` (none, if there's no file).
    pub fn read(root: &Path, fs: &dyn FileSystem) -> Self {
        let file = root.join(kernel::brand::PROJECT_DIR).join(FILE_NAME);
        let marks = fs.read_file(&file).ok().map_or_else(PathMarks::default, |bytes| {
            PathMarks::parse(&String::from_utf8_lossy(&bytes))
        });
        Self::new(marks)
    }

    pub fn get(&self) -> Arc<PathMarks> {
        Arc::clone(&self.0.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn replace(&self, marks: PathMarks) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(marks);
    }
}

#[cfg(test)]
#[path = "../tests/unit/marks.rs"]
mod tests;
