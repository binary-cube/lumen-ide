//! Where the lines go: a file a day in the log folder (`app-2026-11-20.jsonl`), continued
//! in `app-2026-11-20.1.jsonl`, `.2`… past [`Limits::file`]. The instance that opens a
//! day's file first holds it locked; another one running meanwhile writes its own
//! (`app-2026-11-20-<run>.jsonl`), so no line is mixed with another's and no turnover
//! steps on another's. What's kept: at most [`Limits::days`] days and [`Limits::total`]
//! bytes, the oldest let go first (never a file in use).

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// How much is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// One file at most (bytes): past it, the next one.
    pub file: u64,
    /// The days kept.
    pub days: u32,
    /// All the files at most (bytes).
    pub total: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { file: 10 * 1024 * 1024, days: 14, total: 100 * 1024 * 1024 }
    }
}

/// The files' names start with this.
pub const PREFIX: &str = "app-";
/// And end with this.
pub const EXTENSION: &str = "jsonl";

/// The single file of before, moved into the folder once (let go after the days kept).
pub const LEGACY: &str = "legacy.log";

/// The file being written: its path, its day, its number in the day, its size so far.
#[derive(Debug)]
pub struct Current {
    pub file: File,
    pub path: PathBuf,
    pub day: String,
    pub index: u32,
    pub size: u64,
}

/// File `index` of `day` (0: the day's first), `own` when another instance holds the
/// day's (this run's own name).
pub fn name(day: &str, index: u32, own: Option<&str>) -> String {
    let own = own.map(|run| format!("-{run}")).unwrap_or_default();
    match index {
        0 => format!("{PREFIX}{day}{own}.{EXTENSION}"),
        i => format!("{PREFIX}{day}{own}.{i}.{EXTENSION}"),
    }
}

/// `path` opened to add to (made if it isn't there), readable too: a file opened only to
/// add to can't be locked on Windows (its lock needs read or write access), so another
/// instance's would never be told from this one's.
fn append_to(path: &Path) -> io::Result<std::fs::File> {
    OpenOptions::new().create(true).read(true).append(true).open(path)
}

/// Opens where `day`'s lines go in `dir`: the day's last file while it has room (`limits`),
/// else the next; the day's own when another instance holds it (`run`'s).
pub fn open(dir: &Path, day: &str, run: &str, limits: Limits) -> io::Result<Current> {
    std::fs::create_dir_all(dir)?;
    for own in [None, Some(run)] {
        let mut index = 0;
        loop {
            let path = dir.join(name(day, index, own));
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            if size >= limits.file {
                index += 1;
                continue;
            }
            let file = append_to(&path)?;
            // Held by another instance: this one writes its own.
            if own.is_none() && file.try_lock().is_err() {
                break;
            }
            if own.is_some() {
                let _ = file.try_lock();
            }
            return Ok(Current { file, path, day: day.to_owned(), index, size });
        }
    }
    Err(io::Error::other("no log file could be opened"))
}

/// The next file of the same day (`current` full): `.1`, `.2`…, this run's own name kept
/// when it had one.
pub fn next(dir: &Path, current: &Current, run: &str, limits: Limits) -> io::Result<Current> {
    let own = current
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains(&format!("-{run}")))
        .then_some(run);
    let mut index = current.index + 1;
    loop {
        let path = dir.join(name(&current.day, index, own));
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if size < limits.file {
            let file = append_to(&path)?;
            let _ = file.try_lock();
            return Ok(Current { file, path, day: current.day.clone(), index, size });
        }
        index += 1;
    }
}

/// A log file in the folder: its path, its day, its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub path: PathBuf,
    pub day: String,
    pub size: u64,
}

/// The log files in `dir` (others left alone: only `app-<day>….jsonl` are ours), the oldest
/// first (by day, then by name).
pub fn listed(dir: &Path) -> Vec<Kept> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut kept: Vec<Kept> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let rest = name.strip_prefix(PREFIX)?;
            if !name.ends_with(&format!(".{EXTENSION}")) || rest.len() < 10 {
                return None;
            }
            let day = &rest[..10];
            if !is_day(day) {
                return None;
            }
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some(Kept { path: e.path(), day: day.to_owned(), size: meta.len() })
        })
        .collect();
    kept.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.path.cmp(&b.path)));
    kept
}

/// `text` is a day as a file names it (`2026-11-20`).
fn is_day(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// What goes so that what's left keeps `limits` (`today` the newest day kept): the days
/// past the number kept, then the oldest files while the total is over; never `in_use`.
pub fn to_remove(kept: &[Kept], today: &str, limits: Limits, in_use: &Path) -> Vec<PathBuf> {
    let mut days: Vec<&str> = kept.iter().map(|k| k.day.as_str()).collect();
    days.dedup();
    let days_kept = usize::try_from(limits.days).unwrap_or(usize::MAX).max(1);
    let oldest_kept =
        days.iter().rev().filter(|d| **d <= today).nth(days_kept - 1).copied().unwrap_or("");
    let mut gone = Vec::new();
    let mut left: Vec<&Kept> = Vec::new();
    for k in kept {
        if k.path != in_use && k.day.as_str() < oldest_kept {
            gone.push(k.path.clone());
        } else {
            left.push(k);
        }
    }
    let mut total: u64 = left.iter().map(|k| k.size).sum();
    for k in left {
        if total <= limits.total {
            break;
        }
        if k.path != in_use {
            total = total.saturating_sub(k.size);
            gone.push(k.path.clone());
        }
    }
    gone
}

/// Lets go of what [`to_remove`] says, in `dir`; a file another instance still writes (held)
/// is left for later. How many went.
pub fn clean(dir: &Path, today: &str, limits: Limits, in_use: &Path) -> usize {
    // The single file of before (moved here once): gone with the days kept.
    let legacy = dir.join(LEGACY);
    let old = std::fs::metadata(&legacy)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age.as_secs() > u64::from(limits.days) * 86_400);
    let legacy_gone = usize::from(old && std::fs::remove_file(&legacy).is_ok());
    let kept = listed(dir);
    to_remove(&kept, today, limits, in_use)
        .into_iter()
        .filter(|path| {
            let held = File::open(path).is_ok_and(|f| f.try_lock().is_err());
            !held && std::fs::remove_file(path).is_ok()
        })
        .count()
        + legacy_gone
}

#[cfg(test)]
#[path = "../tests/unit/files.rs"]
mod tests;
