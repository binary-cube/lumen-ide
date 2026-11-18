//! `sessions.toml`: the app's sessions ([`workspace::sessions`]) as TOML, read leniently (a
//! damaged entry is left out, the rest kept) and written atomically.
//!
//! ```toml
//! [[sessions]]
//! id = "…32 hex…"
//! started = 1794160846
//! updated = 1794161000
//! clean = false
//! [[sessions.windows]]
//! project = "/work/shop"
//! files = ["/work/shop/src/Cart.php"]
//! ```

use std::io;
use std::path::{Path, PathBuf};

use toml::{Table, Value};
use workspace::sessions::{Session, SessionWindow, Sessions};

use crate::write_atomic;

/// The sessions in `path` (none: no file, or nothing readable in it).
pub fn load(path: &Path) -> Sessions {
    let Ok(text) = std::fs::read_to_string(path) else { return Sessions::default() };
    let Ok(table) = text.parse::<Table>() else { return Sessions::default() };
    let list = table
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
        .filter_map(session)
        .collect();
    Sessions { list }
}

fn session(t: &Table) -> Option<Session> {
    let id = u128::from_str_radix(t.get("id")?.as_str()?, 16).ok()?;
    let number = |key: &str| t.get(key).and_then(Value::as_integer).map_or(0, |n| n.max(0) as u64);
    let windows = t
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
        .map(|w| SessionWindow {
            project: w.get("project").and_then(Value::as_str).map(PathBuf::from),
            files: w
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect(),
        })
        .collect();
    Some(Session {
        id,
        started: number("started"),
        updated: number("updated"),
        clean: t.get("clean").and_then(Value::as_bool).unwrap_or(false),
        windows,
    })
}

/// `sessions` written to `path`.
pub fn save(path: &Path, sessions: &Sessions) -> io::Result<()> {
    let list: Vec<Value> = sessions
        .list
        .iter()
        .map(|s| {
            let mut t = Table::new();
            t.insert("id".into(), Value::String(format!("{:032x}", s.id)));
            let int = |n: u64| Value::Integer(i64::try_from(n).unwrap_or(i64::MAX));
            t.insert("started".into(), int(s.started));
            t.insert("updated".into(), int(s.updated));
            t.insert("clean".into(), Value::Boolean(s.clean));
            let windows = s
                .windows
                .iter()
                .map(|w| {
                    let mut wt = Table::new();
                    if let Some(project) = &w.project {
                        wt.insert("project".into(), Value::String(project.display().to_string()));
                    }
                    let files =
                        w.files.iter().map(|f| Value::String(f.display().to_string())).collect();
                    wt.insert("files".into(), Value::Array(files));
                    Value::Table(wt)
                })
                .collect();
            t.insert("windows".into(), Value::Array(windows));
            Value::Table(t)
        })
        .collect();
    let mut root = Table::new();
    root.insert("sessions".into(), Value::Array(list));
    let text = toml::to_string(&root).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_atomic(path, &text)
}

#[cfg(test)]
#[path = "../tests/unit/app_sessions.rs"]
mod tests;
