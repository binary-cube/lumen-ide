//! Claude Code on disk (always on a worker): its program, found where it's installed, and
//! its sessions of a project — listed with limits (files only, no links followed, a bounded
//! count), each read from its first and last bytes ([`crate::format`]) and kept by its
//! modification time and size, so a list asked again reads only what changed.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use agent_files::{Kept, Stamp};
use agents::AgentSession;

use crate::format::{self, Facts};

/// Session `id` working in `root` named `title`, as Claude Code's `/rename` does: its record
/// added at the end of the session's file (only an existing one, a file itself — never a
/// link —, and only an id that can name a file).
pub fn rename(projects: &Path, root: &Path, id: &str, title: &str) -> Result<(), String> {
    use std::io::Write;
    if !format::valid_id(id) {
        return Err("not a session's id".into());
    }
    let folder = projects.join(format::folder_name(&root.to_string_lossy()));
    let file = folder.join(format!("{id}.jsonl"));
    let meta = std::fs::symlink_metadata(&file).map_err(|_| "the session's file isn't there")?;
    if !meta.is_file() {
        return Err("the session's file isn't a file".into());
    }
    let mut record = format::title_record(id, title);
    // Its last line ended (else the record starts a line of its own).
    let ended = {
        let mut f = File::open(&file).map_err(|e| e.to_string())?;
        let len = meta.len();
        len == 0 || {
            let mut last = [0u8; 1];
            f.seek(SeekFrom::Start(len - 1)).and_then(|_| f.read_exact(&mut last)).is_ok()
                && last[0] == b'\n'
        }
    };
    if !ended {
        record.insert(0, '\n');
    }
    let mut out =
        std::fs::OpenOptions::new().append(true).open(&file).map_err(|e| e.to_string())?;
    out.write_all(record.as_bytes()).map_err(|e| e.to_string())
}

/// The most entries looked at in a project's folder (sorted by time after: the newest are
/// never the ones cut).
const MAX_FILES: usize = 20_000;
/// The sessions' facts kept (by file).
const KEPT: usize = 2_000;

/// Where Claude Code keeps its things: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn config_dir() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => Some(dirs::home_dir()?.join(".claude")),
    }
}

/// Claude Code's program: on `PATH`, in its own folder (`~/.claude/local`), or where
/// installers put programs.
pub fn find_program() -> Option<PathBuf> {
    find("claude")
}

/// Program `name` (Claude Code's, its adapter, npm), its own folder looked in too.
pub fn find(name: &str) -> Option<PathBuf> {
    let own: Vec<PathBuf> =
        dirs::home_dir().map(|h| h.join(".claude").join("local")).into_iter().collect();
    agent_files::find(name, &own)
}

/// A file's first bytes and, when it's longer, its last ones; `true`: the first are all
/// of it.
fn ends(path: &Path, len: u64) -> std::io::Result<(Vec<u8>, Vec<u8>, bool)> {
    let mut file = File::open(path)?;
    let mut head = Vec::with_capacity(format::HEAD.min(len as usize));
    (&mut file).take(format::HEAD as u64).read_to_end(&mut head)?;
    if len <= format::HEAD as u64 {
        return Ok((head, Vec::new(), true));
    }
    let from = len.saturating_sub(format::TAIL as u64).max(format::HEAD as u64);
    file.seek(SeekFrom::Start(from))?;
    let mut tail = Vec::with_capacity(format::TAIL);
    file.take(format::TAIL as u64).read_to_end(&mut tail)?;
    Ok((head, tail, false))
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
        self.kept.get(file, stamp, || {
            let (head, tail, whole) = ends(file, stamp.1).ok()?;
            Some(format::facts(&head, &tail, whole))
        })
    }

    /// The sessions working in `root` among `projects`' (Claude Code's `projects/`), the
    /// most recently active first, at most `limit`.
    pub fn of(&self, projects: &Path, root: &Path, limit: usize) -> Vec<AgentSession> {
        let root_text = root.to_string_lossy();
        let folder = projects.join(format::folder_name(&root_text));
        // Files only (a link isn't followed), the newest first.
        let files = agent_files::files(&folder, "jsonl", MAX_FILES);
        let mut sessions = Vec::new();
        for (file, stamp) in files {
            if sessions.len() >= limit {
                break;
            }
            let Some(facts) = self.facts(&file, stamp) else { continue };
            // Another path written the same way: not this project's.
            if facts
                .cwd
                .as_deref()
                .is_some_and(|cwd| !agent_files::same_folder(Path::new(cwd), root))
            {
                continue;
            }
            // Its id as it may be named on a command line: the file's own, else skipped.
            let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned());
            let Some(id) = facts.id.or(stem.filter(|s| format::valid_id(s))) else { continue };
            sessions.push(AgentSession {
                agent: crate::ID,
                title: facts.title.unwrap_or_else(|| id.chars().take(8).collect()),
                id,
                folder: root.to_path_buf(),
                started: facts.started,
                active: stamp.0,
                file: Some(file),
            });
        }
        sessions
    }
}

#[cfg(test)]
#[path = "../tests/unit/files.rs"]
mod tests;
