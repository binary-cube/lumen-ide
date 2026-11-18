//! `<project dir>/project.toml`: the TOML adapter for [`ProjectLevelsStore`].
//!
//! ```toml
//! [levels]
//! php = "8.3"
//!
//! [files."legacy/import.php"]
//! php = "7.4"
//! ```

use std::io;
use std::path::{Path, PathBuf};

use settings_model::{ProjectLevelsStore, SavedLevels};

use crate::write_atomic;

#[derive(Debug, Default, Clone, Copy)]
pub struct TomlProjectLevelsStore;

fn file(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join("project.toml")
}

impl ProjectLevelsStore for TomlProjectLevelsStore {
    fn load(&self, root: &Path) -> SavedLevels {
        let table: toml::Table = std::fs::read_to_string(file(root))
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        let mut saved = SavedLevels::default();
        for (language, level) in
            table.get("levels").and_then(|l| l.as_table()).into_iter().flatten()
        {
            if let Some(level) = level.as_str() {
                saved.levels.push((language.clone(), level.to_owned()));
            }
        }
        for (path, entry) in table.get("files").and_then(|f| f.as_table()).into_iter().flatten() {
            for (language, level) in entry.as_table().into_iter().flatten() {
                if let Some(level) = level.as_str() {
                    saved.files.push((path.clone(), language.clone(), level.to_owned()));
                }
            }
        }
        saved
    }

    fn save(&self, root: &Path, saved: &SavedLevels) -> io::Result<()> {
        let path = file(root);
        // The other keys of `project.toml` stay; an invalid file is not overwritten.
        let mut table = match std::fs::read_to_string(&path) {
            Ok(text) => text
                .parse::<toml::Table>()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            Err(_) => toml::Table::new(),
        };
        table.remove("files");
        let levels: toml::Table =
            saved.levels.iter().map(|(l, v)| (l.clone(), toml::Value::String(v.clone()))).collect();
        table.insert("levels".into(), toml::Value::Table(levels));
        let mut files = toml::Table::new();
        for (path, language, level) in &saved.files {
            let entry =
                files.entry(path.clone()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if let Some(entry) = entry.as_table_mut() {
                entry.insert(language.clone(), toml::Value::String(level.clone()));
            }
        }
        if !files.is_empty() {
            table.insert("files".into(), toml::Value::Table(files));
        }
        write_atomic(&path, &table.to_string())
    }
}

#[cfg(test)]
#[path = "../tests/unit/project_levels.rs"]
mod tests;
