//! Which files written by others can't change what `git status` shows: those the
//! repository's `.gitignore` files on their way (and `.git/info/exclude`) ignore — an app's
//! logs, a framework's cache, a watch build's output, written all the time. Their writes ask
//! for no read (each read is a `git status` process). The rules files are read once each and
//! again when their date changes; read on the thread the event comes on, never the UI's.
//!
//! A file both tracked and ignored (rare) edited by another program shows at the next read
//! asked for anything else; the editor's own saves always ask for one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use workspace::ignore::IgnoreRules;

/// More paths than this in one event: read anyway (an install, a checkout: not a log).
const MOST: usize = 64;

/// A rules file as read: its date then (`None`: not there) and its text.
type Read = (Option<SystemTime>, Arc<str>);

/// The rules files read (`.gitignore`, `info/exclude`).
#[derive(Debug, Default)]
pub(crate) struct Ignored {
    files: Mutex<HashMap<PathBuf, Read>>,
}

impl Ignored {
    /// Every one of `paths` is ignored (none: `false`).
    pub(crate) fn all(&self, paths: &[PathBuf]) -> bool {
        !paths.is_empty() && paths.len() <= MOST && paths.iter().all(|p| self.is_ignored(p))
    }

    fn is_ignored(&self, path: &Path) -> bool {
        // A rules file itself changing changes what's ignored.
        if path.file_name().is_some_and(|n| n == ".gitignore") {
            return false;
        }
        // The repository: the nearest folder above with a `.git`.
        let mut dirs = Vec::new();
        let mut dir = path.parent();
        let root = loop {
            let Some(d) = dir else { return false };
            dirs.push(d);
            if d.join(".git").exists() {
                break d;
            }
            dir = d.parent();
        };
        let Some(relative) = relative(path, root) else { return false };
        let mut rules = IgnoreRules::default();
        rules.add("", &self.text(&root.join(".git").join("info").join("exclude")));
        for d in dirs.iter().rev() {
            let base = relative_dir(d, root);
            rules.add(&base, &self.text(&d.join(".gitignore")));
        }
        if rules.is_empty() {
            return false;
        }
        // Under an ignored folder, or ignored itself.
        let parts: Vec<&str> = relative.split('/').collect();
        (1..parts.len()).any(|n| rules.is_ignored(&parts[..n].join("/"), true))
            || rules.is_ignored(&relative, false)
    }

    /// A rules file's text, as last read while its date stays (none: empty).
    fn text(&self, file: &Path) -> Arc<str> {
        let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok();
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((date, text)) = files.get(file)
            && *date == modified
        {
            return Arc::clone(text);
        }
        let text: Arc<str> = match modified {
            Some(_) => std::fs::read_to_string(file).unwrap_or_default().into(),
            None => Arc::from(""),
        };
        files.insert(file.to_path_buf(), (modified, Arc::clone(&text)));
        text
    }
}

/// `path` under `root`, `/`-separated.
fn relative(path: &Path, root: &Path) -> Option<String> {
    let under = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = under.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn relative_dir(dir: &Path, root: &Path) -> String {
    relative(dir, root).unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/unit/ignored.rs"]
mod tests;
