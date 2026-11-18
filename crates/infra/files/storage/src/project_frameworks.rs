//! `<project dir>/project.toml`, `[frameworks]`: the TOML adapter for [`ProjectFrameworksStore`].
//!
//! ```toml
//! [frameworks]
//! laravel = true   # it is Laravel, whatever was detected
//! htmx = false
//! ```

use std::io;
use std::path::{Path, PathBuf};

use settings_model::ProjectFrameworksStore;

use crate::write_atomic;

#[derive(Debug, Default, Clone, Copy)]
pub struct TomlProjectFrameworksStore;

fn file(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join("project.toml")
}

impl ProjectFrameworksStore for TomlProjectFrameworksStore {
    fn load(&self, root: &Path) -> Vec<(String, bool)> {
        let table: toml::Table = std::fs::read_to_string(file(root))
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        table
            .get("frameworks")
            .and_then(|f| f.as_table())
            .into_iter()
            .flatten()
            .filter_map(|(id, on)| Some((id.clone(), on.as_bool()?)))
            .collect()
    }

    fn save(&self, root: &Path, choices: &[(String, bool)]) -> io::Result<()> {
        let path = file(root);
        // The other keys of `project.toml` stay; an invalid file is not overwritten.
        let mut table = match std::fs::read_to_string(&path) {
            Ok(text) => text
                .parse::<toml::Table>()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            Err(_) => toml::Table::new(),
        };
        table.remove("frameworks");
        if !choices.is_empty() {
            let frameworks: toml::Table =
                choices.iter().map(|(id, on)| (id.clone(), toml::Value::Boolean(*on))).collect();
            table.insert("frameworks".into(), toml::Value::Table(frameworks));
        }
        write_atomic(&path, &table.to_string())
    }
}

#[cfg(test)]
#[path = "../tests/unit/project_frameworks.rs"]
mod tests;
