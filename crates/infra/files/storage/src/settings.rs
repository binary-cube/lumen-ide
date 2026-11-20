//! The TOML adapter for [`SettingsStore`]: the user's `settings.toml`, and a project's own
//! `project.toml` (a module's table there, the others' kept as they are).
//!
//! The user's settings ([`TomlSettingsStore::written_behind`]: the
//! only writer of their file) are kept as last read (read again only when the file's size
//! or date moved: edited by hand) and written behind, on a thread of its own: a choice never
//! waits for the disk (an atomic write syncs it: ~5 ms on a fast SSD, more on Windows), a
//! slider dragged across its stops writes only where it stopped; [`flush`] waits for what's
//! still to write (the app ends). A project's file, which others write too (its levels, its
//! frameworks), is written at once, as it was.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::time::SystemTime;

use settings_model::{Section, SettingValue, SettingsStore};

use crate::write_atomic;

/// Settings in one TOML file (the user's, or a project's).
#[derive(Debug, Clone)]
pub struct TomlSettingsStore {
    path: Option<PathBuf>,
    file: Arc<File>,
    /// Written behind (the file's only writer), else at once.
    behind: bool,
}

/// A file's size and date: the same, the file as it was.
type Stamp = Option<(u64, SystemTime)>;

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// The file as last read or written, and what's still to write.
#[derive(Debug, Default)]
struct File {
    state: Mutex<FileState>,
    /// Told when what was to write is written.
    written: Condvar,
}

#[derive(Debug, Default)]
struct FileState {
    /// The table as read or changed (`None`: missing), and the stamp it was read at.
    known: Option<(Option<toml::Table>, Stamp)>,
    /// The text to write next (the newest: older ones never written).
    pending: Option<String>,
    /// The writer is writing.
    writing: bool,
}

/// Every store's file, to be flushed at the end.
static FILES: Mutex<Vec<Weak<File>>> = Mutex::new(Vec::new());

/// Waits for every store's writes still to come (the app ends: none is lost).
pub fn flush() {
    let files: Vec<Arc<File>> =
        FILES.lock().map(|f| f.iter().filter_map(Weak::upgrade).collect()).unwrap_or_default();
    for file in files {
        let mut state = file.state.lock().unwrap_or_else(PoisonError::into_inner);
        while state.pending.is_some() || state.writing {
            state = file.written.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl TomlSettingsStore {
    /// `path`: the file (`None`: no config folder; reads find nothing, writes do nothing),
    /// each change written at once.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path, file: Arc::default(), behind: false }
    }

    /// [`Self::new`], its changes written behind: for a file nothing else writes (the user's
    /// settings).
    pub fn written_behind(path: Option<PathBuf>) -> Self {
        let file = Arc::new(File::default());
        if let Ok(mut files) = FILES.lock() {
            files.retain(|f| f.strong_count() > 0);
            files.push(Arc::downgrade(&file));
        }
        Self { path, file, behind: true }
    }

    /// The whole file (`Ok(None)`: missing; `Err`: present but invalid): as known while
    /// writes are pending or the file didn't move, else read again.
    fn read(&self) -> Result<Option<toml::Table>, String> {
        let Some(path) = &self.path else { return Ok(None) };
        // Written at once (others write it too): read as it is, each time.
        if !self.behind {
            return read_file(path);
        }
        let mut state = self.file.state.lock().unwrap_or_else(PoisonError::into_inner);
        let pending = state.pending.is_some() || state.writing;
        let now = if pending { None } else { Some(stamp(path)) };
        if let Some((table, at)) = &state.known
            && (pending || now.as_ref() == Some(at))
        {
            return Ok(table.clone());
        }
        let now = now.unwrap_or_else(|| stamp(path));
        let table = read_file(path)?;
        state.known = Some((table.clone(), now));
        Ok(table)
    }

    /// `text` written behind (the newest wins), `table` known meanwhile.
    fn write_behind(&self, path: &Path, table: toml::Table, text: String) {
        let mut state = self.file.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.known = Some((Some(table), None));
        let start = state.pending.is_none() && !state.writing;
        state.pending = Some(text);
        drop(state);
        if !start {
            return;
        }
        let (file, target) = (Arc::clone(&self.file), path.to_path_buf());
        let spawned = std::thread::Builder::new().name("settings-write".into()).spawn(move || {
            loop {
                let mut state = file.state.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(text) = state.pending.take() else {
                    state.writing = false;
                    file.written.notify_all();
                    return;
                };
                state.writing = true;
                drop(state);
                let written = write_atomic(&target, &text);
                let mut state = file.state.lock().unwrap_or_else(PoisonError::into_inner);
                match written {
                    // What's known is what was just written, unless newer is pending.
                    Ok(()) if state.pending.is_none() => {
                        if let Some((_, at)) = &mut state.known {
                            *at = stamp(&target);
                        }
                    }
                    Ok(()) => {}
                    Err(e) => {
                        tracing::warn!(target = %target.display(), error = %e, "can't write");
                        // Read again next time: what's on disk is what holds.
                        state.known = None;
                    }
                }
            }
        });
        if spawned.is_err() {
            // No thread: written here and now.
            let mut state = self.file.state.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(text) = state.pending.take() {
                if let Err(e) = write_atomic(path, &text) {
                    tracing::warn!(path = %path.display(), error = %e, "can't write");
                }
                if let Some((_, at)) = &mut state.known {
                    *at = stamp(path);
                }
            }
        }
    }
}

/// `path` read and parsed (`Ok(None)`: missing or empty; `Err`: present but invalid).
fn read_file(path: &Path) -> Result<Option<toml::Table>, String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(None) };
    if text.trim().is_empty() {
        return Ok(None);
    }
    text.parse::<toml::Table>().map(Some).map_err(|e| e.to_string())
}

impl SettingsStore for TomlSettingsStore {
    fn section(&self, name: &str) -> Option<Section> {
        let table = match self.read() {
            Ok(table) => table?,
            Err(e) => {
                let path = self.path.as_deref().unwrap_or(std::path::Path::new("settings"));
                tracing::warn!(path = %path.display(), error = %e, "the settings are invalid: the defaults used");
                return None;
            }
        };
        table.get(name)?.as_table().map(to_section)
    }

    /// The file read and parsed once for all of them; an invalid one is `None` (kept as it
    /// was read before).
    fn sections(&self, names: &[String]) -> Option<Vec<Option<Section>>> {
        let table = self.read().ok()?;
        Some(
            names
                .iter()
                .map(|n| table.as_ref()?.get(n.as_str())?.as_table().map(to_section))
                .collect(),
        )
    }

    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let mut table = self
            .read()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
            .unwrap_or_default();
        apply(&mut table, name, change);
        let text =
            toml::to_string(&table).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if self.behind {
            self.write_behind(path, table, text);
            return Ok(());
        }
        write_atomic(path, &text)
    }
}

/// Applies `change` to section `name` of `table`. Only what changed is written back:
/// keys of other types (e.g. floats, nested tables) stay as they were.
fn apply(table: &mut toml::Table, name: &str, change: &mut dyn FnMut(&mut Section)) {
    let entry = table.entry(name).or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::Table::new());
    }
    let Some(raw) = entry.as_table_mut() else { return };
    let before = to_section(raw);
    let mut after = before.clone();
    change(&mut after);
    for key in before.keys().filter(|k| !after.contains_key(*k)) {
        raw.remove(key);
    }
    for (key, value) in after {
        if before.get(&key) != Some(&value) {
            raw.insert(key, to_toml(value));
        }
    }
}

fn to_section(table: &toml::Table) -> Section {
    table
        .iter()
        .filter_map(|(k, v)| {
            let value = match v {
                toml::Value::Boolean(b) => SettingValue::Bool(*b),
                toml::Value::Integer(i) => SettingValue::Int(*i),
                toml::Value::String(s) => SettingValue::Text(s.clone()),
                toml::Value::Array(a) => SettingValue::List(
                    a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
                ),
                _ => return None,
            };
            Some((k.clone(), value))
        })
        .collect()
}

fn to_toml(value: SettingValue) -> toml::Value {
    match value {
        SettingValue::Bool(b) => toml::Value::Boolean(b),
        SettingValue::Int(i) => toml::Value::Integer(i),
        SettingValue::Text(s) => toml::Value::String(s),
        SettingValue::List(l) => {
            toml::Value::Array(l.into_iter().map(toml::Value::String).collect())
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
