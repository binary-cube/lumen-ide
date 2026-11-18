//! AI agents on the user's machine (Claude Code; later others), as the editor hosts them —
//! never as an agent of its own: what one of their sessions is, how a program is started
//! for one ([`Launch`]), and the port each agent's module implements ([`AgentProvider`]).
//!
//! Pure: what a session file says is read by the agent's module (on a worker); here only
//! the shapes every module agrees on, so that the sessions' panel, the graphical face and
//! the MCP server know no agent.

use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

/// How a program is started: the program, its arguments, what's added to its environment,
/// where (`None`: the project's folder).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

impl Launch {
    /// `program` with `args`, in `cwd`.
    pub fn new(program: impl Into<String>, args: &[&str], cwd: Option<&Path>) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: Vec::new(),
            cwd: cwd.map(Path::to_path_buf),
        }
    }

    /// The command as a person would type it (shown, never run through a shell): each
    /// part with spaces quoted.
    pub fn display(&self) -> String {
        std::iter::once(&self.program)
            .chain(&self.args)
            .map(|part| {
                if part.is_empty() || part.contains(char::is_whitespace) {
                    format!("\"{part}\"")
                } else {
                    part.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// An agent found on the machine: where its program is, its version when it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInstall {
    pub program: PathBuf,
    pub version: Option<String>,
}

/// One of an agent's sessions (a conversation it keeps on disk and can take up again).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    /// The agent's id ([`AgentProvider::id`]).
    pub agent: &'static str,
    /// The session's own id (what resuming it names).
    pub id: String,
    /// What it's called: the name given to it, else its first request, shortened.
    pub title: String,
    /// The folder it works in.
    pub folder: PathBuf,
    /// When it started and when it was last active (seconds since the epoch).
    pub started: Option<i64>,
    pub active: i64,
    /// The file it's kept in (shown, revealed; never written by the editor).
    pub file: Option<PathBuf>,
}

/// An agent on the machine, offered by its own module: the sessions' panel, the graphical
/// face and the MCP server reach it only through this. Every method that reads the disk or
/// runs a program is called on a worker, never on the UI thread.
pub trait AgentProvider: Send + Sync + Debug {
    /// Its id, saved in the settings and in [`AgentSession::agent`] (`claude`): never a
    /// name that could change.
    fn id(&self) -> &'static str;
    /// Its name, as shown (translated where it has words).
    fn name(&self) -> String;
    /// Its icon (one its module contributes).
    fn icon(&self) -> &'static str;
    /// On a worker: whether it's installed, and where.
    fn detect(&self) -> Option<AgentInstall>;
    /// Whether it was found the last time it was looked for ([`Self::detect`]): cheap, asked
    /// as a button is drawn.
    fn installed(&self) -> bool {
        true
    }
    /// On a worker: its sessions working in `root`, the most recently active first, at most
    /// `limit`.
    fn sessions(&self, root: &Path, limit: usize) -> Vec<AgentSession>;
    /// A new session in `root`.
    fn new_session(&self, root: &Path) -> Launch;
    /// `session` taken up again.
    fn resume(&self, session: &AgentSession) -> Launch;
    /// On a worker: how its graphical face's adapter (an Agent Client Protocol server)
    /// starts in `root`; `None`: it has none here (not installed).
    fn acp(&self, root: &Path) -> Option<Launch> {
        let _ = root;
        None
    }
    /// On a worker, once the session doesn't run (a running agent writes the title it keeps
    /// in memory over its files): session `id`, working in `folder`, named `title` in the
    /// agent's own files, as its own rename does. The title's already one line, its length
    /// bounded. `Err`: it can't be (no such session, not this agent's way).
    fn rename_session(&self, folder: &Path, id: &str, title: &str) -> Result<(), String> {
        let _ = (folder, id, title);
        Err("this agent's sessions can't be renamed".into())
    }
    /// Whether its sessions can be renamed ([`Self::rename_session`]): cheap, asked as a
    /// menu is made.
    fn can_rename(&self) -> bool {
        false
    }
    /// On a worker: the commands that add the editor's MCP server (at `url`) to this agent's
    /// own configuration, in order (an old entry removed first); empty: it can't be
    /// configured so. `headers` is a shell command that prints, as a JSON object, the headers
    /// the agent sends (its token: never in these commands nor in the agent's files).
    fn mcp_setup(&self, url: &str, headers: &str) -> Vec<Launch> {
        let _ = (url, headers);
        Vec::new()
    }
    /// On a worker: how that adapter is installed (shown to the user, run in a terminal only
    /// on their word); `None`: no way known.
    fn acp_install(&self) -> Option<Launch> {
        None
    }
}

static PROVIDERS: RwLock<Vec<Arc<dyn AgentProvider>>> = RwLock::new(Vec::new());

/// Registers an agent (the app, at startup, from the modules' contributions: whoever needs
/// the agents finds them here and never names their modules); one with the same id is
/// replaced.
pub fn register(provider: Arc<dyn AgentProvider>) {
    let mut providers = PROVIDERS.write().unwrap_or_else(PoisonError::into_inner);
    providers.retain(|p| p.id() != provider.id());
    providers.push(provider);
}

/// Removes agent `id` (tests).
pub fn unregister(id: &str) {
    PROVIDERS.write().unwrap_or_else(PoisonError::into_inner).retain(|p| p.id() != id);
}

/// The agents offered, as registered.
pub fn providers() -> Vec<Arc<dyn AgentProvider>> {
    PROVIDERS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Whether some agent offered says `yes` (nothing copied: asked as a strip is drawn).
pub fn any(yes: impl Fn(&dyn AgentProvider) -> bool) -> bool {
    PROVIDERS.read().unwrap_or_else(PoisonError::into_inner).iter().any(|p| yes(p.as_ref()))
}

/// Every agent offered looked for on disk (a worker's: at startup, before a list is read),
/// what's found kept by each.
pub fn detect_all() {
    for provider in providers() {
        let _ = provider.detect();
    }
}

/// Agent `id`, if it's offered.
pub fn provider(id: &str) -> Option<Arc<dyn AgentProvider>> {
    PROVIDERS.read().unwrap_or_else(PoisonError::into_inner).iter().find(|p| p.id() == id).cloned()
}

/// An agent the editor knows of but doesn't host yet: shown among the agents (greyed out,
/// with its icon) so it's known what will come; one an [`AgentProvider`] offers is that
/// provider's instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedAgent {
    /// Its id, as its provider will have it (`codex`).
    pub id: &'static str,
    pub name: &'static str,
    /// Its icon (one its module contributes).
    pub icon: &'static str,
}

static PLANNED: RwLock<Vec<PlannedAgent>> = RwLock::new(Vec::new());

/// Registers an agent not hosted yet (the app, at startup, from the modules' contributions);
/// one with the same id is replaced.
pub fn register_planned(agent: PlannedAgent) {
    let mut planned = PLANNED.write().unwrap_or_else(PoisonError::into_inner);
    planned.retain(|p| p.id != agent.id);
    planned.push(agent);
}

/// The agents not hosted yet: those registered that no provider offers.
pub fn planned() -> Vec<PlannedAgent> {
    let planned = PLANNED.read().unwrap_or_else(PoisonError::into_inner).clone();
    planned.into_iter().filter(|p| provider(p.id).is_none()).collect()
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
