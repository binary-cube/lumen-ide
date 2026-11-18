//! The workbench's ports answered by the local adapters: the disk (`fs-local`) and the TOML
//! files in the project's folder (`storage`).

use std::sync::Arc;

use crate::workbench::Ports;

impl Ports {
    /// The local disk and the project's own files.
    pub fn local() -> Self {
        Self {
            disk: Arc::new(fs_local::LocalFileSystem),
            watching: Arc::new(fs_local::LocalWatching),
            sessions: Arc::new(storage::TomlSessionStore),
            recent_files: Arc::new(storage::RecentFilesFile),
            project_settings: Arc::new(|file| {
                Arc::new(storage::TomlSettingsStore::new(Some(file)))
            }),
            levels: Arc::new(storage::TomlProjectLevelsStore),
            frameworks: Arc::new(storage::TomlProjectFrameworksStore),
        }
    }
}
