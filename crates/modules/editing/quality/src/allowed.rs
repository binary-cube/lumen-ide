//! The tools a project may run, as its user allowed them. The project's settings
//! (`<project dir>/project.toml`) can come with the project (a cloned repository), so what
//! they choose (a tool, a binary, a command line) runs only once allowed: chosen in the
//! settings window, or allowed there. The list is kept in the project's data folder, outside
//! the project, never shared with it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use code_quality::Choice;

/// The file in the project's data folder.
const FILE: &str = "quality-allowed";

/// A choice as written in the list (`None`: nothing runs).
pub(crate) fn entry(choice: &Choice) -> Option<String> {
    let (what, value) = match choice {
        Choice::None => return None,
        Choice::Tool(id) => ("tool", id),
        Choice::Binary { path, .. } => ("binary", path),
        Choice::Command { line, .. } => ("command", line),
    };
    Some(format!("{what}\t{}", value.escape_default()))
}

/// What a project's user allowed (read once, written on each change).
#[derive(Debug)]
pub(crate) struct Allowed {
    file: Option<PathBuf>,
    entries: Mutex<Option<HashSet<String>>>,
}

impl Allowed {
    /// The list kept in `data` (the project's data folder; `None`: nothing kept, so only
    /// what's allowed in this session).
    pub(crate) fn new(data: Option<&Path>) -> Self {
        Self { file: data.map(|d| d.join(FILE)), entries: Mutex::default() }
    }

    fn with<R>(&self, f: impl FnOnce(&mut HashSet<String>) -> R) -> Option<R> {
        let mut entries = self.entries.lock().ok()?;
        let entries = entries.get_or_insert_with(|| {
            let text = self.file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
            text.unwrap_or_default().lines().map(str::to_owned).collect()
        });
        Some(f(entries))
    }

    /// Whether `choice` may run (nothing chosen: nothing to allow).
    pub(crate) fn allows(&self, choice: &Choice) -> bool {
        entry(choice).is_none_or(|e| self.with(|all| all.contains(&e)).unwrap_or(false))
    }

    /// Allows `choice` from now on.
    pub(crate) fn allow(&self, choice: &Choice) {
        let Some(e) = entry(choice) else { return };
        let text = self.with(|all| {
            all.insert(e).then(|| {
                let mut lines: Vec<&str> = all.iter().map(String::as_str).collect();
                lines.sort_unstable();
                lines.join("\n") + "\n"
            })
        });
        let (Some(Some(text)), Some(file)) = (text, &self.file) else { return };
        if let Err(e) = files::write_private(file, text.as_bytes()) {
            tracing::warn!(error = %e, "can't keep the allowed code quality tools");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/allowed.rs"]
mod tests;
