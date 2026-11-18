//! The rules `check-deps` reads in the sources, beyond the crates' dependencies:
//!
//! - the app names no module (`project_tree::…`, `use lang_php as php`) outside the
//!   loading (`modules.rs`): what it needs comes through contributions;
//! - the app writes no module's command id (`"git.log"`) or thread name (`"git-status"`) as
//!   text: each module reports its own;
//! - the workbench (`crates/app/src/workbench/`) names no adapter (`storage`, `fs_local`):
//!   it reaches the outside through its `Ports`.
//!
//! The code built only for the snapshots (a `mod` behind `#[cfg(feature = "snapshot")]`)
//! drives the modules by name and is left out: the shipped binary doesn't have it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The adapters the workbench never names: it gets them as ports (`Ports::local()`, outside
/// `workbench/`).
pub const WORKBENCH_ADAPTERS: &[&str] = &["storage", "fs_local"];

/// Text in the app equal to a module's command or thread that is something else, with why:
/// (file under `crates/app/src`, the text).
pub const NOT_A_MODULE_NAME: &[(&str, &str)] = &[
    // The editing action a side panel is asked to run (`side_command`), not the tree's
    // `paste` thread.
    ("app/actions.rs", "paste"),
];

/// An app source: its path under `crates/app/src` (with `/`) and its text.
#[derive(Debug)]
pub struct Source {
    pub path: String,
    pub text: String,
}

/// The `.rs` files under `dir`, with their paths relative to it.
pub fn read_sources(dir: &Path) -> Vec<Source> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(at) = dirs.pop() {
        for entry in std::fs::read_dir(&at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|path: PathBuf| {
            let text = std::fs::read_to_string(&path).ok()?;
            let relative = path.strip_prefix(dir).unwrap_or(&path);
            Some(Source { path: relative.to_string_lossy().replace('\\', "/"), text })
        })
        .collect()
}

/// The sources built only for the snapshots: the files (and folders) of each `mod` declared
/// right after `#[cfg(feature = "snapshot")]`.
pub fn snapshot_only(sources: &[Source]) -> Vec<String> {
    let mut found = Vec::new();
    for source in sources {
        // A module's children live next to `main.rs` / `lib.rs` / `mod.rs`, otherwise in the
        // folder named after the file.
        let (dir, file) = source.path.rsplit_once('/').unwrap_or(("", &source.path));
        let base = match file {
            "main.rs" | "lib.rs" | "mod.rs" => dir.to_owned(),
            other => {
                let stem = other.trim_end_matches(".rs");
                if dir.is_empty() { stem.to_owned() } else { format!("{dir}/{stem}") }
            }
        };
        let mut gated = false;
        for line in source.text.lines().map(str::trim) {
            if line == "#[cfg(feature = \"snapshot\")]" {
                gated = true;
                continue;
            }
            if gated && let Some(name) = declared_mod(line) {
                found.push(if base.is_empty() {
                    name.to_owned()
                } else {
                    format!("{base}/{name}")
                });
            }
            if !line.starts_with("#[") {
                gated = false;
            }
        }
    }
    found
}

/// `mod name;` / `pub(crate) mod name;`: the name.
fn declared_mod(line: &str) -> Option<&str> {
    let rest =
        line.strip_prefix("pub ").or_else(|| line.strip_prefix("pub(crate) ")).unwrap_or(line);
    rest.strip_prefix("mod ")?.strip_suffix(';').map(str::trim)
}

/// Whether `path` (under `crates/app/src`) is in one of the `modules` (as `snapshot_only`
/// gives them).
pub fn is_within(path: &str, modules: &[String]) -> bool {
    let stem = path.trim_end_matches(".rs");
    modules.iter().any(|m| stem == m || path.starts_with(&format!("{m}/")))
}

/// The code of a line: without its comment (`//`, `///`, `//!`).
pub fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// Where in `code` crate `name` (`project_tree`) is named: a path rooted at it
/// (`project_tree::X`, `::project_tree::X`, not `crate::project_tree::X` nor
/// `my_project_tree::X`), or it renamed on the way in (`use lang_php as php;`).
pub fn names_crate(code: &str, name: &str) -> bool {
    let aliased = ["use ", "extern crate "].iter().any(|start| {
        code.trim_start().strip_prefix(start).is_some_and(|rest| {
            rest.strip_prefix(name).is_some_and(|after| {
                !after.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == ':')
            })
        })
    });
    if aliased {
        return true;
    }
    let needle = format!("{name}::");
    code.match_indices(&needle).any(|(at, _)| {
        let before = &code[..at];
        // Part of a longer name, or a path's inner segment (`crate::x::`).
        if before.ends_with(|c: char| c.is_alphanumeric() || c == '_') {
            return false;
        }
        let inner = before
            .strip_suffix("::")
            .is_some_and(|outer| outer.ends_with(|c: char| c.is_alphanumeric() || c == '_'));
        !inner
    })
}

/// The string literals in a line of code (`"…"`; escaped quotes kept inside).
pub fn literals(code: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = None;
    let mut escaped = false;
    for (i, c) in code.char_indices() {
        match (start, c) {
            (Some(_), '\\') if !escaped => {
                escaped = true;
                continue;
            }
            (Some(from), '"') if !escaped => {
                found.push(&code[from..i]);
                start = None;
            }
            // A char literal (`'"'`) isn't a string's start.
            (None, '"') if !code[..i].ends_with('\'') => start = Some(i + 1),
            _ => {}
        }
        escaped = false;
    }
    found
}

/// What the modules name themselves: their commands' ids and their threads' names.
#[derive(Debug, Default)]
pub struct ModuleNames {
    /// `CommandId("git.log")`.
    pub commands: BTreeSet<String>,
    /// `.name("…")`, `work::run("…")`, `Pool::new("…")`, `const …THREAD…: &str = "…"`, and
    /// the names in `ModuleThreads(&[…])` (one ending in `-` is a prefix: `"remote-"`).
    pub threads: BTreeSet<String>,
}

impl ModuleNames {
    /// The names declared in the modules' `src` folders (also those in a family's folder:
    /// `modules/languages/php`).
    pub fn read(modules: &Path) -> Self {
        let mut names = Self::default();
        for entry in std::fs::read_dir(modules).into_iter().flatten().flatten() {
            let dir = entry.path();
            let crates: Vec<std::path::PathBuf> = if dir.join("Cargo.toml").is_file() {
                vec![dir]
            } else {
                std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect()
            };
            for dir in crates {
                for source in read_sources(&dir.join("src")) {
                    names.add(&source.text);
                }
            }
        }
        names
    }

    /// The names a module's source declares.
    pub fn add(&mut self, text: &str) {
        let after = |start: &str| -> Vec<String> {
            text.match_indices(start)
                .filter_map(|(at, _)| {
                    let rest = &text[at + start.len()..];
                    rest.find('"').map(|end| rest[..end].to_owned())
                })
                .collect()
        };
        self.commands.extend(after("CommandId(\""));
        for start in [".name(\"", "work::run(\"", "Pool::new(\""] {
            self.threads.extend(after(start));
        }
        for line in text.lines().map(code_of) {
            if line.split_whitespace().any(|w| w == "const")
                && line.contains("THREAD")
                && let Some((_, value)) = line.split_once('=')
            {
                self.threads.extend(literals(value).into_iter().map(str::to_owned));
            }
        }
        for (at, _) in text.match_indices("ModuleThreads(&[") {
            let rest = &text[at..];
            let list = &rest[..rest.find(']').unwrap_or(rest.len())];
            for line in list.lines().map(code_of) {
                self.threads.extend(literals(line).into_iter().map(str::to_owned));
            }
        }
        self.commands.retain(|c| !c.is_empty());
        self.threads.retain(|t| !t.is_empty());
    }

    /// What `text` (a literal in the app) is of a module's: "command" or "thread".
    pub fn kind_of(&self, text: &str) -> Option<&'static str> {
        if self.commands.contains(text) {
            return Some("command");
        }
        let thread = self
            .threads
            .iter()
            .any(|t| t == text || (t.ends_with('-') && text.starts_with(t.as_str())));
        thread.then_some("thread")
    }
}

/// The rules over the app's sources (see the module's comment); `modules`: the modules'
/// library names (`project_tree`).
pub fn violations(sources: &[Source], modules: &[String], names: &ModuleNames) -> Vec<String> {
    let snapshots = snapshot_only(sources);
    let mut found = Vec::new();
    for source in sources {
        // The loading names every module; the snapshots drive them.
        if source.path == "modules.rs" || is_within(&source.path, &snapshots) {
            continue;
        }
        let in_workbench = source.path.starts_with("workbench/");
        for (n, line) in source.text.lines().enumerate() {
            let at = format!("crates/app/src/{}:{}", source.path, n + 1);
            let code = code_of(line);
            for module in modules.iter().filter(|m| names_crate(code, m)) {
                found.push(format!(
                    "{at}: the app names the `{module}` module (use a contribution)"
                ));
            }
            if in_workbench {
                for adapter in WORKBENCH_ADAPTERS.iter().filter(|a| names_crate(code, a)) {
                    found.push(format!(
                        "{at}: the workbench names the `{adapter}` adapter (take it through `Ports`)"
                    ));
                }
            }
            for text in literals(code) {
                if NOT_A_MODULE_NAME.contains(&(source.path.as_str(), text)) {
                    continue;
                }
                if let Some(kind) = names.kind_of(text) {
                    found.push(format!(
                        "{at}: the app writes a module's {kind} as text (\"{text}\"): the module \
                         reports or runs it itself"
                    ));
                }
            }
        }
    }
    found
}

#[cfg(test)]
#[path = "../tests/unit/app_sources.rs"]
mod tests;
