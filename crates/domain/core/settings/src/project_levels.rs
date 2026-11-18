//! A project's language levels as saved (`<project dir>/project.toml`): the level chosen per
//! language and per-file exceptions, read and written through the [`ProjectLevelsStore`]
//! port (the TOML adapter lives in `storage`).

use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedLevels {
    /// (language, level), e.g. `("php", "8.3")`.
    pub levels: Vec<(String, String)>,
    /// (path relative to the project with `/`, language, level).
    pub files: Vec<(String, String, String)>,
}

/// Where a project's levels are kept (port).
pub trait ProjectLevelsStore: std::fmt::Debug + Send + Sync {
    fn load(&self, root: &Path) -> SavedLevels;
    /// Must keep whatever else the project file holds, and refuse to overwrite a file it
    /// can't parse.
    fn save(&self, root: &Path, levels: &SavedLevels) -> std::io::Result<()>;
}
