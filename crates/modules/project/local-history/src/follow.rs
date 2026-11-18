//! The window's fixed tab, "Current file": the history of whichever file is active in the
//! editor, followed as the tabs change — but only while that tab is seen. It says so as it's
//! drawn ([`seen`]: a view is drawn only while it shows); the project's service asks the
//! active file at its poll only then ([`poll`]), and the tab reads it ([`file`]). Hidden, it's
//! drawn no more: nothing is asked, nothing read.
//!
//! A tab that isn't a project's file coming to the front (a version opened from the list
//! to compare, a tool's view) leaves the file as it was: its history stays shown, another
//! version can be opened from it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long after it was last drawn the tab counts as seen (a frame or two: the poll comes
/// before the frame).
const SEEN_FOR: Duration = Duration::from_millis(500);

/// A project's fixed tab: when it was last drawn, the active file as last asked, and a count
/// of its changes.
#[derive(Debug, Default)]
struct Watch {
    seen: Option<Instant>,
    file: Option<PathBuf>,
    generation: u64,
}

static WATCHES: Mutex<Option<HashMap<kernel::ProjectId, Watch>>> = Mutex::new(None);

fn with<R>(project: kernel::ProjectId, f: impl FnOnce(&mut Watch) -> R) -> R {
    let mut all = WATCHES.lock().unwrap_or_else(PoisonError::into_inner);
    f(all.get_or_insert_with(HashMap::new).entry(project).or_default())
}

/// The project's fixed tab was drawn (it shows).
pub(crate) fn seen(project: kernel::ProjectId) {
    with(project, |w| w.seen = Some(Instant::now()));
}

/// The active file as last asked, and its count of changes.
pub(crate) fn file(project: kernel::ProjectId) -> (u64, Option<PathBuf>) {
    with(project, |w| (w.generation, w.file.clone()))
}

/// At the project's poll: while its fixed tab shows, the active file asked (`active`) and
/// kept if it changed — none (a view's tab in front) changes nothing. `true` if it did
/// (the tab drawn again).
pub(crate) fn poll(project: kernel::ProjectId, active: impl FnOnce() -> Option<PathBuf>) -> bool {
    let watching = {
        let all = WATCHES.lock().unwrap_or_else(PoisonError::into_inner);
        all.as_ref()
            .and_then(|a| a.get(&project))
            .and_then(|w| w.seen)
            .is_some_and(|at| at.elapsed() < SEEN_FOR)
    };
    if !watching {
        return false;
    }
    let Some(path) = active() else { return false };
    let file = Some(path);
    with(project, |w| {
        if w.file == file {
            return false;
        }
        w.file = file;
        w.generation += 1;
        true
    })
}

/// The project closed: its tab's state let go.
pub(crate) fn forget(project: kernel::ProjectId) {
    if let Some(all) = WATCHES.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        all.remove(&project);
    }
}

#[cfg(test)]
#[path = "../tests/unit/follow.rs"]
mod tests;
