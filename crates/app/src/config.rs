//! Where the app keeps its files: the config folder (`~/.config/<app id>`, on Windows
//! `%APPDATA%\<app id>`) and the user settings store (`settings.toml`, one section per
//! subsystem: `[general]`, `[appearance]`, `[search]`, `[modules]`).

use std::path::PathBuf;
use std::sync::OnceLock;

use settings_model::SettingsService;

/// The config folder: `~/.config/<app id>` (on Windows, `%APPDATA%\<app id>`).
pub fn config_dir() -> Option<PathBuf> {
    isolate_tests();
    storage::config_dir()
}

/// In the app's tests, every folder of the app (the config, the data) under a temporary one
/// of the process (the modules' too, through `storage`): no test reads or writes the user's
/// settings, downloads or projects' data. Nothing outside the tests.
pub fn isolate_tests() {
    if cfg!(test) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let temp = std::env::temp_dir();
            sweep_test_homes(&temp, std::time::Duration::from_secs(600));
            let root = temp.join(format!("{TEST_HOME}{}", std::process::id()));
            // A folder left by an earlier process with the same id: from scratch.
            let _ = std::fs::remove_dir_all(&root);
            storage::isolate(root);
        });
    }
}

/// The tests' folders' names (then the process's id).
const TEST_HOME: &str = "app-test-home-";

/// The tests' folders of earlier processes in `temp` older than `age` deleted (a test run
/// takes a minute or two: one still running keeps its own).
fn sweep_test_homes(temp: &std::path::Path, age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(temp) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(TEST_HOME) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|elapsed| elapsed >= age);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The app's data folder (local, not synced): `~/Library/Application Support/<id>` on macOS,
/// `%LOCALAPPDATA%\<id>` on Windows, `~/.local/share/<id>` on Linux.
pub fn data_dir() -> Option<PathBuf> {
    isolate_tests();
    storage::data_dir()
}

/// A project's data folder (outside the project): `<data>/projects/<name>-<hash of its path>`.
pub fn project_data_dir(root: &std::path::Path) -> Option<PathBuf> {
    let name = kernel::paths::file_name(root);
    let hash = kernel::hash::fnv64(root.to_string_lossy().as_bytes());
    Some(data_dir()?.join("projects").join(format!("{name}-{hash:016x}")))
}

/// The user settings: one service for the whole process (the settings are declared in
/// `crate::settings`); in the tests one per thread, each with a file of its own in the
/// tests' folder (never the user's, never another test's).
pub fn settings() -> &'static SettingsService {
    if cfg!(test) {
        thread_local! {
            static TEST: &'static SettingsService = {
                let id = format!("{:?}", std::thread::current().id())
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "");
                let file = config_dir().map(|d| d.join(format!("settings-{id}.toml")));
                if let Some(file) = &file {
                    let _ = std::fs::remove_file(file);
                }
                let store = storage::TomlSettingsStore::new(file);
                Box::leak(Box::new(SettingsService::new(std::sync::Arc::new(store))))
            };
        }
        return TEST.with(|s| *s);
    }
    static SERVICE: OnceLock<SettingsService> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let store = storage::TomlSettingsStore::written_behind(
            config_dir().map(|d| d.join("settings.toml")),
        );
        SettingsService::new(std::sync::Arc::new(store))
    })
}

/// The thread reading `settings.toml` again (the Task Manager's UI row).
pub const RELOAD_THREAD: &str = "settings-reload";

/// Reads the settings again in the background (the file may have been edited by hand):
/// one read at a time; `wake` redraws when something changed.
pub fn reload_settings(wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static READING: AtomicBool = AtomicBool::new(false);
    if READING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new().name(RELOAD_THREAD.into()).spawn(move || {
        let changed = refresh_from_outside(settings());
        READING.store(false, Ordering::Release);
        if !changed.is_empty() {
            tracing::debug!(?changed, "settings edited outside the app");
            wake();
        }
    });
    if spawned.is_err() {
        READING.store(false, Ordering::Release);
    }
}

/// How many times the settings were found changed by another program (the file edited by
/// hand): every window resolves again what it derived from them (its theme with its
/// options, the overlay backdrop, its keymap) when this moves.
static OUTSIDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// See [`OUTSIDE`].
pub fn outside_changes() -> u64 {
    OUTSIDE.load(std::sync::atomic::Ordering::Acquire)
}

/// `settings` read again from the file: the sections that changed (announced as
/// `SettingChanged { key: "*" }`, which the settings blocks apply), counted in
/// [`outside_changes`] once they're applied.
pub fn refresh_from_outside(settings: &SettingsService) -> Vec<String> {
    let changed = settings.refresh();
    if !changed.is_empty() {
        OUTSIDE.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
    changed
}

#[cfg(test)]
#[path = "../tests/unit/config.rs"]
mod tests;
