//! Applying a refactoring's plan to the files — open ones in their documents (unsaved),
//! closed ones on disk — and undoing it, every file at once; moving an entry on disk.
//!
//! The files come through the [`ProjectFiles`] port (the app answers with its tabs and its
//! disk), the disk's entries through [`Entries`]. What the app does around them (one batch
//! of changes, the index following, the message) stays the app's.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Plan, TextEdit, apply_edits};

/// The project's files as a refactoring changes them.
pub trait ProjectFiles {
    /// The text of `path`'s open document (`None`: not open).
    fn open_text(&self, path: &Path) -> Option<String>;
    /// Makes `edits` (sorted, already checked) in `path`'s open document, as one command.
    fn edit_open(&mut self, path: &Path, edits: &[TextEdit]);
    /// Replaces the whole text of `path`'s open document, as one command.
    fn replace_open(&mut self, path: &Path, text: String);
    /// A closed file's text.
    fn read(&self, path: &Path) -> Option<String>;
    /// Writes a closed file as a save does (atomically, announced).
    fn write(&mut self, path: &Path, text: &str) -> io::Result<()>;
    /// Moves entry `from` to `to` (not over another one); whatever shows it follows.
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
}

/// The disk's entries, for moving one.
pub trait Entries {
    /// `to` is another entry than `from` (on a disk that ignores case, `User.php` is
    /// `user.php` itself: renaming one to the other only changes case).
    fn is_taken(&self, from: &Path, to: &Path) -> bool;
    /// Makes folder `path`, its parents too.
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    /// Renames `from` to `to`.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

/// Moves entry `from` to `to` on disk: not over another one, its folder made first.
pub fn rename_entry(entries: &dyn Entries, from: &Path, to: &Path) -> io::Result<()> {
    if entries.is_taken(from, to) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a file with this name already exists",
        ));
    }
    if let Some(parent) = to.parent() {
        entries.create_dir(parent)?;
    }
    entries.rename(from, to)
}

/// What undoes a refactoring: the changed files' text before it (and a fingerprint of the
/// text it left: a file changed since isn't overwritten), and the renames.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Undo {
    files: Vec<(PathBuf, String, u64)>,
    renames: Vec<(PathBuf, PathBuf)>,
}

impl Undo {
    /// The changed files (under their names before the renames).
    pub fn files(&self) -> Vec<PathBuf> {
        self.files.iter().map(|(p, ..)| p.clone()).collect()
    }

    /// The entries renamed (`from` → `to`), in order.
    pub fn renames(&self) -> &[(PathBuf, PathBuf)] {
        &self.renames
    }
}

/// Something that couldn't be done to one file (the rest went on): logged by the app.
#[derive(Debug)]
pub enum Problem {
    Write(PathBuf, io::Error),
    Rename(PathBuf, io::Error),
    RenameBack(PathBuf, io::Error),
    Restore(PathBuf, io::Error),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(path, e) => write!(f, "can't write {}: {e}", path.display()),
            Self::Rename(path, e) => write!(f, "can't rename {}: {e}", path.display()),
            Self::RenameBack(path, e) => write!(f, "can't rename {} back: {e}", path.display()),
            Self::Restore(path, e) => write!(f, "can't restore {}: {e}", path.display()),
        }
    }
}

/// A plan applied.
#[derive(Debug)]
pub struct Applied {
    /// How many places changed, in how many files.
    pub places: usize,
    pub files: usize,
    /// The files (and renames) left out: changed since the plan, unreadable, unwritable.
    pub skipped: usize,
    pub undo: Undo,
    pub problems: Vec<Problem>,
}

impl Applied {
    /// Nothing changed (the plan is stale): nothing to undo either.
    pub fn changed_nothing(&self) -> bool {
        self.places == 0 && self.undo.renames.is_empty()
    }
}

/// A refactoring undone.
#[derive(Debug)]
pub struct Undone {
    /// The files changed since the refactoring (their work is kept, not undone).
    pub skipped: usize,
    /// What changed back: the files (at their names again) and the renames back.
    pub files: Vec<PathBuf>,
    pub renames: Vec<(PathBuf, PathBuf)>,
    pub problems: Vec<Problem>,
}

/// Applies `plan`'s chosen changes: each file's edits (an open file in its document, a
/// closed one on disk; a file whose text changed since the plan is skipped), then the
/// chosen renames.
pub fn apply_plan(plan: &Plan, files: &mut dyn ProjectFiles) -> Applied {
    let mut undo = Undo::default();
    let mut problems = Vec::new();
    let (mut places, mut changed, mut skipped) = (0, 0, 0);
    for file in plan.edits() {
        let (before, after) = if let Some(before) = files.open_text(&file.path) {
            let Ok(after) = apply_edits(&before, &file.edits) else {
                skipped += 1;
                continue;
            };
            files.edit_open(&file.path, &file.edits);
            (before, after)
        } else {
            let Some(before) = files.read(&file.path) else {
                skipped += 1;
                continue;
            };
            let Ok(after) = apply_edits(&before, &file.edits) else {
                skipped += 1;
                continue;
            };
            if let Err(e) = files.write(&file.path, &after) {
                problems.push(Problem::Write(file.path.clone(), e));
                skipped += 1;
                continue;
            }
            (before, after)
        };
        undo.files.push((file.path.clone(), before, fingerprint(&after)));
        places += file.edits.len();
        changed += 1;
    }
    for rename in plan.renames.iter().filter(|r| r.chosen && r.from != r.to) {
        match files.rename(&rename.from, &rename.to) {
            Ok(()) => undo.renames.push((rename.from.clone(), rename.to.clone())),
            Err(e) => {
                problems.push(Problem::Rename(rename.from.clone(), e));
                skipped += 1;
            }
        }
    }
    Applied { places, files: changed, skipped, undo, problems }
}

/// Undoes a refactoring: the renames back, every file's text as it was — except the files
/// changed since (their work is kept).
pub fn undo_plan(undo: Undo, files: &mut dyn ProjectFiles) -> Undone {
    let mut problems = Vec::new();
    for (from, to) in undo.renames.iter().rev() {
        if let Err(e) = files.rename(to, from) {
            problems.push(Problem::RenameBack(to.clone(), e));
        }
    }
    let changed = undo.files();
    let renames = undo.renames.iter().map(|(from, to)| (to.clone(), from.clone())).collect();
    let mut skipped = 0;
    for (path, text, left) in undo.files {
        if let Some(now) = files.open_text(&path) {
            if fingerprint(&now) != left {
                skipped += 1;
                continue;
            }
            files.replace_open(&path, text);
        } else {
            let now = files.read(&path);
            if now.as_deref().map(fingerprint) != Some(left) {
                skipped += 1;
                continue;
            }
            if let Err(e) = files.write(&path, &text) {
                problems.push(Problem::Restore(path, e));
            }
        }
    }
    Undone { skipped, files: changed, renames, problems }
}

/// A text's fingerprint (is it still what a refactoring left?).
fn fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

#[cfg(test)]
#[path = "../tests/unit/applying.rs"]
mod tests;
