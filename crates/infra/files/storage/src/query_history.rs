//! `<project dir>/database/history/<source>.toml`: the TOML adapter for [`HistoryStore`] — a
//! data source's query history in the project (plan 39i). Each run is a `[[entry]]` block
//! appended at the end (the file stays valid TOML, never rewritten for a run); the file is
//! rewritten whole, atomically, only to compact it or after a deletion. Never a password
//! (the text's secrets are masked before), never a parameter's value (only the names).
//!
//! A file that can't be read is never written over: kept beside as `<source>.toml.broken`
//! (`.broken-2`… when one is there already), the blocks that can still be read taken from
//! it and written again.
//!
//! ```toml
//! [[entry]]
//! id = "7c1e5a2b-9d3f-4e6a-8b0c-2f4d6e8a0b1c"
//! console = "0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10"
//! database = "shop"
//! schema = "public"
//! at_ms = 1793440823698
//! duration_ms = 28
//! rows = 9
//! outcome = "ok"                # ok | error | cancelled
//! params = [":id"]
//! text = "SELECT * FROM orders WHERE id = :id"
//! ```

use std::io::Write as _;
use std::path::{Path, PathBuf};

use database::DataSourceId;
use database::console::ConsoleId;
use database::history::{HistoryEntry, HistoryId, HistoryStore, Loaded, Outcome};
use toml::Value;

use crate::write_atomic;

/// The folder of the histories, in the project's own folder.
pub const DIR: &str = "database/history";

/// The project's query histories, a file per source.
#[derive(Debug, Clone)]
pub struct TomlQueryHistory {
    dir: PathBuf,
}

impl TomlQueryHistory {
    /// The histories of the project at `root`.
    pub fn new(root: &Path) -> Self {
        Self { dir: root.join(kernel::brand::PROJECT_DIR).join(DIR) }
    }

    /// Kept in `dir` (the tests').
    pub fn in_dir(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    /// Source `source`'s file.
    pub fn file(&self, source: DataSourceId) -> PathBuf {
        self.dir.join(format!("{source}.toml"))
    }
}

fn text(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn count(table: &toml::Table, key: &str) -> Option<u64> {
    table.get(key).and_then(Value::as_integer).and_then(|n| u64::try_from(n).ok())
}

fn entry_of(table: &toml::Table, source: DataSourceId) -> Option<HistoryEntry> {
    Some(HistoryEntry {
        id: text(table, "id").and_then(|t| HistoryId::parse(&t))?,
        source,
        console: text(table, "console").and_then(|t| ConsoleId::parse(&t)),
        database: text(table, "database"),
        schema: text(table, "schema"),
        text: text(table, "text").filter(|t| !t.trim().is_empty())?,
        params: table
            .get("params")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        at_ms: table.get("at_ms").and_then(Value::as_integer).unwrap_or(0),
        duration_ms: count(table, "duration_ms"),
        rows: count(table, "rows"),
        outcome: text(table, "outcome").and_then(|o| Outcome::from_id(&o)).unwrap_or_default(),
    })
}

fn table_of(entry: &HistoryEntry) -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("id".into(), Value::String(entry.id.to_string()));
    if let Some(console) = entry.console {
        t.insert("console".into(), Value::String(console.to_string()));
    }
    if let Some(database) = &entry.database {
        t.insert("database".into(), Value::String(database.clone()));
    }
    if let Some(schema) = &entry.schema {
        t.insert("schema".into(), Value::String(schema.clone()));
    }
    t.insert("at_ms".into(), Value::Integer(entry.at_ms));
    for (key, value) in [("duration_ms", entry.duration_ms), ("rows", entry.rows)] {
        if let Some(n) = value.and_then(|n| i64::try_from(n).ok()) {
            t.insert(key.into(), Value::Integer(n));
        }
    }
    t.insert("outcome".into(), Value::String(entry.outcome.id().into()));
    if !entry.params.is_empty() {
        t.insert(
            "params".into(),
            Value::Array(entry.params.iter().cloned().map(Value::String).collect()),
        );
    }
    t.insert("text".into(), Value::String(entry.text.clone()));
    t
}

/// `entries` as the file writes them: a `[[entry]]` block each.
fn blocks(entries: &[HistoryEntry]) -> String {
    let mut root = toml::Table::new();
    root.insert(
        "entry".into(),
        Value::Array(entries.iter().map(|e| Value::Table(table_of(e))).collect()),
    );
    root.to_string()
}

/// The entries of a whole file (`None`: it isn't valid TOML, or isn't a history).
fn parse(text: &str, source: DataSourceId) -> Option<Vec<HistoryEntry>> {
    let table: toml::Table = text.parse().ok()?;
    let Some(list) = table.get("entry") else { return Some(Vec::new()) };
    let list = list.as_array()?;
    // An entry without what it must have (its id, its text) is left out, not the file.
    Some(list.iter().filter_map(|e| entry_of(e.as_table()?, source)).collect())
}

/// What can still be read of a broken file: each `[[entry]]` block on its own.
fn salvage(text: &str, source: DataSourceId) -> Vec<HistoryEntry> {
    let mut out = Vec::new();
    let mut block = String::new();
    let mut flush = |block: &mut String| {
        if let Some(entries) = parse(block, source) {
            out.extend(entries);
        }
        block.clear();
    };
    for line in text.split_inclusive('\n') {
        if line.trim() == "[[entry]]" && !block.is_empty() {
            flush(&mut block);
        }
        block.push_str(line);
    }
    flush(&mut block);
    out
}

/// A free name beside `file` for keeping it aside: `<file>.broken`, else `.broken-2`…
fn broken_name(file: &Path) -> PathBuf {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    (1..)
        .map(|n| {
            let suffix = if n == 1 { ".broken".to_owned() } else { format!(".broken-{n}") };
            file.with_file_name(format!("{name}{suffix}"))
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| file.with_file_name(format!("{name}.broken")))
}

impl HistoryStore for TomlQueryHistory {
    fn load(&self, source: DataSourceId) -> Result<Loaded, String> {
        let file = self.file(source);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Loaded::default()),
            Err(e) => return Err(e.to_string()),
        };
        if let Some(entries) = parse(&text, source) {
            return Ok(Loaded { entries, broken: None });
        }
        // Kept as it is, beside; what can be read of it written again.
        let entries = salvage(&text, source);
        let aside = broken_name(&file);
        std::fs::rename(&file, &aside).map_err(|e| e.to_string())?;
        if !entries.is_empty() {
            write_atomic(&file, &blocks(&entries)).map_err(|e| e.to_string())?;
        }
        Ok(Loaded { entries, broken: Some(aside) })
    }

    fn append(&self, source: DataSourceId, entry: &HistoryEntry) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let file = self.file(source);
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&file)
            .map_err(|e| e.to_string())?;
        // After what's there, a blank line (and the last line ended first, if it isn't).
        let len = out.metadata().map_err(|e| e.to_string())?.len();
        let lead = if len == 0 {
            ""
        } else {
            use std::io::{Read as _, Seek as _, SeekFrom};
            let mut last = [0u8; 1];
            out.seek(SeekFrom::End(-1)).map_err(|e| e.to_string())?;
            out.read_exact(&mut last).map_err(|e| e.to_string())?;
            if last[0] == b'\n' { "\n" } else { "\n\n" }
        };
        let block = blocks(std::slice::from_ref(entry));
        out.write_all(format!("{lead}{block}").as_bytes()).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())
    }

    fn rewrite(&self, source: DataSourceId, entries: &[HistoryEntry]) -> Result<(), String> {
        let file = self.file(source);
        if entries.is_empty() {
            return match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        write_atomic(&file, &blocks(entries)).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[path = "../tests/unit/query_history.rs"]
mod tests;
