//! A project's folders of sessions: the user's own grouping of the list, as folders in a
//! tree — made, named, opened and closed, deleted (their sessions back in the list), a
//! session moved into one or out. Every agent's sessions alike: a session is known by its
//! agent's and its own ids (its [`Mark::folder`](crate::favorites::Mark)), never by
//! anything an agent keeps, so an agent added later needs nothing here.
//!
//! Kept with the favorites and the marks (one state per project, one writer:
//! `agent-sessions/folders.json`), read and written on a worker only; the file is read back
//! as hostile input.

use serde_json::{Value, json};

use crate::favorites::{save, with};

/// A folder of sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Its identity (a UUID v4: names may repeat, may change).
    pub id: String,
    pub name: String,
    /// Shown open (its sessions under it) or closed.
    pub open: bool,
}

/// The most folders kept, the longest name, the longest id read back.
const MOST: usize = 200;
const NAME_MOST: usize = 100;
const ID_MOST: usize = 64;

/// A folder's name as kept: one line, no control characters, trimmed, bounded.
fn tidy(name: &str) -> String {
    let line: String = name.chars().filter(|c| !c.is_control()).take(NAME_MOST).collect();
    line.trim().to_owned()
}

/// The folders as a file's text says them (bounded; what isn't one, skipped; an id seen
/// twice, once).
pub fn parse(text: &str) -> Vec<Folder> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let mut out: Vec<Folder> = Vec::new();
    for v in &items {
        let field = |k: &str| v.get(k).and_then(Value::as_str);
        let (Some(id), Some(name)) = (field("id"), field("name")) else { continue };
        let name = tidy(name);
        if id.is_empty() || id.len() > ID_MOST || name.is_empty() || out.iter().any(|f| f.id == id)
        {
            continue;
        }
        let open = v.get("open").and_then(Value::as_bool).unwrap_or(true);
        out.push(Folder { id: id.to_owned(), name, open });
        if out.len() >= MOST {
            break;
        }
    }
    out
}

/// The folders as the file's text.
pub fn written(folders: &[Folder]) -> String {
    let items: Vec<Value> =
        folders.iter().map(|f| json!({"id": f.id, "name": f.name, "open": f.open})).collect();
    serde_json::to_string_pretty(&Value::Array(items)).unwrap_or_else(|_| "[]".into())
}

/// The project's folders, in the order they were made.
pub(crate) fn list(project: kernel::ProjectId) -> Vec<Folder> {
    with(project, |p| p.folders.clone())
}

/// A folder made, named `name` (empty: "New Folder"): its id; `None` with too many.
pub(crate) fn create(project: kernel::ProjectId, name: &str) -> Option<String> {
    let name = tidy(name);
    let name = if name.is_empty() { i18n::t("agents.new_folder_name") } else { name };
    let id = with(project, |p| {
        if p.folders.len() >= MOST {
            return None;
        }
        let id = format!("{:032x}", kernel::id::new_uuid());
        p.folders.push(Folder { id: id.clone(), name, open: true });
        p.generation += 1;
        Some(id)
    })?;
    save(project);
    Some(id)
}

/// Folder `id` named `name` (an empty name changes nothing).
pub(crate) fn rename(project: kernel::ProjectId, id: &str, name: &str) {
    let name = tidy(name);
    if name.is_empty() {
        return;
    }
    let changed = with(project, |p| {
        let Some(f) = p.folders.iter_mut().find(|f| f.id == id && f.name != name) else {
            return false;
        };
        f.name = name;
        p.generation += 1;
        true
    });
    if changed {
        save(project);
    }
}

/// Folder `id` shown open or closed, the other way round.
pub(crate) fn toggle_open(project: kernel::ProjectId, id: &str) {
    let changed = with(project, |p| {
        let Some(f) = p.folders.iter_mut().find(|f| f.id == id) else { return false };
        f.open = !f.open;
        p.generation += 1;
        true
    });
    if changed {
        save(project);
    }
}

/// Folder `id` deleted: its sessions back in the list (nothing of them lost).
pub(crate) fn remove(project: kernel::ProjectId, id: &str) {
    let changed = with(project, |p| {
        let before = p.folders.len();
        p.folders.retain(|f| f.id != id);
        if p.folders.len() == before {
            return false;
        }
        for mark in &mut p.marks {
            if mark.folder.as_deref() == Some(id) {
                mark.folder = None;
            }
        }
        p.marks.retain(|m| !m.is_plain());
        p.generation += 1;
        true
    });
    if changed {
        save(project);
    }
}

/// Session `id` of `agent` moved into folder `folder` (`None`: out of any); a folder that
/// isn't there, nothing.
pub(crate) fn move_to(project: kernel::ProjectId, agent: &str, id: &str, folder: Option<&str>) {
    if let Some(folder) = folder
        && !with(project, |p| p.folders.iter().any(|f| f.id == folder))
    {
        return;
    }
    crate::favorites::mark(project, agent, id, |m| m.folder = folder.map(str::to_owned));
}

#[cfg(test)]
#[path = "../tests/unit/folders.rs"]
mod tests;
