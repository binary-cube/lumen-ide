//! Use cases on the project's entries (the tree's files and folders), through one port to
//! the disk ([`EntryStore`]): a new entry's name checked and the entry made; what a paste
//! takes (the system's file manager, what was copied here, paths as text); a paste done —
//! copies or moves, with its progress and a stop; entries removed to the trash. They run
//! where they're called (a background thread for the long ones); whoever shows the tree
//! (the app today, its module tomorrow) asks them the same way.

use std::io;
use std::path::{Path, PathBuf};

use crate::clipboard::{paste_forbidden, paste_target};

/// The disk, as these use cases need it (implemented in infrastructure: `fs-local`).
pub trait EntryStore: Send + Sync {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// How many files `path` holds (1 for a file).
    fn count_files(&self, path: &Path) -> u64;
    /// Copies `from` (a file or a folder, recursively) to `to`, telling `copied` after each
    /// file; stops as soon as `stop` says so.
    fn copy(
        &self,
        from: &Path,
        to: &Path,
        copied: &mut dyn FnMut(u64),
        stop: &dyn Fn() -> bool,
    ) -> io::Result<()>;
    /// Moves `from` to `to` (which must not exist).
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Moves `paths` to the system's trash (restorable).
    fn trash(&self, paths: &[PathBuf]) -> io::Result<()>;
    /// Makes an empty file (its folders too); fails if it exists.
    fn create_file(&self, path: &Path) -> io::Result<()>;
    /// Makes a folder (its parents too).
    fn create_dir(&self, path: &Path) -> io::Result<()>;
}

/// A new entry's name is unusable: empty, absolute, climbing out (`..`) or with characters
/// a file system refuses.
pub fn name_problem(name: &str) -> bool {
    let name = name.trim();
    name.is_empty()
        || name.starts_with('/')
        || name.split(['/', '\\']).any(|part| part == "..")
        || name.contains(['<', '>', ':', '"', '|', '?', '*'])
}

/// An entry's new name is unusable: a new entry's problems, or a path (a rename stays in its
/// folder), `.` / `..`, control characters; on Windows also a trailing dot or space and the
/// devices' names (`CON`, `NUL`, `COM1`…), which it refuses or strips.
pub fn rename_problem(name: &str) -> bool {
    let name = name.trim();
    name_problem(name)
        || name.contains(['/', '\\'])
        || name == "."
        || name.chars().any(char::is_control)
        || (cfg!(windows) && windows_problem(name))
}

/// What Windows refuses in a name (or changes: a trailing dot or space is dropped).
fn windows_problem(name: &str) -> bool {
    const DEVICES: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    let stem = name.split('.').next().unwrap_or(name).trim_end().to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix)
            .is_some_and(|n| n.len() == 1 && n.chars().all(|c| c.is_ascii_digit() && c != '0'))
    };
    name.ends_with(['.', ' '])
        || DEVICES.contains(&stem.as_str())
        || numbered("COM")
        || numbered("LPT")
}

/// Makes entry `name` (a folder or an empty file) in folder `parent`. `Err`: the name is
/// unusable, it exists, or the disk refused.
pub fn create(
    store: &dyn EntryStore,
    parent: &Path,
    name: &str,
    is_dir: bool,
) -> io::Result<PathBuf> {
    if name_problem(name) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unusable name"));
    }
    let path = parent.join(name.trim());
    if store.exists(&path) {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    if is_dir { store.create_dir(&path) } else { store.create_file(&path) }?;
    Ok(path)
}

/// What a paste takes, and whether it moves (a cut) or copies: files copied in the system's
/// file manager (`os_files`) when they're not what was copied here (`ours`), else what was
/// copied / cut here, else absolute paths in the clipboard's text (copied). Only entries
/// that exist and aren't a folder pasted into itself (`into`).
pub fn paste_sources(
    store: &dyn EntryStore,
    ours: Option<(Vec<PathBuf>, bool)>,
    os_files: &[PathBuf],
    text: Option<&str>,
    into: &Path,
) -> (Vec<PathBuf>, bool) {
    // Copied in the file manager after (or instead of) here: those win, as copies.
    let ours = match ours {
        Some((paths, _)) if !os_files.is_empty() && os_files != paths.as_slice() => {
            Some((os_files.to_vec(), false))
        }
        None if !os_files.is_empty() => Some((os_files.to_vec(), false)),
        other => other,
    };
    // Paths copied from another app as text: copied, not moved.
    let (sources, cut) = ours
        .or_else(|| {
            let paths: Vec<PathBuf> = text?
                .lines()
                .map(|l| PathBuf::from(l.trim()))
                .filter(|p| p.is_absolute() && store.exists(p))
                .collect();
            (!paths.is_empty()).then_some((paths, false))
        })
        .unwrap_or_default();
    let sources = sources
        .into_iter()
        .filter(|s| store.exists(s) && !(store.is_dir(s) && paste_forbidden(s, into)))
        .collect();
    (sources, cut)
}

/// What a paste did: what was placed where (`from` → `to`), what failed (the reason).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PasteOutcome {
    pub placed: Vec<(PathBuf, PathBuf)>,
    pub failed: Vec<(PathBuf, String)>,
}

/// Pastes `sources` into folder `into` — moved (`cut`) or copied, each under a free name
/// (`name copy`…); a cut already there is left alone. `progress` hears the files done and
/// how many there are, `copied` each copied file's bytes (read and written); `stop` ends it
/// after the entry going on (what's done stays).
pub fn paste(
    store: &dyn EntryStore,
    sources: Vec<PathBuf>,
    cut: bool,
    into: &Path,
    progress: &mut dyn FnMut(u64, u64),
    copied: &mut dyn FnMut(u64),
    stop: &dyn Fn() -> bool,
) -> PasteOutcome {
    let total: u64 = sources.iter().map(|s| store.count_files(s)).sum();
    let mut done = 0u64;
    let mut outcome = PasteOutcome::default();
    for source in sources {
        if cut && source.parent() == Some(into) {
            continue;
        }
        let to = paste_target(&source, store.is_dir(&source), into, &|p| store.exists(p));
        let result = if cut {
            let files = store.count_files(&source);
            store.rename(&source, &to).map(|()| {
                done += files;
                progress(done, total);
            })
        } else {
            let mut one = |bytes: u64| {
                done += 1;
                copied(bytes);
                progress(done, total);
            };
            store.copy(&source, &to, &mut one, stop)
        };
        match result {
            Ok(()) => outcome.placed.push((source, to)),
            Err(e) => outcome.failed.push((source, e.to_string())),
        }
        if stop() {
            break;
        }
    }
    outcome
}

/// Removes `paths` (already confirmed) to the trash. `Err`: why it failed.
pub fn trash(store: &dyn EntryStore, paths: &[PathBuf]) -> Result<(), String> {
    store.trash(paths).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/entries.rs"]
mod tests;
