//! The programs the app started that would outlive it if it crashed: told to a recorder
//! the app sets once ([`set_recorder`]), which notes them on disk so the next start knows
//! what was left running (plan 83). Nothing is recorded without a recorder (tests, tools).
//!
//! Only programs that live long are told: one [`crate::start`]ed (a server, an agent, a
//! driver) at once, one [`crate::run`] once it has run [`LONG_LIVED`] (a git command ending
//! sooner is never told), a terminal's shell when it starts ([`shell`]). Only the program's
//! name is told, never its arguments (they may hold secrets).
//!
//! Telling never blocks: the recorder takes the news and writes on a thread of its own.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// How long a program [`crate::run`] must have run before it's recorded: shorter ones (most
/// git commands, a formatter) end before a crash could leave them behind.
pub const LONG_LIVED: Duration = Duration::from_secs(2);

/// The longest program name kept (characters).
const MAX_NAME: usize = 64;

/// What a program is to the app: what happens to it if the app ends without stopping it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A language server, a database driver: useless without the app, stopped.
    Server,
    /// A terminal's shell (and what runs in it): the user's, asked about.
    Terminal,
    /// An agent (a terminal running one, a conversation's): the user's, asked about.
    Agent,
    /// A task the user started: asked about.
    Task,
    /// A git command: stopped.
    Git,
    /// Any other helper: stopped.
    Other,
}

impl Kind {
    /// The kind of a program run by `owner` ([`crate::Run::owner`]).
    pub fn of_owner(owner: &str) -> Self {
        match owner {
            "lsp" | "driver" => Self::Server,
            "acp" => Self::Agent,
            "task" => Self::Task,
            "git" => Self::Git,
            "terminal" => Self::Terminal,
            _ => Self::Other,
        }
    }

    /// Whether it's the user's work (kept and asked about) rather than a helper (stopped).
    pub const fn is_users(self) -> bool {
        matches!(self, Self::Terminal | Self::Agent | Self::Task)
    }

    /// Its name in the registry's files.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Terminal => "terminal",
            Self::Agent => "agent",
            Self::Task => "task",
            Self::Git => "git",
            Self::Other => "other",
        }
    }

    /// The kind named `id` in a registry's file (an unknown one: a helper).
    pub fn from_id(id: &str) -> Self {
        match id {
            "server" => Self::Server,
            "terminal" => Self::Terminal,
            "agent" => Self::Agent,
            "task" => Self::Task,
            "git" => Self::Git,
            _ => Self::Other,
        }
    }
}

/// A program started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub pid: u32,
    pub kind: Kind,
    /// Its name only ([`program_name`]).
    pub program: String,
    /// The project (its folder, or a folder in it) it runs for, when known.
    pub project: Option<PathBuf>,
    /// Its end is told ([`Recorded`] dropped); `false`: the recorder finds out by itself
    /// that it ended (a terminal's shell).
    pub told_end: bool,
}

/// Who notes the programs started (the app's registry).
pub trait Recorder: Send + Sync {
    /// `launch` started. Never blocks.
    fn started(&self, launch: Launch);
    /// Program `pid` ended. Never blocks.
    fn ended(&self, pid: u32);
}

static RECORDER: OnceLock<Arc<dyn Recorder>> = OnceLock::new();

/// From now on the programs started are told to `recorder` (set once: a second call
/// changes nothing).
pub fn set_recorder(recorder: Arc<dyn Recorder>) {
    let _ = RECORDER.set(recorder);
}

/// A program told as started: its end is told when this is dropped.
#[derive(Debug)]
pub struct Recorded(u32);

impl Drop for Recorded {
    fn drop(&mut self) {
        if let Some(recorder) = RECORDER.get() {
            recorder.ended(self.0);
        }
    }
}

/// `pid` (running `program`, of `kind`, for `project`) told as started, its end told when
/// the result is dropped; `None` without a recorder.
pub fn record(pid: u32, kind: Kind, program: &Path, project: Option<&Path>) -> Option<Recorded> {
    let recorder = RECORDER.get()?;
    recorder.started(Launch {
        pid,
        kind,
        program: program_name(program),
        project: project.map(Path::to_path_buf),
        told_end: true,
    });
    Some(Recorded(pid))
}

/// A terminal's shell `pid` started (running `program`; `agent`: an agent's session in its
/// place) in `folder`: recorded until it ends (the recorder sees that by itself).
pub fn shell(pid: u32, program: &Path, folder: Option<&Path>, agent: bool) {
    let Some(recorder) = RECORDER.get() else { return };
    recorder.started(Launch {
        pid,
        kind: if agent { Kind::Agent } else { Kind::Terminal },
        program: program_name(program),
        project: folder.map(Path::to_path_buf),
        told_end: false,
    });
}

/// A program's name as shown: the last part of its path, without Windows' `.exe`, only
/// printable characters, at most [`MAX_NAME`] of them — never more than the name.
pub fn program_name(program: &Path) -> String {
    let text = program.to_string_lossy();
    // Either separator (a Windows path read on Unix too).
    let last = text.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or("");
    let last = last.strip_suffix(".exe").or_else(|| last.strip_suffix(".EXE")).unwrap_or(last);
    let name: String = last.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    // A login shell's name starts with `-` (`-zsh`).
    let name = name.trim().trim_start_matches('-');
    if name.is_empty() { "?".to_owned() } else { name.to_owned() }
}

#[cfg(test)]
#[path = "../tests/unit/launched.rs"]
mod tests;
