//! Completion in a `.http` file ([`crate::complete`]'s places and items), with the variables
//! a `{{` may take: the file's own, and its environments' — read from the files beside it
//! (`http-client.env.json`, the private and local ones; the nearest folder up to the
//! project's root that has one), in the background, kept by folder. The assist answers from
//! what's read already; a folder read for the first time (or again, a moment after) wakes
//! the app and bumps its generation, so a list open is made again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use code_intelligence::{AssistContext, DocumentAssist, FrameworkCompletion};
use http_requests::env::Environments;

/// How long a folder's environments are trusted before they're read again.
const FRESH: Duration = Duration::from_secs(2);

/// The thread the environments are read on.
pub const THREAD: &str = "http-env";

/// A folder's environments, as last read.
#[derive(Debug)]
struct Read {
    at: Instant,
    environments: Arc<Environments>,
}

#[derive(Debug, Default)]
pub struct Completion {
    /// The `.http` file's folder → its environments.
    read: Arc<Mutex<HashMap<PathBuf, Read>>>,
    /// Folders being read now.
    reading: Arc<Mutex<Vec<PathBuf>>>,
    generation: Arc<AtomicU64>,
}

impl Completion {
    /// `file`'s environments as read (none yet: empty), reading them again in the background
    /// when they're not fresh.
    fn environments(&self, file: &Path, root: Option<&Path>) -> Arc<Environments> {
        let folder = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let known = {
            let read = self.read.lock().unwrap_or_else(PoisonError::into_inner);
            read.get(&folder).map(|r| (r.at.elapsed() < FRESH, Arc::clone(&r.environments)))
        };
        if known.as_ref().is_none_or(|(fresh, _)| !fresh) {
            self.read_in_background(file.to_path_buf(), folder, root.map(Path::to_path_buf));
        }
        known.map(|(_, e)| e).unwrap_or_default()
    }

    fn read_in_background(&self, file: PathBuf, folder: PathBuf, root: Option<PathBuf>) {
        {
            let mut reading = self.reading.lock().unwrap_or_else(PoisonError::into_inner);
            if reading.contains(&folder) {
                return;
            }
            reading.push(folder.clone());
        }
        let (read, reading, generation) =
            (Arc::clone(&self.read), Arc::clone(&self.reading), Arc::clone(&self.generation));
        kernel::work::run(THREAD, move || {
            let environments = http_environments::of(&file, root.as_deref());
            let changed = {
                let mut read = read.lock().unwrap_or_else(PoisonError::into_inner);
                let before = read.get(&folder).map(|r| Arc::clone(&r.environments));
                let changed = before.is_none_or(|b| *b != environments);
                read.insert(
                    folder.clone(),
                    Read { at: Instant::now(), environments: Arc::new(environments) },
                );
                changed
            };
            reading.lock().unwrap_or_else(PoisonError::into_inner).retain(|f| *f != folder);
            if changed {
                generation.fetch_add(1, Ordering::Release);
                workbench_model::app_wide::wake();
            }
        });
    }
}

/// The names a `{{` may take in `text`, with where each comes from: the file's variables,
/// then the environments' (the environments having it; a private one said so).
pub fn variables(text: &str, environments: &Environments) -> Vec<(String, String)> {
    let file = http_requests::parse(text);
    let mut out: Vec<(String, String)> = Vec::new();
    for variable in &file.variables {
        if !out.iter().any(|(n, _)| *n == variable.name) {
            out.push((variable.name.clone(), "@ this file".to_owned()));
        }
    }
    let names: Vec<&str> = environments.names().collect();
    let mut by_name: std::collections::BTreeMap<&str, (Vec<&str>, bool)> = Default::default();
    for environment in names.iter().copied().map(Some).chain([None]) {
        for (name, variable) in environments.variables(environment) {
            let entry = by_name.entry(name).or_default();
            let label = environment.unwrap_or(http_requests::env::SHARED);
            if !entry.0.contains(&label) {
                entry.0.push(label);
            }
            entry.1 |= variable.source.is_secret();
        }
    }
    // The requests named in the file: their responses' variables.
    for request in file.requests.iter().filter_map(|r| r.name.as_ref()) {
        let reference = format!("{request}.response.body.$");
        if !out.iter().any(|(n, _)| *n == reference) {
            out.push((reference, format!("{request}'s response")));
        }
    }
    for (name, (in_environments, secret)) in by_name {
        if out.iter().any(|(n, _)| n == name) {
            continue;
        }
        let mut from = in_environments.join(", ");
        if secret {
            from.push_str(" · private");
        }
        out.push((name.to_owned(), from));
    }
    out
}

impl DocumentAssist for Completion {
    fn id(&self) -> &'static str {
        "http-requests"
    }

    fn languages(&self) -> &'static [&'static str] {
        &[crate::ID]
    }

    fn triggers(&self) -> &'static str {
        "{@:"
    }

    fn complete(&self, at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        let (place, replace) = crate::complete::place(at.text, at.offset)?;
        // A request's response (`login.response.…`): the runner knows them, not the text.
        if place == crate::complete::Place::Variable
            && at.text[replace.clone()].contains(".response")
        {
            return None;
        }
        let names = match place {
            crate::complete::Place::Variable => {
                variables(at.text, &self.environments(at.path, at.root))
            }
            _ => Vec::new(),
        };
        let items = crate::complete::items(&place, &names);
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }

    fn hover(&self, at: &AssistContext<'_>) -> Option<code_intelligence::HoverInfo> {
        let (range, name) = crate::hover::variable_at(at.text, at.offset)?;
        let environments = self.environments(at.path, at.root);
        Some(crate::hover::card(at.text, name, range, &environments))
    }

    fn generation(&self, _project: Option<kernel::ProjectId>) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

#[cfg(test)]
#[path = "../tests/unit/assist.rs"]
mod tests;
