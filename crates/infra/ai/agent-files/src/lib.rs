//! What every agent's module reads on disk alike (always on a worker): its program, found
//! where installers put it though the app wasn't started from a shell ([`find`],
//! [`path_beside`]); its sessions' files and folders listed with limits — entries
//! themselves, never a link followed, a bounded count ([`files`], [`folders`]) — read
//! bounded ([`read_bounded`]) and kept by their stamps, so a list asked again reads only
//! what changed ([`Kept`]).
//!
//! An agent's own formats stay in its module: here only the ways of reading them.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use kernel::GenerationalCache;

/// A program's file names on this system (`claude` → `claude.exe`, `claude.cmd`).
fn file_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd")]
    } else {
        vec![name.to_owned()]
    }
}

/// Program `name`: on `PATH`, else in `extra` (an agent's own folder), else where installers
/// put it (an app started from the Dock doesn't get the shell's `PATH`): `~/.local/bin`,
/// npm's global folders, Homebrew's.
pub fn find(name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    let home = dirs::home_dir();
    let mut places: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    places.extend(extra.iter().cloned());
    if let Some(home) = &home {
        places.push(home.join(".local").join("bin"));
        places.push(home.join(".npm-global").join("bin"));
        if cfg!(windows) {
            places.push(home.join("AppData").join("Roaming").join("npm"));
        }
    }
    if !cfg!(windows) {
        places.push(PathBuf::from("/opt/homebrew/bin"));
        places.push(PathBuf::from("/usr/local/bin"));
    }
    let names = file_names(name);
    places.iter().flat_map(|dir| names.iter().map(move |n| dir.join(n))).find(|p| p.is_file())
}

/// `PATH` with `program`'s folder first: a Node script (`#!/usr/bin/env node`) finds the
/// `node` installed beside it, though the app wasn't started from a shell.
pub fn path_beside(program: &Path) -> (String, String) {
    let mut dirs: Vec<PathBuf> = program.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let joined = std::env::join_paths(dirs).map(|p| p.to_string_lossy().into_owned());
    ("PATH".to_owned(), joined.unwrap_or_default())
}

/// Whether `a` and `b` name the same folder as written (on Windows, in any case: `c:` and
/// `C:`; a trailing separator either way).
pub fn same_folder(a: &Path, b: &Path) -> bool {
    let tidy = |p: &Path| {
        let text = p.to_string_lossy();
        let text = text.trim_end_matches(['/', '\\']);
        if cfg!(windows) { text.to_lowercase() } else { text.to_owned() }
    };
    tidy(a) == tidy(b)
}

/// A file's stamp: its modification time (seconds since the epoch) and its size.
pub type Stamp = (i64, u64);

/// `path`'s stamp, if it's a file itself (a link isn't followed).
pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some((i64::try_from(modified).ok()?, meta.len()))
}

/// The entries of `dir` that are `want` themselves (a link isn't followed), at most `most`
/// looked at.
fn entries(dir: &Path, most: usize, want: fn(&std::fs::FileType) -> bool) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .take(most)
        .filter(|e| e.file_type().is_ok_and(|t| want(&t)))
        .map(|e| e.path())
        .collect()
}

/// The files in `dir` ending in `.{extension}`, with their stamps, newest first; at most
/// `most` entries looked at (sorted after: the newest are never the ones cut).
pub fn files(dir: &Path, extension: &str, most: usize) -> Vec<(PathBuf, Stamp)> {
    let mut found: Vec<(PathBuf, Stamp)> = entries(dir, most, std::fs::FileType::is_file)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == extension))
        .filter_map(|p| Some((stamp(&p)?, p)).map(|(s, p)| (p, s)))
        .collect();
    found.sort_by_key(|(_, (modified, _))| std::cmp::Reverse(*modified));
    found
}

/// The folders in `dir` (themselves, not links), at most `most` looked at.
pub fn folders(dir: &Path, most: usize) -> Vec<PathBuf> {
    entries(dir, most, std::fs::FileType::is_dir)
}

/// `path`'s bytes if it's a file itself no longer than `most` bytes.
pub fn read_bounded(path: &Path, most: u64) -> Option<Vec<u8>> {
    let (_, len) = stamp(path)?;
    if len > most {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).ok()?);
    std::fs::File::open(path).ok()?.take(most).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// What was read of files, kept by each file's stamp (a bounded count, the oldest let go):
/// a file read again only once it changed.
pub struct Kept<T> {
    kept: Mutex<GenerationalCache<PathBuf, (Stamp, T)>>,
}

impl<T: Clone> Kept<T> {
    /// At most `most` files' worth kept.
    pub fn new(most: usize) -> Self {
        Self { kept: Mutex::new(GenerationalCache::new(most)) }
    }

    /// `file`'s value at `stamp`: kept if its stamp didn't change, else `read` (outside the
    /// lock: it reads the disk) and kept.
    pub fn get(&self, file: &Path, stamp: Stamp, read: impl FnOnce() -> Option<T>) -> Option<T> {
        {
            let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((at, value)) = kept.get(file)
                && *at == stamp
            {
                return Some(value.clone());
            }
        }
        let value = read()?;
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.insert(file.to_path_buf(), (stamp, value.clone()));
        Some(value)
    }
}

impl<T> std::fmt::Debug for Kept<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kept").finish_non_exhaustive()
    }
}

/// An id an agent's session can be named by on a command line and as a file's name: ASCII
/// letters, digits, `-` and `_`, 1 to 128 of them.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A title read from an agent's files, as a list shows it: its markup's tags taken out
/// (`<user_input mode="act">…`), one line, no control characters, at most `most`
/// characters (an ellipsis when cut).
pub fn title_of(text: &str, most: usize) -> String {
    let mut plain = String::with_capacity(text.len().min(most * 4));
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' if !in_tag => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            c if c.is_whitespace() => plain.push(' '),
            c if c.is_control() => {}
            c => plain.push(c),
        }
        if plain.len() > most * 4 {
            break;
        }
    }
    let words: Vec<&str> = plain.split_whitespace().collect();
    let line = words.join(" ");
    if line.chars().count() <= most {
        return line;
    }
    let cut: String = line.chars().take(most.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
