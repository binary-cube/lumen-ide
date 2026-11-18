//! The server while it runs, and how a tool's call reaches the editor: the connection's
//! thread (`mcp-conn`) queues the call and wakes the windows, the window it's for runs it on
//! its UI thread at its next poll ([`take`]) and sends the outcome back; the connection waits
//! for it, a while at most — then it's given up, dropped from the queue, never run later.
//! A call about a file goes to the window whose project holds it; one about no file (or a
//! relative one) to the window the user last worked in. Locks are held only to move things
//! in or out — never while waking, calling a tool or waiting.

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mcp::{McpTool, Reply, ServerInfo, ToolKind, ToolOutcome};
use serde_json::Value;

/// How long a call waits for the editor (a window busy, or hidden: it's still polled); one
/// that asks the user first, longer (they decide).
const WAIT: Duration = Duration::from_secs(30);
const WAIT_ASKED: Duration = Duration::from_secs(300);

/// A tool's call waiting for a window.
struct Job {
    tool: Arc<dyn McpTool>,
    args: Value,
    /// The file it's about (absolute): the window whose project holds it runs it; none: the
    /// window last focused.
    path: Option<PathBuf>,
    reply: Reply,
}

/// A window that may run calls: its project's identity and root, and its wake.
struct Window {
    id: kernel::ProjectId,
    root: PathBuf,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// The server running: where, with which token.
pub struct Running {
    server: mcp_transport::Server,
    pub token: String,
}

impl Running {
    pub fn url(&self) -> String {
        self.server.url()
    }

    pub fn port(&self) -> u16 {
        self.server.port()
    }
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static JOBS: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
static WINDOWS: Mutex<Vec<Window>> = Mutex::new(Vec::new());
/// The window the user worked in last (told at its polls).
static FOCUSED: Mutex<Option<kernel::ProjectId>> = Mutex::new(None);
/// The port asked for was taken: the server listens on another (the agents configured with
/// the old one are to be configured again; the settings' page says so).
static MOVED: AtomicBool = AtomicBool::new(false);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Its address and token while it runs.
pub fn running() -> Option<(String, String)> {
    lock(&RUNNING).as_ref().map(|r| (r.url(), r.token.clone()))
}

/// Started on `port` (`0`: any) with `token`, or kept as it is if it already runs there;
/// the port it listens on returned (a taken one: another, [`moved`] then).
pub fn start(port: u16, token: &str) -> Result<u16, String> {
    let mut running = lock(&RUNNING);
    if let Some(r) = running.as_ref()
        && r.token == token
        && (port == 0 || r.port() == port)
    {
        return Ok(r.port());
    }
    // Another port or token: the one running stops first (its port freed).
    *running = None;
    let handler: mcp_transport::Handler = Arc::new(|message: &Value| {
        let info = ServerInfo { name: "editor", version: kernel::brand::version() };
        mcp::answer(message, info, &mcp::tools(), &mut call)
    });
    let server = match mcp_transport::Server::start(port, token.to_owned(), Arc::clone(&handler)) {
        Ok(server) => server,
        // Taken (by another program, another instance): one the system picks.
        Err(e) if port != 0 => {
            tracing::warn!("the MCP server's port {port} is taken ({e}): it listens on another");
            let server = mcp_transport::Server::start(0, token.to_owned(), handler)
                .map_err(|e| e.to_string())?;
            MOVED.store(true, Ordering::Release);
            server
        }
        Err(e) => return Err(e.to_string()),
    };
    let port = server.port();
    *running = Some(Running { server, token: token.to_owned() });
    Ok(port)
}

/// The port asked for was taken since the agents were configured (see [`MOVED`]).
pub fn moved() -> bool {
    MOVED.load(Ordering::Acquire)
}

/// The port changed (the user chose another): the agents are to be configured again.
pub fn mark_moved() {
    MOVED.store(true, Ordering::Release);
}

/// The agents configured again: the port they have is the one listened on.
pub fn configured() {
    MOVED.store(false, Ordering::Release);
}

/// Stopped (its connections end with their requests).
pub fn stop() {
    let stopped = lock(&RUNNING).take();
    drop(stopped);
}

/// `args`' `path`, as the file it names, when it's an absolute one.
fn path_of(args: &Value) -> Option<PathBuf> {
    let path = PathBuf::from(args.get("path")?.as_str()?);
    path.is_absolute().then_some(path)
}

/// Tool `name` called (on the connection's thread): queued for the editor, its outcome
/// waited for. `None`: no such tool.
fn call(name: &str, args: &Value) -> Option<ToolOutcome> {
    let tool = mcp::tool(name)?;
    let wait = if tool.kind() == ToolKind::Read { WAIT } else { WAIT_ASKED };
    // No window to run it: said at once (nothing queued that nobody takes).
    let wakes: Vec<_> = lock(&WINDOWS).iter().map(|w| Arc::clone(&w.wake)).collect();
    if wakes.is_empty() {
        return Some(ToolOutcome::Failed("no project is open in the editor".into()));
    }
    let (reply, outcome) = Reply::channel();
    lock(&JOBS).push_back(Job { tool, args: args.clone(), path: path_of(args), reply });
    for wake in wakes {
        wake();
    }
    // Past the wait, given up: dropped from the queue, refused if the user answers later.
    Some(
        outcome
            .wait(wait)
            .unwrap_or_else(|| ToolOutcome::Failed("the editor didn't answer in time".into())),
    )
}

/// Every open window woken.
pub fn wake_all() {
    let wakes: Vec<_> = lock(&WINDOWS).iter().map(|w| Arc::clone(&w.wake)).collect();
    for wake in wakes {
        wake();
    }
}

/// A window that may run calls (its project's service attached); gone with the
/// subscription returned.
pub fn attach(
    id: kernel::ProjectId,
    root: &Path,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> kernel::Subscription {
    lock(&WINDOWS).push(Window { id, root: root.to_path_buf(), wake });
    kernel::Subscription::on_end(move || lock(&WINDOWS).retain(|w| w.id != id))
}

/// Whether a job about `path` is for window `id` at `root` (`windows`: every open one's id
/// and root; `focused`: the one worked in last): a file of its project, or one of no open
/// project's — or no file — when it's the window worked in last (or, that one closed, the
/// first open).
fn for_window(
    path: Option<&Path>,
    id: kernel::ProjectId,
    root: &Path,
    windows: &[(kernel::ProjectId, PathBuf)],
    focused: Option<kernel::ProjectId>,
) -> bool {
    match path {
        Some(path) if path.starts_with(root) => true,
        Some(path) if windows.iter().any(|(_, r)| path.starts_with(r)) => false,
        _ => {
            let chosen = focused
                .filter(|f| windows.iter().any(|(w, _)| w == f))
                .or_else(|| windows.first().map(|(w, _)| *w));
            chosen == Some(id)
        }
    }
}

/// Window `id` polled (`focused`: it has the keyboard now): the calls for it (at `root`),
/// taken off the queue — run by the caller, on its UI thread. Those given up are dropped.
pub fn take(
    id: kernel::ProjectId,
    root: impl FnOnce() -> Option<PathBuf>,
    focused: bool,
) -> Vec<Taken> {
    if focused {
        *lock(&FOCUSED) = Some(id);
    }
    // Nothing queued (nearly always): nothing else looked at.
    if lock(&JOBS).is_empty() {
        return Vec::new();
    }
    let Some(root) = root() else { return Vec::new() };
    let windows: Vec<(kernel::ProjectId, PathBuf)> =
        lock(&WINDOWS).iter().map(|w| (w.id, w.root.clone())).collect();
    let last = *lock(&FOCUSED);
    let mut jobs = lock(&JOBS);
    let mut taken = Vec::new();
    jobs.retain(|job| !job.reply.given_up());
    let mut left = VecDeque::with_capacity(jobs.len());
    while let Some(job) = jobs.pop_front() {
        if for_window(job.path.as_deref(), id, &root, &windows, last) {
            taken.push(Taken(job));
        } else {
            left.push_back(job);
        }
    }
    *jobs = left;
    taken
}

/// A call taken for this window: the tool, its arguments, where its outcome goes.
pub struct Taken(Job);

impl Taken {
    pub fn into_parts(self) -> (Arc<dyn McpTool>, Value, Reply) {
        (self.0.tool, self.0.args, self.0.reply)
    }
}

/// `path` as an agent gives it, inside the project at `root`: absolute or relative to it, never
/// climbing out of it (`..`), as written (nothing read).
pub fn inside(root: &Path, path: &str) -> Option<PathBuf> {
    let given = Path::new(path);
    if given.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let path = if given.is_absolute() { given.to_path_buf() } else { root.join(given) };
    path.starts_with(root).then_some(path)
}

#[cfg(test)]
#[path = "../tests/unit/server.rs"]
mod tests;
