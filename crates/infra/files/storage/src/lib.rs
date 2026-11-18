//! Persistence adapters for the domain ports: TOML files written atomically.

pub mod app_sessions;
pub mod atomic;
pub mod consoles;
pub mod data_sources;
pub mod grid_layouts;
pub mod project_frameworks;
pub mod project_levels;
pub mod query_history;
pub mod recent;
pub mod recent_files;
pub mod session;
pub mod settings;

pub use atomic::write_atomic;
pub use consoles::TomlConsoles;
pub use data_sources::TomlDataSources;
pub use grid_layouts::TomlGridLayouts;
pub use project_frameworks::TomlProjectFrameworksStore;
pub use project_levels::TomlProjectLevelsStore;
pub use query_history::TomlQueryHistory;
pub use recent::TomlRecentStore;
pub use recent_files::RecentFilesFile;
pub use session::TomlSessionStore;
pub use settings::TomlSettingsStore;

/// The data folder (local to the machine): the projects' own data, the language servers
/// downloaded once for every project ([`files::places::data_dir`]).
pub fn data_dir() -> Option<std::path::PathBuf> {
    files::places::data_dir()
}

/// From now on every folder of the app is under `root` (the app's tests:
/// [`files::places::isolate`]).
pub fn isolate(root: std::path::PathBuf) {
    files::places::isolate(root);
}

/// The config folder: `~/.config/<app id>` (on Windows, `%APPDATA%\<app id>`): the
/// settings, and what's downloaded once for every project ([`files::places::config_dir`]).
pub fn config_dir() -> Option<std::path::PathBuf> {
    files::places::config_dir()
}
