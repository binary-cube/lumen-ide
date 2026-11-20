//! A project's favorite sessions: shown first in the panel, found faster; kept in the
//! project's data folder (outside the project: `agent-sessions/favorites.json`), read and
//! written on a worker only. Each is known by its agent's and its own ids (its title kept to
//! name it once it's gone). One whose session can't be found any more (deleted) is marked
//! missing: shown so, the user told once and offered to take it off the list.
//!
//! Beside them, each session's marks ([`Mark`]: pinned, done, unread, a color label, its
//! folder), kept the same way in `agent-sessions/marks.json`, and the project's folders of
//! sessions (`crate::folders`) in `agent-sessions/folders.json`: one state, one writer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};

/// A favorite session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub agent: String,
    pub id: String,
    pub title: String,
}

/// A session's color label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    Red,
    Blue,
    Green,
    Yellow,
    Purple,
    Orange,
    Pink,
    Cyan,
}

impl Label {
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Blue,
        Self::Green,
        Self::Yellow,
        Self::Purple,
        Self::Orange,
        Self::Pink,
        Self::Cyan,
    ];

    /// Its key (saved, and its name's in the locales: `agents.color_<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Purple => "purple",
            Self::Orange => "orange",
            Self::Pink => "pink",
            Self::Cyan => "cyan",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }

    /// Its color (`0xRRGGBB`): soft, read the same on light and dark themes.
    pub fn rgb(self) -> u32 {
        match self {
            Self::Red => 0xE5_6B6F,
            Self::Blue => 0x5B_8DEF,
            Self::Green => 0x4C_B782,
            Self::Yellow => 0xE0_B83E,
            Self::Purple => 0x9D_7BE0,
            Self::Orange => 0xEC_8D4E,
            Self::Pink => 0xDE_6FA9,
            Self::Cyan => 0x3F_B6C4,
        }
    }
}

/// A session's marks: pinned (on top of the list), done with, not read since, its color
/// label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mark {
    pub agent: String,
    pub id: String,
    pub pinned: bool,
    pub done: bool,
    pub unread: bool,
    pub label: Option<Label>,
    /// The folder it's in (its id: [`crate::folders::Folder::id`]).
    pub folder: Option<String>,
}

impl Mark {
    /// Nothing marked (not kept).
    pub(crate) fn is_plain(&self) -> bool {
        !self.pinned && !self.done && !self.unread && self.label.is_none() && self.folder.is_none()
    }
}

/// A project's favorites as known now.
#[derive(Debug, Default)]
pub(crate) struct Project {
    /// Where they're kept (its data folder's file), the marks, the folders.
    file: Option<PathBuf>,
    marks_file: Option<PathBuf>,
    folders_file: Option<PathBuf>,
    list: Vec<Favorite>,
    pub(crate) marks: Vec<Mark>,
    pub(crate) folders: Vec<crate::folders::Folder>,
    loaded: bool,
    /// Those whose sessions weren't found at the last read (agent, id).
    missing: Vec<(String, String)>,
    /// Those the user was told are missing (told once).
    told: Vec<(String, String)>,
    /// Counts the changes (a panel shows them again).
    pub(crate) generation: u64,
    /// A worker is writing the list; `dirty`: it changed since that worker took it (written
    /// again by the same worker, so saves never land out of order).
    saving: bool,
    dirty: bool,
}

static PROJECTS: Mutex<Option<HashMap<kernel::ProjectId, Project>>> = Mutex::new(None);

pub(crate) fn with<R>(project: kernel::ProjectId, f: impl FnOnce(&mut Project) -> R) -> R {
    let mut all = PROJECTS.lock().unwrap_or_else(PoisonError::into_inner);
    f(all.get_or_insert_with(HashMap::new).entry(project).or_default())
}

/// The most favorites kept, and the longest title kept of one (the file is read back as
/// hostile input).
const MOST: usize = 200;
/// The most sessions with marks kept.
const MARKS_MOST: usize = 2_000;
const TITLE_MOST: usize = 200;

/// Where project `project`'s favorites are kept: its data folder (told when it opens).
pub(crate) fn set_storage(project: kernel::ProjectId, storage: &Path) {
    let dir = storage.join("agent-sessions");
    with(project, |p| {
        p.file = Some(dir.join("favorites.json"));
        p.marks_file = Some(dir.join("marks.json"));
        p.folders_file = Some(dir.join("folders.json"));
    });
}

/// The project's favorites as a file's text says them (bounded; what isn't one, skipped).
pub fn parse(text: &str) -> Vec<Favorite> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    items
        .iter()
        .filter_map(|v| {
            let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
            let title: String =
                field("title").unwrap_or_default().chars().take(TITLE_MOST).collect();
            Some(Favorite { agent: field("agent")?, id: field("id")?, title })
        })
        .take(MOST)
        .collect()
}

/// `list` as the file's text.
pub fn written(list: &[Favorite]) -> String {
    let items: Vec<Value> =
        list.iter().map(|f| json!({"agent": f.agent, "id": f.id, "title": f.title})).collect();
    serde_json::to_string_pretty(&Value::Array(items)).unwrap_or_else(|_| "[]".into())
}

/// The marks as a file's text says them (bounded; what isn't one, skipped).
pub fn parse_marks(text: &str) -> Vec<Mark> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    items
        .iter()
        .filter_map(|v| {
            let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
            let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
            let mark = Mark {
                agent: field("agent")?,
                id: field("id")?,
                pinned: flag("pinned"),
                done: flag("done"),
                unread: flag("unread"),
                label: field("label").as_deref().and_then(Label::from_key),
                folder: field("folder").filter(|f| !f.is_empty() && f.len() <= 64),
            };
            (!mark.is_plain()).then_some(mark)
        })
        .take(MARKS_MOST)
        .collect()
}

/// The marks as the file's text.
pub fn written_marks(marks: &[Mark]) -> String {
    let items: Vec<Value> = marks
        .iter()
        .map(|m| {
            let mut item = json!({"agent": m.agent, "id": m.id});
            if m.pinned {
                item["pinned"] = json!(true);
            }
            if m.done {
                item["done"] = json!(true);
            }
            if m.unread {
                item["unread"] = json!(true);
            }
            if let Some(label) = m.label {
                item["label"] = json!(label.key());
            }
            if let Some(folder) = &m.folder {
                item["folder"] = json!(folder);
            }
            item
        })
        .collect();
    serde_json::to_string_pretty(&Value::Array(items)).unwrap_or_else(|_| "[]".into())
}

/// On a worker: the project's favorites and marks read from their files the first time.
pub(crate) fn load(project: kernel::ProjectId) {
    let Some((file, marks_file, folders_file)) = with(project, |p| {
        (!p.loaded).then(|| Some((p.file.clone()?, p.marks_file.clone()?, p.folders_file.clone()?)))
    })
    .flatten() else {
        return;
    };
    let read = |file: &Path| std::fs::read_to_string(file).unwrap_or_default();
    let list = parse(&read(&file));
    let marks = parse_marks(&read(&marks_file));
    let folders = crate::folders::parse(&read(&folders_file));
    with(project, |p| {
        if !p.loaded {
            p.list = list;
            p.marks = marks;
            p.folders = folders;
            p.loaded = true;
            p.generation += 1;
        }
    });
}

/// The favorites' generation now (cheap: a panel compares it before taking the list).
pub(crate) fn generation(project: kernel::ProjectId) -> u64 {
    with(project, |p| p.generation)
}

/// The project's favorites and their generation.
pub(crate) fn list(project: kernel::ProjectId) -> (u64, Vec<Favorite>, Vec<(String, String)>) {
    with(project, |p| (p.generation, p.list.clone(), p.missing.clone()))
}

/// A favorite's title kept anew (its session renamed): the name it's known by once gone.
pub(crate) fn retitle(project: kernel::ProjectId, agent: &str, id: &str, title: &str) {
    let changed = with(project, |p| {
        let Some(f) = p.list.iter_mut().find(|f| f.agent == agent && f.id == id) else {
            return false;
        };
        f.title = title.chars().take(TITLE_MOST).collect();
        p.generation += 1;
        true
    });
    if changed {
        save(project);
    }
}

/// The project's sessions' marks.
pub(crate) fn marks(project: kernel::ProjectId) -> Vec<Mark> {
    with(project, |p| p.marks.clone())
}

/// Session `id` of `agent` marked anew (`change` given its marks as they stand): kept, or
/// let go when nothing's marked any more.
pub(crate) fn mark(
    project: kernel::ProjectId,
    agent: &str,
    id: &str,
    change: impl FnOnce(&mut Mark),
) {
    let changed = with(project, |p| {
        let at = p.marks.iter().position(|m| m.agent == agent && m.id == id);
        let mut mark = at.map_or_else(
            || Mark { agent: agent.to_owned(), id: id.to_owned(), ..Mark::default() },
            |i| p.marks[i].clone(),
        );
        let before = mark.clone();
        change(&mut mark);
        if mark == before {
            return false;
        }
        match (at, mark.is_plain()) {
            (Some(i), true) => {
                p.marks.remove(i);
            }
            (Some(i), false) => p.marks[i] = mark,
            (None, false) if p.marks.len() < MARKS_MOST => p.marks.push(mark),
            (None, _) => return false,
        }
        p.generation += 1;
        true
    });
    if changed {
        save(project);
    }
}

/// The project's favorites let go (it closed): read again if it opens again.
pub(crate) fn release(project: kernel::ProjectId) {
    let mut all = PROJECTS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(all) = all.as_mut()
        && all.get(&project).is_some_and(|p| !p.saving)
    {
        all.remove(&project);
    }
}

/// Written on a worker: the latest list, one write at a time, whole (a temporary file renamed
/// over it, never a torn one).
pub(crate) fn save(project: kernel::ProjectId) {
    let start = with(project, |p| {
        p.dirty = true;
        p.file.is_some() && !std::mem::replace(&mut p.saving, true)
    });
    if !start {
        return;
    }
    kernel::work::run("agent-sessions", move || {
        loop {
            let Some((file, text, marks, folders)) = with(project, |p| {
                let taken = p
                    .dirty
                    .then(|| {
                        let marks = (p.marks_file.clone()?, written_marks(&p.marks));
                        let folders =
                            (p.folders_file.clone()?, crate::folders::written(&p.folders));
                        Some((p.file.clone()?, written(&p.list), marks, folders))
                    })
                    .flatten();
                p.dirty = false;
                if taken.is_none() {
                    p.saving = false;
                }
                taken
            }) else {
                return;
            };
            if let Err(e) = write_whole(&file, &text) {
                tracing::warn!(error = %e, "the favorite sessions weren't saved");
            }
            if let Err(e) = write_whole(&marks.0, &marks.1) {
                tracing::warn!(error = %e, "the sessions' marks weren't saved");
            }
            if let Err(e) = write_whole(&folders.0, &folders.1) {
                tracing::warn!(error = %e, "the sessions' folders weren't saved");
            }
        }
    });
}

/// `text` written to `file` through a temporary file beside it, renamed over it.
fn write_whole(file: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // One write at a time per project: one temporary name is enough.
    let temp = file.with_extension("json.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// A session added to the favorites, or taken off them (`true`: it's one now).
pub(crate) fn toggle(project: kernel::ProjectId, agent: &str, id: &str, title: &str) -> bool {
    let now = with(project, |p| {
        let at = p.list.iter().position(|f| f.agent == agent && f.id == id);
        let now = match at {
            Some(i) => {
                p.list.remove(i);
                false
            }
            None if p.list.len() < MOST => {
                let title = title.chars().take(TITLE_MOST).collect();
                p.list.push(Favorite { agent: agent.to_owned(), id: id.to_owned(), title });
                true
            }
            None => false,
        };
        p.missing.retain(|(a, i)| !(a == agent && i == id));
        p.generation += 1;
        now
    });
    save(project);
    now
}

/// What a read found (`found`: every session listed, by agent and id): the favorites not
/// among them marked missing; those not told yet returned, to be told (marked so by
/// [`told`] once they are: a read whose panel was let go tells them next time).
pub(crate) fn seen(project: kernel::ProjectId, found: &[(&str, &str)]) -> Vec<Favorite> {
    with(project, |p| {
        let missing: Vec<(String, String)> = p
            .list
            .iter()
            .filter(|f| !found.contains(&(f.agent.as_str(), f.id.as_str())))
            .map(|f| (f.agent.clone(), f.id.clone()))
            .collect();
        if missing != p.missing {
            p.missing = missing;
            p.generation += 1;
        }
        let new: Vec<Favorite> = p
            .list
            .iter()
            .filter(|f| {
                let key = (f.agent.clone(), f.id.clone());
                p.missing.contains(&key) && !p.told.contains(&key)
            })
            .cloned()
            .collect();
        new
    })
}

/// The user was told these favorites are missing (not told again).
pub(crate) fn told(project: kernel::ProjectId, favorites: &[Favorite]) {
    with(project, |p| {
        for f in favorites {
            let key = (f.agent.clone(), f.id.clone());
            if !p.told.contains(&key) {
                p.told.push(key);
            }
        }
    });
}

/// The missing favorites taken off the list.
pub(crate) fn forget_missing(project: kernel::ProjectId) {
    with(project, |p| {
        let missing = std::mem::take(&mut p.missing);
        p.list.retain(|f| !missing.contains(&(f.agent.clone(), f.id.clone())));
        p.generation += 1;
    });
    save(project);
}

#[cfg(test)]
#[path = "../tests/unit/favorites.rs"]
mod tests;
