//! `<project dir>/consoles/consoles.toml`: the TOML adapter for [`ConsoleStore`] — the
//! project's query consoles (their files beside it, `consoles/<source>/<name>.sql`). Written
//! whole, atomically; never a password, never a parameter's value (only the names asked
//! for). A file that can't be read as a list is an error, never written over.
//!
//! The same adapter keeps the project's own SQL files attached to a source (39n:
//! `<project dir>/sql-files.toml`, [`TomlConsoles::sql_files`]): a `[[file]]` per file, its
//! `file` relative to the project's root (always with `/`), its name the file's.
//!
//! ```toml
//! [[console]]
//! id = "7c1e5a2b-9d3f-4e6a-8b0c-2f4d6e8a0b1c"
//! source = "0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10"
//! name = "console"
//! file = "consoles/0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10/console.sql"
//! database = "shop"             # none: the connection's
//! schema = "public"
//! tx = "auto"                   # auto | manual
//! isolation = "read_committed"  # none: the engine's default
//! mode = "playground"           # playground | script
//! last_run_ms = 543
//! params = ["id", "email"]
//! ```

use std::path::{Path, PathBuf};

use database::console::{ConsoleId, ConsoleKind, ConsoleSpec, ConsoleStore, RunMode, TxMode};
use database::{DataSourceId, Isolation};
use toml::Value;

use crate::write_atomic;

/// The project's consoles' list.
#[derive(Debug, Clone)]
pub struct TomlConsoles {
    file: PathBuf,
    kind: ConsoleKind,
}

impl TomlConsoles {
    /// The consoles of the project at `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            file: database::console::folder(root).join(database::console::LIST_FILE),
            kind: ConsoleKind::Console,
        }
    }

    /// The SQL files of the project at `root` attached to a source
    /// (`<project dir>/sql-files.toml`).
    pub fn sql_files(root: &Path) -> Self {
        Self {
            file: root.join(kernel::brand::PROJECT_DIR).join(database::console::SQL_FILES),
            kind: ConsoleKind::Attached,
        }
    }

    /// The list's entries' key (`[[console]]`, `[[file]]`).
    fn key(&self) -> &'static str {
        match self.kind {
            ConsoleKind::Console => "console",
            ConsoleKind::Attached => "file",
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}

fn text(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn spec_of(table: &toml::Table, i: usize, kind: ConsoleKind) -> Result<ConsoleSpec, String> {
    let at = |what: &str| format!("entry {}: {what}", i + 1);
    let id = text(table, "id").and_then(|t| ConsoleId::parse(&t)).ok_or_else(|| at("no id"))?;
    let source = text(table, "source")
        .and_then(|t| DataSourceId::parse(&t))
        .ok_or_else(|| at("no source"))?;
    let (name, file) = match kind {
        ConsoleKind::Console => {
            let name =
                text(table, "name").filter(|n| !n.is_empty()).ok_or_else(|| at("no name"))?;
            let file = text(table, "file")
                .map(PathBuf::from)
                .unwrap_or_else(|| database::console::relative_file(source, &name));
            (name, file)
        }
        // An attached file is known by its path in the project (its name is the file's).
        ConsoleKind::Attached => {
            let file = text(table, "file")
                .filter(|f| !f.is_empty())
                .map(|f| f.split('/').collect::<PathBuf>())
                .ok_or_else(|| at("no file"))?;
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| at("no file"))?;
            (name, file)
        }
    };
    Ok(ConsoleSpec {
        kind,
        id,
        source,
        name,
        file,
        database: text(table, "database"),
        schema: text(table, "schema"),
        tx: text(table, "tx").and_then(|t| TxMode::from_id(&t)).unwrap_or_default(),
        isolation: text(table, "isolation").and_then(|t| Isolation::from_id(&t)),
        mode: text(table, "mode").and_then(|t| RunMode::from_id(&t)).unwrap_or_default(),
        last_run_ms: table
            .get("last_run_ms")
            .and_then(Value::as_integer)
            .and_then(|n| u64::try_from(n).ok()),
        params: table
            .get("params")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
    })
}

fn table_of(spec: &ConsoleSpec) -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("id".into(), Value::String(spec.id.to_string()));
    t.insert("source".into(), Value::String(spec.source.to_string()));
    t.insert("name".into(), Value::String(spec.name.clone()));
    // Always with `/`: the same file on every system.
    let file = spec.file.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>();
    t.insert("file".into(), Value::String(file.join("/")));
    if let Some(database) = &spec.database {
        t.insert("database".into(), Value::String(database.clone()));
    }
    if let Some(schema) = &spec.schema {
        t.insert("schema".into(), Value::String(schema.clone()));
    }
    t.insert("tx".into(), Value::String(spec.tx.id().into()));
    if let Some(isolation) = spec.isolation {
        t.insert("isolation".into(), Value::String(isolation.id().into()));
    }
    t.insert("mode".into(), Value::String(spec.mode.id().into()));
    if let Some(ms) = spec.last_run_ms.and_then(|ms| i64::try_from(ms).ok()) {
        t.insert("last_run_ms".into(), Value::Integer(ms));
    }
    if !spec.params.is_empty() {
        t.insert(
            "params".into(),
            Value::Array(spec.params.iter().cloned().map(Value::String).collect()),
        );
    }
    t
}

impl ConsoleStore for TomlConsoles {
    fn load(&self) -> Result<Vec<ConsoleSpec>, String> {
        let text = match std::fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let table: toml::Table =
            text.parse().map_err(|e: toml::de::Error| e.message().to_owned())?;
        let key = self.key();
        let Some(list) = table.get(key) else { return Ok(Vec::new()) };
        let list = list.as_array().ok_or_else(|| format!("`{key}` isn't a list"))?;
        list.iter()
            .enumerate()
            .map(|(i, entry)| {
                let entry =
                    entry.as_table().ok_or_else(|| format!("entry {}: not a table", i + 1))?;
                spec_of(entry, i, self.kind)
            })
            .collect()
    }

    fn save(&self, consoles: &[ConsoleSpec]) -> Result<(), String> {
        let mut root = toml::Table::new();
        root.insert(
            self.key().into(),
            Value::Array(consoles.iter().map(|c| Value::Table(table_of(c))).collect()),
        );
        write_atomic(&self.file, &root.to_string()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[path = "../tests/unit/consoles.rs"]
mod tests;
