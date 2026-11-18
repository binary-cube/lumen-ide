//! Where the app keeps its own files, in one place for every adapter and module: the config
//! folder (`~/.config/<app id>`, on Windows `%APPDATA%\<app id>`), the data folder (local
//! to the machine) and the cache folder. The app's tests move all of them under a
//! temporary folder ([`isolate`]): no test reads or writes the user's.
//!
//! A few files were once kept in the system's config folder (`dirs::config_dir()`: on macOS
//! `~/Library/Application Support/<app id>`): [`config_file`] goes on using such a file
//! where it is until the config folder has its own (nothing is moved or lost).

use std::path::PathBuf;
use std::sync::OnceLock;

/// Set by [`isolate`]: every folder of the app under it.
static ISOLATED: OnceLock<PathBuf> = OnceLock::new();

/// From now on [`config_dir`], [`data_dir`] and [`cache_dir`] are `root/config`,
/// `root/data` and `root/cache` (the app's tests). Set once: a second call changes nothing.
pub fn isolate(root: PathBuf) {
    let _ = ISOLATED.set(root);
}

/// The root [`isolate`] set, if any.
pub fn isolated() -> Option<&'static PathBuf> {
    ISOLATED.get()
}

/// The config folder: `~/.config/<app id>` (on Windows, `%APPDATA%\<app id>`): the
/// settings, and what's downloaded once for every project (the semantic search's model).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(root) = ISOLATED.get() {
        return Some(root.join("config"));
    }
    if cfg!(windows) {
        dirs::config_dir().map(|d| d.join(kernel::brand::ID))
    } else {
        dirs::home_dir().map(|d| d.join(".config").join(kernel::brand::ID))
    }
}

/// The data folder: the system's folder for an app's own data, local to the machine —
/// `~/Library/Application Support/<app id>` on macOS, `%LOCALAPPDATA%\<app id>` on Windows
/// (no admin rights), `~/.local/share/<app id>` on Linux.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(root) = ISOLATED.get() {
        return Some(root.join("data"));
    }
    dirs::data_local_dir().map(|d| d.join(kernel::brand::ID))
}

/// The cache folder: what can be made again (the terminal's prompt files).
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(root) = ISOLATED.get() {
        return Some(root.join("cache"));
    }
    dirs::cache_dir().map(|d| d.join(kernel::brand::ID))
}

/// File `name` of the config folder; one kept before in the system's config folder (where
/// it differs: macOS) and not yet in this one is used where it is — never moved, nothing
/// written by asking.
pub fn config_file(name: &str) -> Option<PathBuf> {
    let file = config_dir()?.join(name);
    if ISOLATED.get().is_some() {
        return Some(file);
    }
    let legacy = dirs::config_dir().map(|d| d.join(kernel::brand::ID).join(name));
    Some(resolved(legacy, file))
}

/// `file`, unless only `legacy` exists (then that one).
pub fn resolved(legacy: Option<PathBuf>, file: PathBuf) -> PathBuf {
    match legacy {
        Some(legacy) if !file.exists() && legacy.exists() => legacy,
        _ => file,
    }
}

#[cfg(test)]
#[path = "../tests/unit/places.rs"]
mod tests;
