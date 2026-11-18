//! Cline's sessions on disk (always on a worker): one folder per session under its
//! `sessions/` (`<id>/<id>.json`: when it started and ended, the folder it worked in, its
//! title or its first request), read as hostile input — folders themselves, no link
//! followed, a bounded count and size — and kept by each file's stamp, so a list asked again
//! reads only what changed. The messages beside it are never read.

use std::path::{Path, PathBuf};

use agent_files::{Kept, Stamp};
use agents::AgentSession;
use serde_json::Value;

/// The most session folders looked at; the facts kept (by file); the biggest facts file
/// read; a title's length.
const MOST: usize = 20_000;
const KEPT: usize = 2_000;
const FILE_MOST: u64 = 1024 * 1024;
const TITLE_MOST: usize = 100;

/// What a session's file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub id: String,
    /// The folder it worked in.
    pub cwd: Option<String>,
    pub title: Option<String>,
    /// When it started and when it ended (seconds since the epoch).
    pub started: Option<i64>,
    pub ended: Option<i64>,
}

/// A time as Cline writes it (`2026-07-31T12:58:59.557Z`), in seconds since the epoch.
fn seconds(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp())
}

/// A session's facts as its file's text says them (`None`: not one: no id that can name
/// a file and a command's argument).
pub fn facts(text: &[u8]) -> Option<Facts> {
    let v: Value = serde_json::from_slice(text).ok()?;
    let field = |k: &str| v.get(k).and_then(Value::as_str);
    let id = field("session_id").filter(|id| agent_files::valid_id(id))?.to_owned();
    let cwd = field("workspace_root").or_else(|| field("cwd")).map(str::to_owned);
    let named = v.get("metadata").and_then(|m| m.get("title")).and_then(Value::as_str);
    let title = named
        .or_else(|| field("prompt"))
        .map(|t| agent_files::title_of(t, TITLE_MOST))
        .filter(|t| !t.is_empty());
    let started = field("started_at").and_then(seconds);
    let ended = field("ended_at").and_then(seconds);
    Some(Facts { id, cwd, title, started, ended })
}

/// The sessions' facts, kept by file and stamp.
pub struct Sessions {
    kept: Kept<Facts>,
}

impl Default for Sessions {
    fn default() -> Self {
        Self { kept: Kept::new(KEPT) }
    }
}

impl std::fmt::Debug for Sessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sessions").finish_non_exhaustive()
    }
}

impl Sessions {
    /// `file`'s facts: kept ones if its stamp didn't change, else read again.
    fn facts(&self, file: &Path, stamp: Stamp) -> Option<Facts> {
        self.kept.get(file, stamp, || facts(&agent_files::read_bounded(file, FILE_MOST)?))
    }

    /// The sessions working in `root` among `dir`'s (Cline's `sessions/`), the most recently
    /// active first, at most `limit`.
    pub fn of(&self, dir: &Path, root: &Path, limit: usize) -> Vec<AgentSession> {
        let mut files: Vec<(PathBuf, Stamp)> = agent_files::folders(dir, MOST)
            .into_iter()
            .filter_map(|folder| {
                let name = folder.file_name()?.to_string_lossy().into_owned();
                let file = folder.join(format!("{name}.json"));
                Some((agent_files::stamp(&file)?, file)).map(|(s, f)| (f, s))
            })
            .collect();
        files.sort_by_key(|(_, (modified, _))| std::cmp::Reverse(*modified));
        let mut sessions = Vec::new();
        for (file, stamp) in files {
            if sessions.len() >= limit {
                break;
            }
            let Some(facts) = self.facts(&file, stamp) else { continue };
            if !facts
                .cwd
                .as_deref()
                .is_some_and(|cwd| agent_files::same_folder(Path::new(cwd), root))
            {
                continue;
            }
            sessions.push(AgentSession {
                agent: crate::ID,
                title: facts.title.unwrap_or_else(|| facts.id.chars().take(8).collect()),
                id: facts.id,
                folder: root.to_path_buf(),
                started: facts.started,
                active: facts.ended.unwrap_or(stamp.0).max(stamp.0),
                file: Some(file),
            });
        }
        sessions
    }
}

#[cfg(test)]
#[path = "../tests/unit/sessions.rs"]
mod tests;
