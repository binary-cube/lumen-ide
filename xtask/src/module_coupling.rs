//! The rules keeping modules from knowing each other, over their sources: none declares a
//! dependency on another (`depends_on`), reads another's files (`include_str!` into its
//! folder) or names one of its commands or views by their ids. Neutral contracts live in
//! `workbench-model` and the other shared crates, where any module may meet them.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// A source file of a module.
#[derive(Debug, Clone)]
pub struct Source {
    /// Its module's folder (`crates/modules/tools/relax`).
    pub module: String,
    /// The file (relative to the workspace, with `/`).
    pub path: String,
    pub text: String,
}

/// Every module's sources under `root/crates/modules`.
pub fn read_sources(root: &Path) -> Vec<Source> {
    let mut found = Vec::new();
    let Ok(families) = std::fs::read_dir(root.join("crates/modules")) else { return found };
    for family in families.flatten() {
        let Ok(modules) = std::fs::read_dir(family.path()) else { continue };
        for module in modules.flatten() {
            let dir = module.path();
            if !dir.join("Cargo.toml").is_file() {
                continue;
            }
            let name = relative(root, &dir);
            collect(root, &dir.join("src"), &name, &mut found);
        }
    }
    found
}

fn collect(root: &Path, dir: &Path, module: &str, found: &mut Vec<Source>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, module, found);
        } else if path.extension().is_some_and(|e| e == "rs")
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            found.push(Source { module: module.to_owned(), path: relative(root, &path), text });
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The string literals of `text` (no escapes needed: ids and paths have none).
fn literals(text: &str) -> impl Iterator<Item = &str> {
    text.split('"').skip(1).step_by(2)
}

/// The first `"…"` after each `marker` in `text`.
fn after<'a>(text: &'a str, marker: &'a str) -> impl Iterator<Item = &'a str> {
    text.match_indices(marker).filter_map(|(at, _)| literals(&text[at..]).next())
}

/// `path` with its `..` resolved.
fn normal(path: &Path) -> String {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out.to_string_lossy().replace('\\', "/")
}

/// A module's family folder (`crates/modules/database`): its modules may share a section.
fn family(module: &str) -> &str {
    module.rsplit_once('/').map_or(module, |(family, _)| family)
}

/// What breaks the rules among `sources`, one line each.
pub fn violations(sources: &[Source]) -> Vec<String> {
    // Which module declares each command and view.
    let mut owners: HashMap<&str, &str> = HashMap::new();
    // Which module's window each tool id is (short common words: looked for only where a
    // window is named).
    let mut tools: HashMap<&str, &str> = HashMap::new();
    for source in sources {
        for id in after(&source.text, "CommandId(").filter(|i| i.contains('.')) {
            owners.insert(id, &source.module);
        }
        for id in after(&source.text, "EXTENSION: &str =").filter(|i| i.starts_with("view-")) {
            owners.insert(id, &source.module);
        }
        tools.extend(after(&source.text, "TOOL: &str =").map(|id| (id, source.module.as_str())));
    }
    let mut found = Vec::new();
    // A settings section is one module's.
    let mut sections: HashMap<&str, &str> = HashMap::new();
    for source in sources {
        for section in after(&source.text, "Setting::new(") {
            match sections.get(section) {
                Some(owner) if family(owner) != family(&source.module) => found.push(format!(
                    "{}: writes the settings section `{section}`, {owner}'s",
                    source.path
                )),
                _ => {
                    sections.insert(section, &source.module);
                }
            }
        }
    }
    for source in sources {
        if source.text.contains("depends_on: &[\"") {
            found.push(format!("{}: a module may not depend on another (depends_on)", source.path));
        }
        for marker in ["include_str!(", "include_bytes!("] {
            for file in after(&source.text, marker) {
                let dir = Path::new(&source.path).parent().unwrap_or(Path::new(""));
                let target = normal(&dir.join(file));
                if target.starts_with("crates/modules/")
                    && !target.starts_with(&format!("{}/", source.module))
                {
                    found.push(format!("{}: reads another module's file {target}", source.path));
                }
            }
        }
        // Its own module id may read like another's command: that names nothing.
        let own: Vec<&str> = after(&source.text, "new_static(").collect();
        for call in ["show_side_panel(", "toggle_tool(", "tell_panel(", "show_in_panel("] {
            for tool in after(&source.text, call) {
                if tools.get(tool).is_some_and(|owner| *owner != source.module) {
                    found.push(format!("{}: names the window `{tool}`", source.path));
                }
            }
        }
        for literal in literals(&source.text).filter(|l| !own.contains(l)) {
            if let Some(owner) = owners.get(literal)
                && *owner != source.module
            {
                found.push(format!("{}: names `{literal}`, {owner}'s", source.path));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
#[path = "../tests/unit/module_coupling.rs"]
mod tests;
