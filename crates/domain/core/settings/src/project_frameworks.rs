//! A project's framework choices as saved (`<project dir>/project.toml`, `[frameworks]`): `true`
//! forces a framework on, `false` off, when the detector got it wrong. Read and written
//! through the [`ProjectFrameworksStore`] port (the TOML adapter lives in `storage`).

use std::path::Path;

/// Where a project's framework choices are kept (port).
pub trait ProjectFrameworksStore: std::fmt::Debug + Send + Sync {
    /// (framework id, on), e.g. `("laravel", true)`.
    fn load(&self, root: &Path) -> Vec<(String, bool)>;
    /// Must keep whatever else the project file holds, and refuse to overwrite a file it
    /// can't parse.
    fn save(&self, root: &Path, choices: &[(String, bool)]) -> std::io::Result<()>;
}
