//! Fetch / pull / push: run in the background (the network may take a while; the UI never
//! waits), one at a time, the outcome told in the status bar — what came / went, how the
//! branch stands against its upstream, or what's missing (git not installed, no remote,
//! credentials).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use i18n::{t, tr};
use version_control::application::{SyncOp, sync};
use version_control::{RemoteOutcome, Repository, SyncState, VcsError};
use workbench_model::{CommandHandler, Completion, WorkbenchHost};

/// An operation on a repository's remote running: which, for which project, since when.
#[derive(Debug, Clone, Copy)]
struct Underway {
    op: SyncOp,
    project: Option<kernel::ProjectId>,
    since: Instant,
}

/// The repositories with an operation on their remote running (a second one on the same
/// repository waits for it to end; another project's runs meanwhile).
static RUNNING: Mutex<Option<HashMap<PathBuf, Underway>>> = Mutex::new(None);

/// `root` marked as running while this lives: also when the job is dropped without running
/// (its project closed) or panics.
struct Running(PathBuf);

impl Running {
    /// `None` if `root` already runs.
    fn start(root: PathBuf, op: SyncOp, project: Option<kernel::ProjectId>) -> Option<Self> {
        let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        let running = running.get_or_insert_default();
        if running.contains_key(&root) {
            return None;
        }
        running.insert(root.clone(), Underway { op, project, since: Instant::now() });
        Some(Self(root))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(running) = RUNNING.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            running.remove(&self.0);
        }
    }
}

/// The operations on remotes running now, as work a closing would stop: a fetch only
/// interrupted (it's done again), a pull or a push left half done. None can be stopped: a
/// git stopped midway leaves its `index.lock` (and a half-written repository) behind — they
/// finish on their own.
pub(crate) fn running() -> Vec<workbench_model::RunningWork> {
    let running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(running) = running.as_ref() else { return Vec::new() };
    running
        .iter()
        .map(|(root, underway)| {
            let (key, impact) = match underway.op {
                SyncOp::Fetch => ("git.fetching", workbench_model::StopImpact::Interrupts),
                SyncOp::Pull => ("git.pulling", workbench_model::StopImpact::LosesWork),
                SyncOp::Push => ("git.pushing", workbench_model::StopImpact::LosesWork),
            };
            let mut work = workbench_model::RunningWork::new(
                workbench_model::running::WorkKind::Network,
                t(key),
                crate::MANIFEST.id.as_str(),
                underway.project,
                impact,
            );
            work.detail = root.display().to_string();
            work.since = Some(underway.since);
            work
        })
        .collect()
}

/// A Git menu command running `op`.
pub(crate) struct Sync(pub SyncOp);

impl CommandHandler for Sync {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(repo) = host.repository() else { return false };
        let op = self.0;
        start(host, repo, op, move |repo| sync(repo, op))
    }
}

/// `job`, doing `op` on the repository's remote, run in the background: its progress, then
/// how it went, in the status bar. `false` if the repository's remote is busy.
pub(crate) fn start(
    host: &mut dyn WorkbenchHost,
    repo: Arc<dyn Repository>,
    op: SyncOp,
    job: impl FnOnce(&dyn Repository) -> Result<RemoteOutcome, VcsError> + Send + 'static,
) -> bool {
    let Some(running) = Running::start(repo.root().to_path_buf(), op, host.project_id()) else {
        host.notify(workbench_model::Notification::info(t("git.sync_busy")));
        return false;
    };
    // Work in progress (a spinner) until it ends: then how it went, in its place.
    let work = workbench_model::Notification::progress(t(match op {
        SyncOp::Fetch => "git.fetching",
        SyncOp::Pull => "git.pulling",
        SyncOp::Push => "git.pushing",
    }));
    let id = work.id;
    host.notify(work);
    host.spawn(Box::new(move || {
        let result = job(repo.as_ref());
        // The branches moved (pushed, fetched, pulled): every view of them shows it.
        crate::refs_store::moved(repo.root());
        let state = repo.sync_state();
        drop(running);
        Box::new(move |host: &mut dyn WorkbenchHost| {
            let kind = if result.is_ok() {
                workbench_model::NotificationKind::Success
            } else {
                workbench_model::NotificationKind::Error
            };
            let message = outcome_message(op, &result, state.as_ref());
            host.update_notification(workbench_model::Notification::new(kind, message).with_id(id));
        }) as Completion
    }));
    true
}

/// `↑2 ↓1` (nothing when in step, or without an upstream).
pub fn sync_badge(state: &SyncState) -> String {
    let mut parts = Vec::new();
    if state.ahead > 0 {
        parts.push(format!("↑{}", state.ahead));
    }
    if state.behind > 0 {
        parts.push(format!("↓{}", state.behind));
    }
    parts.join(" ")
}

/// What the user reads after `op`.
pub fn outcome_message(
    op: SyncOp,
    result: &Result<RemoteOutcome, VcsError>,
    state: Option<&SyncState>,
) -> String {
    let outcome = match result {
        Err(error) => return crate::error_message(error),
        Ok(outcome) => outcome,
    };
    let badge = state.map(sync_badge).filter(|b| !b.is_empty());
    let done = match (op, outcome.up_to_date) {
        (SyncOp::Push, true) => t("git.nothing_to_push"),
        (SyncOp::Pull | SyncOp::Fetch, true) => t("git.up_to_date"),
        (SyncOp::Fetch, false) => t("git.fetched"),
        (SyncOp::Pull, false) => tr!("git.pulled", detail = outcome.message.as_str()),
        (SyncOp::Push, false) => tr!("git.pushed", detail = outcome.message.as_str()),
    };
    match badge {
        Some(badge) => format!("{done} · {badge}"),
        None => done,
    }
}

#[cfg(test)]
#[path = "../tests/unit/sync.rs"]
mod tests;
