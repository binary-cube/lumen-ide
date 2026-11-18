//! The project's console commands: `#[AsCommand(name: 'app:x', description: '…')]` (or its
//! name as the first argument) on the classes in `src/`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use code_intelligence::{Project, line_of};
use kernel::GenerationalCache;

use crate::routes::{first_string, named};

/// Each source file's commands, with the time the file was read: only files changed since
/// are read again.
pub type FileCommands = GenerationalCache<PathBuf, (Option<SystemTime>, Arc<Vec<Command>>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub file: PathBuf,
    /// Where its name is written (in the attribute).
    pub offset: usize,
    pub line: u32,
}

/// The commands of the project's `src` files (`sources`, relative to it).
pub fn gather(project: &Project<'_>, sources: &[String], files: &mut FileCommands) -> Vec<Command> {
    let Some(root) = project.root else { return Vec::new() };
    let mut commands = Vec::new();
    for relative in sources {
        let file = root.join("src").join(relative);
        let modified = (project.modified)(&file);
        let cached = files.get(&file).filter(|(m, _)| modified.is_some() && *m == modified);
        let found = match cached {
            Some((_, found)) => Arc::clone(found),
            None => {
                let text = (project.read)(&file).unwrap_or_default();
                let found = if text.contains("AsCommand(") {
                    Arc::new(from_attributes(&text, &file))
                } else {
                    Arc::default()
                };
                files.insert(file, (modified, Arc::clone(&found)));
                found
            }
        };
        commands.extend(found.iter().cloned());
    }
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    commands.dedup_by(|a, b| a.name == b.name);
    commands
}

/// `#[AsCommand('app:x', 'Does x')]` / `#[AsCommand(name: 'app:x', description: 'Does x')]`.
pub(crate) fn from_attributes(text: &str, file: &std::path::Path) -> Vec<Command> {
    const OPEN: &str = "AsCommand(";
    let mut commands = Vec::new();
    for (at, _) in text.match_indices(OPEN) {
        let end = text[at..].find(")]").map_or(text.len(), |e| at + e);
        let args = &text[at + OPEN.len()..end];
        let Some((name, name_at)) = named(args, "name").or_else(|| first_string(args)) else {
            continue;
        };
        let description = named(args, "description").map(|(d, _)| d.to_owned()).or_else(|| {
            // The second argument, when the first is the name: a string right after the comma
            // (a named argument starts with its name, never a quote).
            let after = &args[name_at + name.len() + 1..];
            let rest = after.trim_start().strip_prefix(',')?;
            let (d, _) = first_string(rest)?;
            Some(d.to_owned())
        });
        let offset = at + OPEN.len() + name_at;
        commands.push(Command {
            name: name.to_owned(),
            description,
            file: file.to_path_buf(),
            offset,
            line: line_of(text, offset),
        });
    }
    commands
}

#[cfg(test)]
#[path = "../tests/unit/commands.rs"]
mod tests;
