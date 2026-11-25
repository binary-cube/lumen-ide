//! The engine on disk, under the app's data (`<data>/web-engine/`):
//!
//! - `<version>/`: one engine, unpacked whole (its `engine.json` says which and what it
//!   speaks); a host running from it holds the lock on its `.in-use`;
//! - `state.json`: which one is chosen (`current`), the one before it kept until the new one
//!   worked once (`previous`), those that failed to start here (`bad`, and the app's version
//!   that judged them) — written whole and synced through a temporary file renamed over it
//!   (a break leaves the old choice), changed only while holding `state.lock` (two windows,
//!   two instances of the app never lose each other's change);
//! - `.staging/`: a download and an unpacking under way, while its `.lock` is held (by any
//!   instance of the app) — and the move of what was unpacked into its place and its choice;
//!   left behind by a break, what was unpacked is cleaned once nobody holds it, a download
//!   broken off (`*.part`) kept [`PART_KEPT`] to be resumed;
//! - `profile/`: the pages' storage (draw.io's), apart from any version so it outlives them.
//!
//! Disk only: on a worker.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use web_engine_protocol::EngineInfo;
use web_engine_versions::Version;

/// The chosen engine and its history here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// The version chosen: the next host starts from it.
    pub current: Option<String>,
    /// The one chosen before it, kept until the current one started once (it comes back if
    /// the current one fails to start [`FAILED_STARTS`] times in a row).
    pub previous: Option<String>,
    /// The versions that failed to start here (as the app's version `judged_by` saw them).
    #[serde(default)]
    pub bad: Vec<String>,
    /// The app's version that found `bad` bad: another (a newer app) tries them again.
    #[serde(default)]
    pub judged_by: Option<String>,
    /// The current one's starts that failed in a row (two: it's bad, the previous comes back).
    #[serde(default)]
    pub failures: u32,
}

impl State {
    /// The versions this app (its version `app`) won't take again: those it found bad itself.
    pub fn refused(&self, app: &str) -> &[String] {
        if self.judged_by.as_deref() == Some(app) { &self.bad } else { &[] }
    }
}

/// Failed starts in a row that make a version bad.
pub const FAILED_STARTS: u32 = 2;

/// The files and folders of the engine's place.
pub const STATE: &str = "state.json";
pub const STATE_LOCK: &str = "state.lock";
pub const STAGING: &str = ".staging";
pub const STAGING_LOCK: &str = ".lock";
pub const PROFILE: &str = "profile";
pub const ENGINE: &str = web_engine_protocol::ENGINE_INFO;
pub const IN_USE: &str = ".in-use";

/// How long a download broken off (its `.part` in `.staging`) is kept to be resumed: one
/// older is let go by the next clean (a version long since replaced).
pub const PART_KEPT: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// The end of a download's name in `.staging`.
const PART: &str = ".part";

/// How often a download waiting for another one (another window's, another instance's) to
/// finish looks again, and whether it was stopped meanwhile.
const STAGING_RETRY: Duration = Duration::from_millis(100);

/// The engine's place under the app's data folder `data`.
pub fn root(data: &Path) -> PathBuf {
    data.join("web-engine")
}

/// Where the pages keep their storage.
pub fn profile(root: &Path) -> PathBuf {
    root.join(PROFILE)
}

/// The host's program in an engine's folder, as this system's package lays it out.
pub fn program(version_dir: &Path) -> PathBuf {
    web_engine_versions::program(version_dir, std::env::consts::OS)
}

/// The state as written (none, or unreadable: nothing chosen).
pub fn read_state(root: &Path) -> State {
    std::fs::read(root.join(STATE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// `state` written whole and synced: a temporary file of this write renamed over the old (a
/// break leaves the old). Changes go through [`update_state`].
fn write_state(root: &Path, state: &State) -> std::io::Result<()> {
    let text = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    files::write_atomic(&root.join(STATE), &text)
}

/// The state read, changed by `change` and written back if it changed, while holding
/// `state.lock` (every window and instance of the app takes it to change the state).
/// What `change` gave.
pub fn update_state<T>(root: &Path, change: impl FnOnce(&mut State) -> T) -> std::io::Result<T> {
    std::fs::create_dir_all(root)?;
    let lock =
        File::options().create(true).truncate(false).write(true).open(root.join(STATE_LOCK))?;
    lock.lock()?;
    let before = read_state(root);
    let mut state = before.clone();
    let given = change(&mut state);
    if state != before {
        write_state(root, &state)?;
    }
    Ok(given)
}

/// The engine unpacked in `dir`, if it's whole: what it says of itself, and its program.
pub fn engine_in(dir: &Path) -> Option<EngineInfo> {
    let engine: EngineInfo = serde_json::from_slice(&std::fs::read(dir.join(ENGINE)).ok()?).ok()?;
    program(dir).is_file().then_some(engine)
}

/// The chosen engine speaking `protocol`, whole: its folder and what it is. A state naming
/// anything but a version (a path, a name of the user's) chooses nothing.
pub fn chosen(root: &Path, protocol: u32) -> Option<(PathBuf, EngineInfo)> {
    let state = read_state(root);
    let version = state.current.filter(|v| Version::parse(v).is_some())?;
    let dir = root.join(&version);
    let engine = engine_in(&dir).filter(|e| e.version == version && e.protocol == protocol)?;
    Some((dir, engine))
}

/// `version` (unpacked under `root`) chosen; the one chosen before kept as `previous`
/// until it worked.
pub fn choose(root: &Path, version: &str) -> std::io::Result<()> {
    update_state(root, |state| {
        if state.current.as_deref() != Some(version) {
            state.previous = state.current.take();
            state.current = Some(version.to_owned());
            state.failures = 0;
        }
    })
}

/// `version` started and greeted: it's good, the one before it isn't needed any more.
pub fn worked(root: &Path, version: &str) -> std::io::Result<()> {
    update_state(root, |state| {
        if state.current.as_deref() == Some(version) {
            state.failures = 0;
            state.previous = None;
        }
    })
}

/// `version` didn't start (its own failure, judged by the app's version `app`): past
/// [`FAILED_STARTS`] in a row it's bad and the one before comes back. `true`: it was given
/// up.
pub fn failed(root: &Path, version: &str, app: &str) -> std::io::Result<bool> {
    update_state(root, |state| {
        if state.current.as_deref() != Some(version) {
            return false;
        }
        state.failures = state.failures.saturating_add(1);
        if state.failures < FAILED_STARTS {
            return false;
        }
        // Bad as this app sees it: what an older app found bad is forgotten.
        if state.judged_by.as_deref() != Some(app) {
            state.bad.clear();
            state.judged_by = Some(app.to_owned());
        }
        if !state.bad.iter().any(|b| b == version) {
            state.bad.push(version.to_owned());
        }
        state.current = state.previous.take();
        state.failures = 0;
        true
    })
}

/// The hold on `.staging`: a download and its unpacking go on while it lives (one at a time,
/// for every window and instance of the app); nothing cleans `.staging` meanwhile.
#[derive(Debug)]
pub struct Staging {
    dir: PathBuf,
    _lock: File,
}

impl Staging {
    /// The staging folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// `.staging` held, waiting while another download holds it (`Ok(None)`: `stop` was set
/// meanwhile).
pub fn hold_staging(root: &Path, stop: &AtomicBool) -> std::io::Result<Option<Staging>> {
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if let Some(staging) = try_hold_staging(root)? {
            return Ok(Some(staging));
        }
        std::thread::sleep(STAGING_RETRY);
    }
}

/// `.staging` held if nobody holds it (`Ok(None)`: a download is under way).
pub fn try_hold_staging(root: &Path) -> std::io::Result<Option<Staging>> {
    let dir = root.join(STAGING);
    std::fs::create_dir_all(&dir)?;
    let lock =
        File::options().create(true).truncate(false).write(true).open(dir.join(STAGING_LOCK))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(Staging { dir, _lock: lock })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// What `clean` let go.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// The versions removed.
    pub versions: Vec<String>,
    /// Something a break left in `.staging` was removed.
    pub staging: bool,
}

/// What isn't needed let go: the versions neither chosen nor kept as the one before, that
/// no host runs from (their `.in-use` not locked); what a break left in `.staging` (but a
/// download to resume). The profile and the state stay. Nothing at all while a download
/// holds `.staging` (any instance's): it moves a version into place and chooses it under
/// that hold, so a version just put there is never taken for one nobody chose.
pub fn clean(root: &Path) -> Cleaned {
    let mut cleaned = Cleaned::default();
    if !root.is_dir() {
        return cleaned;
    }
    let staging = match try_hold_staging(root) {
        Ok(Some(staging)) => staging,
        Ok(None) => return cleaned,
        Err(e) => {
            tracing::warn!(error = %e, "the web engine's place wasn't cleaned");
            return cleaned;
        }
    };
    cleaned.staging = clear_staging(&staging, std::time::SystemTime::now());
    // Read while holding its lock: never a state half changed by another window.
    let state = match update_state(root, |state| state.clone()) {
        Ok(state) => state,
        Err(e) => {
            tracing::warn!(error = %e, "the web engine's state can't be read to clean");
            return cleaned;
        }
    };
    let kept = [state.current.as_deref(), state.previous.as_deref()];
    for (name, path) in version_dirs(root) {
        if kept.contains(&Some(name.as_str())) || in_use(&path) {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => cleaned.versions.push(name),
            Err(e) => tracing::warn!(error = %e, "an old web engine wasn't removed"),
        }
    }
    drop(staging);
    cleaned
}

/// The engines' folders under `root`, by version: only a folder named as a version (never
/// the profile, the state, anything else).
pub fn version_dirs(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            (Version::parse(&name).is_some() && path.is_dir()).then_some((name, path))
        })
        .collect()
}

/// What a break left in `.staging` (held by the caller) removed: what was being unpacked,
/// and downloads broken off more than [`PART_KEPT`] before `now` (the younger ones are
/// resumed by the next download); its lock stays (a held file can't be removed on
/// Windows). Whether anything was removed.
fn clear_staging(staging: &Staging, now: std::time::SystemTime) -> bool {
    let Ok(entries) = std::fs::read_dir(staging.dir()) else { return false };
    let mut removed = false;
    for entry in entries.flatten() {
        if entry.file_name() == STAGING_LOCK {
            continue;
        }
        let path = entry.path();
        let part = entry.file_name().to_string_lossy().ends_with(PART) && path.is_file();
        let young = || {
            let modified = entry.metadata().and_then(|m| m.modified());
            modified.is_ok_and(|at| now.duration_since(at).unwrap_or_default() < PART_KEPT)
        };
        if part && young() {
            continue;
        }
        let gone = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed |= gone.is_ok();
    }
    removed
}

/// A host runs from `dir` (it holds its `.in-use`'s lock), or it can't be told (kept: only
/// a version known unused is removed).
fn in_use(dir: &Path) -> bool {
    match std::fs::OpenOptions::new().write(true).create(false).open(dir.join(IN_USE)) {
        Ok(file) => file.try_lock().is_err(),
        // Never started from: nobody runs from it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/install/layout.rs"]
mod tests;
