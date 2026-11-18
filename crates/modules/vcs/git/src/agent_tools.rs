//! What Git offers the agents through the editor's MCP server (`mcp::McpTool`: the server
//! names no module, this one names no server): the repository's branch and changed files, a
//! file as it is at `HEAD`. Both read the repository on a worker (`mcp-git`), never on the
//! UI thread; the reply goes back from there.

use mcp::{McpTool, Reply, ToolKind, ToolOutcome, text_arg};
use serde_json::{Value, json};
use version_control::{FileState, Revision};
use workbench_model::WorkbenchHost;

/// The most changed files listed (the rest counted), and the biggest file sent whole: an
/// agent's answer stays small enough to read.
const FILES_MOST: usize = 2_000;
const TEXT_MOST: usize = 2 * 1024 * 1024;

/// A state as an agent reads it.
fn state(state: FileState) -> &'static str {
    match state {
        FileState::Modified => "modified",
        FileState::Added => "added",
        FileState::Deleted => "deleted",
        FileState::Renamed => "renamed",
        FileState::Untracked => "untracked",
        FileState::Conflicted => "conflicted",
    }
}

#[derive(Debug)]
pub struct GitStatus;

impl McpTool for GitStatus {
    fn name(&self) -> &'static str {
        "git_status"
    }
    fn description(&self) -> &'static str {
        "The project's git branch and its changed files, each with its state (modified, added, deleted, renamed, untracked, conflicted)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn call(&self, _args: &Value, host: &mut dyn WorkbenchHost, reply: Reply) {
        let Some(repo) = host.repository() else {
            return reply.send(ToolOutcome::Failed("the project isn't in a git repository".into()));
        };
        kernel::work::run("mcp-git", move || {
            // Given up while it waited for the worker: not read for nothing.
            if reply.given_up() {
                return;
            }
            let outcome = match repo.status() {
                Ok(all) => {
                    let files: Vec<Value> = all
                        .iter()
                        .take(FILES_MOST)
                        .map(|f| json!({"path": f.path, "state": state(f.state)}))
                        .collect();
                    let mut answer = json!({"branch": repo.branch(), "files": files});
                    if all.len() > FILES_MOST {
                        answer["more"] = json!(all.len() - FILES_MOST);
                    }
                    ToolOutcome::Text(answer.to_string())
                }
                Err(e) => ToolOutcome::Failed(e.to_string()),
            };
            reply.send(outcome);
        });
    }
}

#[derive(Debug)]
pub struct GitHeadText;

impl McpTool for GitHeadText {
    fn name(&self) -> &'static str {
        "git_file_at_head"
    }
    fn description(&self) -> &'static str {
        "A file's text as committed at HEAD (relative to the repository, with /): to compare with what's changed since."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn call(&self, args: &Value, host: &mut dyn WorkbenchHost, reply: Reply) {
        let path = match text_arg(args, "path") {
            Ok(path) => path.replace('\\', "/"),
            Err(why) => return reply.send(why),
        };
        // Within the repository: no climbing out, no absolute path.
        if path.starts_with('/') || path.split('/').any(|part| part == "..") {
            return reply
                .send(ToolOutcome::Failed(format!("{path} isn't a path in the repository")));
        }
        let Some(repo) = host.repository() else {
            return reply.send(ToolOutcome::Failed("the project isn't in a git repository".into()));
        };
        kernel::work::run("mcp-git", move || {
            if reply.given_up() {
                return;
            }
            let outcome = match repo.text_at(&Revision::Head, &path) {
                Some(text) if text.len() > TEXT_MOST => ToolOutcome::Failed(format!(
                    "{path} is too big to send ({} bytes, at most {TEXT_MOST})",
                    text.len()
                )),
                Some(text) => ToolOutcome::Text(text),
                None => ToolOutcome::Failed(format!("{path} isn't at HEAD")),
            };
            reply.send(outcome);
        });
    }
}

#[cfg(test)]
#[path = "../tests/unit/agent_tools.rs"]
mod tests;
