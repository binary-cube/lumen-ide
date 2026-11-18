//! The editor's MCP server (Model Context Protocol), as data: the tools modules offer the
//! agents on the machine ([`McpTool`], found in a neutral registry the app fills: the server
//! names no module), and the protocol's requests answered ([`answer`]: `initialize`,
//! `tools/list`, `tools/call`, `ping`). Pure: the socket is infrastructure's; a tool's call
//! is handed to whoever runs it (on the UI thread, with the editor).

use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use serde_json::{Value, json};

/// The protocol's versions the server speaks, the newest first.
pub const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// What a tool does, for what it may do unasked: reading is free; writing and running ask
/// the user first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Reads, or shows something in the editor (a file opened).
    Read,
    /// Changes a file.
    Write,
    /// Runs something (a query, a request, a process).
    Run,
}

/// What a tool's call comes to: its text, or why it failed (said to the agent, not thrown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    Text(String),
    Failed(String),
}

/// Where a tool's outcome goes, once: now, or later from a worker (a repository read). One
/// dropped unanswered says so for it — the user refused, a list was dismissed — so no call
/// ever waits for an answer nobody will give. Whoever waited may give up (too long): then
/// the call isn't run any more ([`Reply::given_up`]), however late the user allows it.
#[derive(Debug)]
pub struct Reply {
    tx: Option<std::sync::mpsc::Sender<ToolOutcome>>,
    given_up: Arc<AtomicBool>,
}

/// Where a call's outcome arrives, waited for a while at most.
#[derive(Debug)]
pub struct Outcome {
    rx: std::sync::mpsc::Receiver<ToolOutcome>,
    given_up: Arc<AtomicBool>,
}

impl Outcome {
    /// The outcome, or `None` past `wait` — the call given up then (not run after).
    pub fn wait(&self, wait: std::time::Duration) -> Option<ToolOutcome> {
        let outcome = self.rx.recv_timeout(wait).ok();
        if outcome.is_none() {
            self.given_up.store(true, Ordering::Release);
        }
        outcome
    }
}

impl Reply {
    /// A reply, and where its outcome arrives.
    pub fn channel() -> (Self, Outcome) {
        let (tx, rx) = std::sync::mpsc::channel();
        let given_up = Arc::new(AtomicBool::new(false));
        (Self { tx: Some(tx), given_up: Arc::clone(&given_up) }, Outcome { rx, given_up })
    }

    /// Whoever waited stopped waiting: the call isn't to be run.
    pub fn given_up(&self) -> bool {
        self.given_up.load(Ordering::Acquire)
    }

    /// `outcome` sent (whoever waited may have stopped: nothing then).
    pub fn send(mut self, outcome: ToolOutcome) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(outcome);
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(ToolOutcome::Failed("refused (not run)".into()));
        }
    }
}

/// A tool offered to the agents, by a module (or the server's own, over the host).
pub trait McpTool: Send + Sync + Debug {
    /// Its name (`open_file`): the same in every language, never the app's name.
    fn name(&self) -> &'static str;
    /// What it does, for the agent (English: the agents read it).
    fn description(&self) -> &'static str;
    /// Its arguments, as a JSON Schema.
    fn input_schema(&self) -> Value;
    fn kind(&self) -> ToolKind;
    /// Called on the UI thread with the window's host: short — what reads the disk or a
    /// repository moves `reply` to a worker and answers from there.
    fn call(&self, args: &Value, host: &mut dyn workbench_model::WorkbenchHost, reply: Reply);
}

static TOOLS: RwLock<Vec<Arc<dyn McpTool>>> = RwLock::new(Vec::new());

/// Registers a tool (the app, at startup, from the modules' contributions); one with the
/// same name is replaced.
pub fn register(tool: Arc<dyn McpTool>) {
    let mut tools = TOOLS.write().unwrap_or_else(PoisonError::into_inner);
    tools.retain(|t| t.name() != tool.name());
    tools.push(tool);
}

/// The tools offered, as registered.
pub fn tools() -> Vec<Arc<dyn McpTool>> {
    TOOLS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Tool `name`, if it's offered.
pub fn tool(name: &str) -> Option<Arc<dyn McpTool>> {
    TOOLS.read().unwrap_or_else(PoisonError::into_inner).iter().find(|t| t.name() == name).cloned()
}

/// What the program is run with to print the headers an agent sends (its `headersHelper`):
/// the token never on a command line nor in an agent's files.
pub const HELPER_ARG: &str = "--mcp-headers";
/// Where the server's token is kept: the keychain's service (after the app's id: `<id>.mcp`)
/// and the entry's name.
pub const TOKEN_SERVICE: &str = "mcp";
pub const TOKEN_ID: &str = "token";

/// The headers an agent sends with `token`, as the helper prints them (a JSON object).
pub fn helper_output(token: &str) -> String {
    json!({ "Authorization": format!("Bearer {token}") }).to_string()
}

/// The server's name and version, as it introduces itself.
#[derive(Debug, Clone, Copy)]
pub struct ServerInfo<'a> {
    pub name: &'a str,
    pub version: &'a str,
}

/// A JSON-RPC error's answer to `id`.
fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// A tool's result as the protocol gives it (`isError` when it failed).
pub fn result(outcome: &ToolOutcome) -> Value {
    let (text, failed) = match outcome {
        ToolOutcome::Text(text) => (text, false),
        ToolOutcome::Failed(why) => (why, true),
    };
    json!({"content": [{"type": "text", "text": text}], "isError": failed})
}

/// `tools/list`'s list.
fn listed(tools: &[Arc<dyn McpTool>]) -> Value {
    let tools: Vec<Value> = tools
        .iter()
        .map(|t| json!({"name": t.name(), "description": t.description(), "inputSchema": t.input_schema()}))
        .collect();
    json!({"tools": tools})
}

/// A request's answer (`None`: a notification, answered by nothing). `call` runs a tool by
/// its name and arguments (`None`: no such tool) — the caller hands it to the UI thread
/// and waits.
pub fn answer(
    message: &Value,
    info: ServerInfo<'_>,
    tools: &[Arc<dyn McpTool>],
    call: &mut dyn FnMut(&str, &Value) -> Option<ToolOutcome>,
) -> Option<Value> {
    let method = message.get("method").and_then(Value::as_str)?;
    // A notification (`notifications/initialized`…) has no id and gets no answer.
    let id = message.get("id")?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let ok = |result: Value| json!({"jsonrpc": "2.0", "id": id, "result": result});
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked.filter(|v| VERSIONS.contains(v)).unwrap_or(VERSIONS[0]);
            ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": info.name, "version": info.version}
            }))
        }
        "ping" => ok(json!({})),
        "tools/list" => ok(listed(tools)),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, -32602, "a tool's name is needed"));
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call(name, &args) {
                Some(outcome) => ok(result(&outcome)),
                None => error(id, -32602, &format!("no tool named {name}")),
            }
        }
        _ => error(id, -32601, "not offered by this server"),
    })
}

/// A string argument `key` of `args`, or why not.
pub fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolOutcome> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolOutcome::Failed(format!("`{key}` is needed (a string)")))
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
