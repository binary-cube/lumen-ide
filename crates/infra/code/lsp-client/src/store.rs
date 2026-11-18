//! The language servers downloaded once for every project, window and version of the app,
//! in the user's data folder (`<data>/lsp/`, the bundle's layout):
//!
//! ```text
//! node/<version>/…               Node (bin/node, node.exe on Windows)
//! php/<version>/…                PHP
//! servers/<server>/<version>/…   a server's files
//! <each of them>/.installed      what's there: its component, version, recipe, size
//! .downloads/<sha256>.part       a large download under way (resumed after a break)
//! .locks/<component>.lock        held while it's installed or removed (by any process)
//! refs/<app version>.toml        the folders an app version uses (kept from the cleanup)
//! ```
//!
//! A folder is only ever there whole: made beside its place and renamed into it, its
//! marker removed first when it goes. [`collect`] removes the versions nothing uses.

use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// A component's marker, in its folder.
pub const MARKER: &str = ".installed";

/// How long another app version's references keep what they name.
pub const REFS_KEPT: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// How long a broken-off download is kept to be resumed.
pub const PARTS_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// How long a temporary folder may stand before it's taken as left by a crash.
pub const TEMP_KEPT: Duration = Duration::from_secs(24 * 60 * 60);

/// What a component's folder holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub name: String,
    pub version: String,
    /// Its recipe's fingerprint (a different one: made from another recipe).
    #[serde(default)]
    pub fingerprint: String,
    /// Bytes on disk.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub platform: String,
}

/// Folder `dir`'s marker, if it has a readable one.
pub fn marker(dir: &Path) -> Option<Marker> {
    toml::from_str(&std::fs::read_to_string(dir.join(MARKER)).ok()?).ok()
}

/// `marker` written in `dir`.
pub fn write_marker(dir: &Path, marker: &Marker) -> Result<(), String> {
    let text = toml::to_string(marker).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MARKER), text).map_err(|e| format!("{}: {e}", dir.display()))
}

/// A component found installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub dir: PathBuf,
    pub version: String,
    /// Bytes on disk.
    pub size: u64,
    /// The version and recipe this app pins (otherwise another one, which still runs).
    pub current: bool,
}

/// A name usable as one folder (anything else replaced).
pub fn safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    if s.trim_matches('.').is_empty() { "_".to_owned() } else { s }
}

/// A version's numbers, to order versions (`1.18.5` → `[1, 18, 5]`).
pub fn version_key(version: &str) -> Vec<u64> {
    version.split(['.', '-']).map(|p| p.parse().unwrap_or(0)).collect()
}

/// `relative` (`/`-separated) under `root`.
pub fn path_in(root: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Component `name` (at `version`, of recipe `fingerprint`) in its versions' folder `group`
/// (relative to `root`): its pinned version if it's there, else the newest other one.
pub fn find(
    root: &Path,
    group: &str,
    name: &str,
    version: &str,
    fingerprint: &str,
) -> Option<Found> {
    let versions = path_in(root, group);
    let pinned = versions.join(safe(version));
    if let Some(m) = marker(&pinned).filter(|m| m.name == name && m.version == version) {
        let current = m.fingerprint == fingerprint;
        return Some(Found { dir: pinned, version: m.version, size: m.size, current });
    }
    let mut others: Vec<Found> = std::fs::read_dir(&versions)
        .ok()?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let m = marker(&e.path()).filter(|m| m.name == name)?;
            Some(Found { dir: e.path(), version: m.version, size: m.size, current: false })
        })
        .collect();
    others.sort_by_key(|f| std::cmp::Reverse(version_key(&f.version)));
    others.into_iter().next()
}

/// A folder's size (bytes).
pub fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// A component's lock: held while it's installed or removed, by whichever process (two
/// windows, two versions of the app); let go when dropped.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

impl Lock {
    fn file(root: &Path, name: &str) -> Result<File, String> {
        let dir = root.join(".locks");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(format!("{}.lock", safe(name)));
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Component `name`'s lock under `root`, waited for (`waiting` once if another holds
    /// it); `stop` gives up.
    pub fn acquire(
        root: &Path,
        name: &str,
        stop: &AtomicBool,
        waiting: &dyn Fn(),
    ) -> Result<Self, String> {
        let file = Self::file(root, name)?;
        let mut told = false;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(e)) => return Err(format!("{name}'s lock: {e}")),
            }
            if !told {
                waiting();
                told = true;
            }
            if stop.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Component `name`'s lock, if nobody holds it now.
    pub fn try_acquire(root: &Path, name: &str) -> Option<Self> {
        let file = Self::file(root, name).ok()?;
        file.try_lock().ok()?;
        Some(Self { _file: file })
    }
}

/// Folder `dir` removed: its marker first (never a half folder taken as installed).
pub fn remove_dir(dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(dir.join(MARKER)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

/// The folders holding versions under `root`: `node`, `php`, `servers/<each>`.
pub fn groups(root: &Path) -> Vec<String> {
    let mut out = vec!["node".to_owned(), "php".to_owned()];
    for entry in std::fs::read_dir(root.join("servers")).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && entry.file_type().is_ok_and(|t| t.is_dir()) {
            out.push(format!("servers/{name}"));
        }
    }
    out
}

/// The name a group's components lock by (`servers/vtsls` → `vtsls`).
fn lock_name(group: &str) -> &str {
    group.rsplit('/').next().unwrap_or(group)
}

/// What an app version (`key`) uses (folders relative to `root`): written now (its time is
/// when it was last seen).
pub fn write_refs(root: &Path, key: &str, folders: &BTreeSet<String>) -> Result<(), String> {
    let dir = root.join("refs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = toml::to_string(&Refs { folders: folders.iter().cloned().collect() })
        .map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.toml", safe(key)));
    let temp = dir.join(format!(".{}.{}", safe(key), kernel::id::new_uuid()));
    std::fs::write(&temp, text).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Debug, Serialize, Deserialize)]
struct Refs {
    #[serde(default)]
    folders: Vec<String>,
}

fn older_than(path: &Path, now: SystemTime, age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| now.duration_since(t).is_ok_and(|d| d > age))
}

/// Removes under `root` what nothing uses (see the module's doc): versions `keep` doesn't
/// name (relative folders) nor another app version's references seen in the last
/// [`REFS_KEPT`] — each under its component's lock, skipped while another process holds it
/// —, references older than that, temporary folders older than [`TEMP_KEPT`], downloads
/// broken off longer than [`PARTS_KEPT`] ago. `own`: this app version's references' key
/// (not read: `keep` says it). What was removed.
pub fn collect(root: &Path, keep: &BTreeSet<String>, own: &str, now: SystemTime) -> Vec<PathBuf> {
    let mut kept = keep.clone();
    let mut removed = Vec::new();
    let own_file = format!("{}.toml", safe(own));
    for entry in std::fs::read_dir(root.join("refs")).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == own_file {
            continue;
        }
        if older_than(&path, now, REFS_KEPT)
            || name.starts_with('.') && older_than(&path, now, TEMP_KEPT)
        {
            if std::fs::remove_file(&path).is_ok() {
                removed.push(path);
            }
            continue;
        }
        if let Some(refs) =
            std::fs::read_to_string(&path).ok().and_then(|t| toml::from_str::<Refs>(&t).ok())
        {
            kept.extend(refs.folders);
        }
    }
    for group in groups(root) {
        for entry in std::fs::read_dir(path_in(root, &group)).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if name.starts_with('.') {
                // An install or removal broken off (a crash): old enough, nobody's.
                if older_than(&path, now, TEMP_KEPT)
                    && let Some(_lock) = Lock::try_acquire(root, lock_name(&group))
                    && std::fs::remove_dir_all(&path).is_ok()
                {
                    removed.push(path);
                }
                continue;
            }
            if kept.contains(&format!("{group}/{name}")) {
                continue;
            }
            let Some(_lock) = Lock::try_acquire(root, lock_name(&group)) else { continue };
            if remove_dir(&path).is_ok() {
                removed.push(path);
            }
        }
        // A server's folder left empty.
        if group.starts_with("servers/") {
            let _ = std::fs::remove_dir(path_in(root, &group));
        }
    }
    for entry in std::fs::read_dir(root.join(".downloads")).into_iter().flatten().flatten() {
        let path = entry.path();
        let partial = path.extension().is_some_and(|e| e == "part");
        let age = if partial { PARTS_KEPT } else { TEMP_KEPT };
        if older_than(&path, now, age) && std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

#[cfg(test)]
#[path = "../tests/unit/store.rs"]
mod tests;
