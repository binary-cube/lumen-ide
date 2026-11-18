//! What the workbench reaches the outside through: the domain's ports, given when it's made
//! (`Ports::local()` in the wiring: the local disk and the TOML files; tests may give fakes).
//! The workbench never names an adapter.

use std::path::PathBuf;
use std::sync::Arc;

use settings_model::SettingsStore;

/// The disk as the workbench uses it: listing and reading (`workspace`), opening and saving
/// documents (`document`), `.editorconfig` (`settings_model`) and moving entries
/// (`refactor`).
pub trait Disk:
    workspace::FileSystem
    + document::DocumentStorage
    + settings_model::EditorConfigSource
    + refactor::applying::Entries
    + std::fmt::Debug
    + Send
    + Sync
{
}

impl<T> Disk for T where
    T: workspace::FileSystem
        + document::DocumentStorage
        + settings_model::EditorConfigSource
        + refactor::applying::Entries
        + std::fmt::Debug
        + Send
        + Sync
{
}

/// A project's own settings store, for its settings file (`<project dir>/project.toml`).
pub type ProjectSettings = Arc<dyn Fn(PathBuf) -> Arc<dyn SettingsStore> + Send + Sync>;

/// The workbench's ports (cheap to clone: each window's workbench gets the same).
#[derive(Clone)]
pub struct Ports {
    /// The files: listing, reading, writing, moving.
    pub disk: Arc<dyn Disk>,
    /// Watching the open project's files.
    pub watching: Arc<dyn workspace::FileWatching>,
    /// Where each project's session (its tabs, groups, tool windows) is kept.
    pub sessions: Arc<dyn workbench_model::SessionStore + Send + Sync>,
    /// Where each project's recently opened files are kept.
    pub recent_files: Arc<dyn workspace::RecentFiles>,
    /// A project's own settings, kept in its file.
    pub project_settings: ProjectSettings,
    /// A project's language levels, kept in its settings file.
    pub levels: Arc<dyn settings_model::ProjectLevelsStore>,
    /// A project's framework choices, kept in its settings file.
    pub frameworks: Arc<dyn settings_model::ProjectFrameworksStore>,
}

impl std::fmt::Debug for Ports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ports")
            .field("disk", &self.disk)
            .field("watching", &self.watching)
            .field("sessions", &self.sessions)
            .field("recent_files", &self.recent_files)
            .field("levels", &self.levels)
            .field("frameworks", &self.frameworks)
            .finish_non_exhaustive()
    }
}
