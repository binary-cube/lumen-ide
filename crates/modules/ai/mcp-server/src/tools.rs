//! The editor's own tools, over the host (any editor has them; the modules add theirs): a
//! file opened at a line, what's selected, the open tabs, an open file's text as the editor
//! has it. Each reads only what the window already holds (nothing from disk on its UI
//! thread) and keeps to the project's files.

use mcp::{McpTool, Reply, ToolKind, ToolOutcome, text_arg};
use serde_json::{Value, json};
use workbench_model::WorkbenchHost;

use crate::server::inside;

/// The project's root, or why there's none.
fn root(host: &dyn WorkbenchHost) -> Result<std::path::PathBuf, ToolOutcome> {
    host.project_root().ok_or_else(|| ToolOutcome::Failed("no project is open".into()))
}

/// `path` relative to the project's root, `/`-separated (as an agent names files).
fn relative(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Where line `line` (1-based) starts in `text` (past its end: the end).
fn line_start(text: &str, line: u64) -> usize {
    let wanted = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
    if wanted == 0 {
        return 0;
    }
    text.match_indices('\n').nth(wanted - 1).map_or(text.len(), |(at, _)| at + 1)
}

#[derive(Debug)]
pub struct OpenFile;

impl McpTool for OpenFile {
    fn name(&self) -> &'static str {
        "open_file"
    }
    fn description(&self) -> &'static str {
        "Opens a file of the project in the editor, at a line when given (1-based)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string", "description": "The file: relative to the project, or absolute inside it"},
            "line": {"type": "integer", "minimum": 1}
        }, "required": ["path"]})
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn call(&self, args: &Value, host: &mut dyn WorkbenchHost, reply: Reply) {
        reply.send(Self::run(args, host));
    }
}

impl OpenFile {
    fn run(args: &Value, host: &mut dyn WorkbenchHost) -> ToolOutcome {
        let root = match root(host) {
            Ok(root) => root,
            Err(why) => return why,
        };
        let path = match text_arg(args, "path") {
            Ok(path) => path,
            Err(why) => return why,
        };
        let Some(file) = inside(&root, path) else {
            return ToolOutcome::Failed(format!("{path} isn't a file of the project"));
        };
        if !host.open_file(&file) {
            return ToolOutcome::Failed(format!("{path} couldn't be opened"));
        }
        if let Some(line) = args.get("line").and_then(Value::as_u64)
            && let Some(active) = host.active_file()
        {
            let at = line_start(&active.text, line);
            host.select_active(at..at);
        }
        ToolOutcome::Text(format!("opened {}", relative(&file, &root)))
    }
}

#[derive(Debug)]
pub struct Selection;

impl McpTool for Selection {
    fn name(&self) -> &'static str {
        "get_selection"
    }
    fn description(&self) -> &'static str {
        "The file the user is in, the cursor's line (1-based) and the text selected, if any."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn call(&self, args: &Value, host: &mut dyn WorkbenchHost, reply: Reply) {
        reply.send(Self::run(args, host));
    }
}

impl Selection {
    fn run(_args: &Value, host: &mut dyn WorkbenchHost) -> ToolOutcome {
        let Some(active) = host.active_file() else {
            return ToolOutcome::Text(json!({"file": null}).to_string());
        };
        let root = host.project_root().unwrap_or_default();
        let selected =
            active.selection.as_ref().and_then(|r| active.text.get(r.clone())).unwrap_or_default();
        let said = json!({
            "file": relative(&active.path, &root),
            "line": active.line.saturating_add(1),
            "selection": selected,
        });
        ToolOutcome::Text(said.to_string())
    }
}

#[derive(Debug)]
pub struct OpenFiles;

impl McpTool for OpenFiles {
    fn name(&self) -> &'static str {
        "get_open_files"
    }
    fn description(&self) -> &'static str {
        "The files open in the editor's tabs (relative to the project)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn call(&self, args: &Value, host: &mut dyn WorkbenchHost, reply: Reply) {
        reply.send(Self::run(args, host));
    }
}

impl OpenFiles {
    fn run(_args: &Value, host: &mut dyn WorkbenchHost) -> ToolOutcome {
        let root = host.project_root().unwrap_or_default();
        let files: Vec<String> = host
            .tabs()
            .iter()
            .filter_map(|t| t.path.as_deref())
            .map(|p| relative(p, &root))
            .collect();
        ToolOutcome::Text(json!({"files": files}).to_string())
    }
}

#[derive(Debug)]
pub struct FileText;

impl McpTool for FileText {
    fn name(&self) -> &'static str {
        "get_file_text"
    }
    fn description(&self) -> &'static str {
        "An open file's text as the editor has it, unsaved changes included (a file that isn't open: read it from disk yourself)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn call(&self, args: &Value, host: &mut dyn WorkbenchHost, reply: Reply) {
        reply.send(Self::run(args, host));
    }
}

impl FileText {
    fn run(args: &Value, host: &mut dyn WorkbenchHost) -> ToolOutcome {
        let root = match root(host) {
            Ok(root) => root,
            Err(why) => return why,
        };
        let path = match text_arg(args, "path") {
            Ok(path) => path,
            Err(why) => return why,
        };
        let Some(file) = inside(&root, path) else {
            return ToolOutcome::Failed(format!("{path} isn't a file of the project"));
        };
        // Only an open one: its text is in memory (a closed one would be read from disk here).
        if !host.tabs().iter().any(|t| t.path.as_deref() == Some(file.as_path())) {
            return ToolOutcome::Failed(format!("{path} isn't open in the editor"));
        }
        match host.file_text(&file) {
            Some(text) => ToolOutcome::Text(text),
            None => ToolOutcome::Failed(format!("{path} has no text here")),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/tools.rs"]
mod tests;
