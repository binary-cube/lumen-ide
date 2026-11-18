//! The messages on the wire (JSON-RPC 2.0, one a line): what the agent sends read into
//! [`Inbound`], what the editor sends written by the builders here. Only the parts the
//! editor uses are given shapes; everything else is kept as JSON or let go.

use serde_json::{Value, json};

/// The protocol's version the editor speaks.
pub const VERSION: u64 = 1;

/// A message from the agent.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// The answer to one of ours: its result, or its error's message.
    Response { id: u64, result: Result<Value, String> },
    /// A request the agent makes of the editor (a permission), answered by its id.
    Request { id: Value, method: String, params: Value },
    /// Something the agent tells (a session's update).
    Notification { method: String, params: Value },
}

/// One line from the agent, read (`None`: not a message, or not one of ours).
pub fn read(line: &str) -> Option<Inbound> {
    let message: Value = serde_json::from_str(line.trim()).ok()?;
    let method = message.get("method").and_then(Value::as_str).map(str::to_owned);
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match (method, message.get("id")) {
        (Some(method), Some(id)) => Some(Inbound::Request { id: id.clone(), method, params }),
        (Some(method), None) => Some(Inbound::Notification { method, params }),
        (None, Some(id)) => {
            let id = id.as_u64()?;
            let result = match message.get("error") {
                Some(error) => Err(error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("the agent refused")
                    .to_owned()),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            Some(Inbound::Response { id, result })
        }
        (None, None) => None,
    }
}

/// A request of ours, as a line.
pub fn request(id: u64, method: &str, params: Value) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
}

/// A notification of ours, as a line.
pub fn notification(method: &str, params: Value) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "method": method, "params": params}))
}

/// Our answer to the agent's request `id`.
pub fn response(id: &Value, result: Value) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Our refusal of the agent's request `id` (a method the editor doesn't offer).
pub fn error(id: &Value, code: i64, message: &str) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
    )
}

/// `initialize`'s parameters: the editor reads and writes no files for the agent and runs no
/// terminals for it (the agent does its own, asking permission); `name` / `version` its own.
pub fn initialize(name: &str, version: &str) -> Value {
    json!({
        "protocolVersion": VERSION,
        "clientCapabilities": {
            "fs": {"readTextFile": false, "writeTextFile": false},
            "terminal": false
        },
        "clientInfo": {"name": name, "version": version}
    })
}

/// Whether the agent can take a session up again (`initialize`'s answer).
pub fn loads_sessions(initialized: &Value) -> bool {
    initialized.pointer("/agentCapabilities/loadSession").and_then(Value::as_bool) == Some(true)
}

/// `session/new` in `cwd`.
pub fn new_session(cwd: &str) -> Value {
    json!({"cwd": cwd, "mcpServers": []})
}

/// `session/load` of `session` in `cwd` (its history replayed as updates first).
pub fn load_session(session: &str, cwd: &str) -> Value {
    json!({"sessionId": session, "cwd": cwd, "mcpServers": []})
}

/// `session/prompt`: `text` typed, in `session`.
pub fn prompt(session: &str, text: &str) -> Value {
    json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]})
}

/// `session/cancel`'s parameters.
pub fn cancel(session: &str) -> Value {
    json!({"sessionId": session})
}

/// The answer to a permission: option `chosen` (`None`: the turn was cancelled).
pub fn permission(chosen: Option<&str>) -> Value {
    match chosen {
        Some(option) => json!({"outcome": {"outcome": "selected", "optionId": option}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
}

#[cfg(test)]
#[path = "../tests/unit/wire.rs"]
mod tests;
