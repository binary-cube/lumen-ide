//! Diagnostics modules find in the project's files (a checker's problems), announced on the
//! project's bus ([`DiagnosticsFound`]) and kept per source and per file
//! ([`ProjectDiagnostics`], the project's data: gone with it). The app shows them in the
//! editor like its own errors and warnings — the same squiggles, scroll strip marks, hover,
//! Structure panel and status bar. A file's findings replace that file's; a project's, all
//! of that source's.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kernel::{DomainEvent, EventBus, Subscription};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    /// A note (shown faint).
    Info,
}

/// One finding in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Relative to the project's root.
    pub path: PathBuf,
    /// 0-based.
    pub line: u32,
    /// 0-based, in characters; `None`: the whole line.
    pub column: Option<u32>,
    /// Where it ends (0-based line and character), when its source says: the mark covers
    /// exactly that; `None`: the word at `column`.
    pub end: Option<(u32, u32)>,
    pub severity: Severity,
    pub message: String,
    /// Who found it (a tool's name: `PHPStan`) and the rule broken, if it says.
    pub source: String,
    pub rule: Option<String>,
}

/// What a source's findings are about: one file (relative to the project) or the whole
/// project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticsScope {
    File(PathBuf),
    Project,
}

/// A source's findings for a scope: they replace that scope's previous ones (an empty list:
/// it's clean now).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsFound {
    /// Its findings are kept apart from the other sources' (a checker's id: `phpstan`).
    pub source: String,
    pub scope: DiagnosticsScope,
    pub diagnostics: Vec<Diagnostic>,
}

impl DomainEvent for DiagnosticsFound {}

/// The project's diagnostics, by source, then by file (relative to the project): what was
/// announced on its bus, taken in on the UI thread ([`Self::sync`]).
#[derive(Default)]
pub struct ProjectDiagnostics {
    heard: Arc<Mutex<Vec<DiagnosticsFound>>>,
    by_source: HashMap<String, HashMap<PathBuf, Vec<Diagnostic>>>,
    /// Bumped on each change (what shows them is rebuilt).
    generation: u64,
    _subscription: Option<Subscription>,
}

impl std::fmt::Debug for ProjectDiagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectDiagnostics").field("generation", &self.generation).finish()
    }
}

impl ProjectDiagnostics {
    /// Follows what's announced on the project's `events` (until dropped).
    pub fn listen(events: &EventBus) -> Self {
        let heard: Arc<Mutex<Vec<DiagnosticsFound>>> = Arc::default();
        let into = Arc::clone(&heard);
        Self {
            _subscription: Some(events.subscribe(move |e: &DiagnosticsFound| {
                if let Ok(mut heard) = into.lock() {
                    heard.push(e.clone());
                }
            })),
            heard,
            ..Self::default()
        }
    }

    /// Takes in what was heard. `true` if anything changed.
    pub fn sync(&mut self) -> bool {
        let heard = self.heard.lock().map(|mut h| std::mem::take(&mut *h)).unwrap_or_default();
        if heard.is_empty() {
            return false;
        }
        for found in heard {
            self.apply(found);
        }
        self.by_source.retain(|_, files| !files.is_empty());
        self.generation += 1;
        true
    }

    fn apply(&mut self, found: DiagnosticsFound) {
        let files = self.by_source.entry(found.source).or_default();
        match found.scope {
            DiagnosticsScope::File(file) => {
                let diagnostics: Vec<Diagnostic> =
                    found.diagnostics.into_iter().filter(|d| d.path == file).collect();
                if diagnostics.is_empty() {
                    files.remove(&file);
                } else {
                    files.insert(file, diagnostics);
                }
            }
            DiagnosticsScope::Project => {
                files.clear();
                for diagnostic in found.diagnostics {
                    files.entry(diagnostic.path.clone()).or_default().push(diagnostic);
                }
            }
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A file's diagnostics (relative to the project) from the sources `keep` takes.
    pub fn of_sources<'a>(
        &'a self,
        file: &'a Path,
        keep: impl Fn(&str) -> bool + 'a,
    ) -> impl Iterator<Item = &'a Diagnostic> + 'a {
        self.by_source
            .iter()
            .filter(move |(source, _)| keep(source))
            .filter_map(move |(_, files)| files.get(file))
            .flatten()
    }

    /// A file's diagnostics (relative to the project), every source's.
    pub fn of<'a>(&'a self, file: &'a Path) -> impl Iterator<Item = &'a Diagnostic> + 'a {
        self.by_source.values().filter_map(move |files| files.get(file)).flatten()
    }
}

#[cfg(test)]
#[path = "../tests/unit/diagnostics.rs"]
mod tests;
