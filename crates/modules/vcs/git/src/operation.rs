//! The operation in progress (a rebase, a merge, a cherry-pick… started here or by another
//! program: SourceTree, the terminal): continued once its conflicts are resolved, the commit
//! it stopped on skipped, or given up (asked first: it can't be undone). From the Git menu
//! or the Conflicts tab's header; run in the background, one at a time per repository, how
//! it went told in a notification.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use i18n::{t, tr};
use version_control::application::{ProceedError, proceed};
use version_control::{Operation, OperationKind, OperationStep, Proceeded};
use workbench_model::{CommandHandler, Completion, Notification, NotificationKind, WorkbenchHost};

/// The operation in progress in this project's repository, as last read (with its changes).
pub(crate) fn in_progress(host: &dyn WorkbenchHost) -> Option<Operation> {
    host.repository()?;
    crate::status::changes(host.project_id()?)?.operation()
}

/// What's in progress, for the user: `Rebase · feature · 3/7`.
pub(crate) fn operation_text(operation: &Operation) -> String {
    let what = t(match operation.kind {
        OperationKind::Rebase => "git.op_rebase",
        OperationKind::Merge => "git.op_merge",
        OperationKind::CherryPick => "git.op_cherry_pick",
        OperationKind::Revert => "git.op_revert",
        OperationKind::Bisect => "git.op_bisect",
        OperationKind::ApplyMailbox => "git.op_apply",
    });
    let mut parts = vec![what];
    parts.extend(operation.branch.clone());
    parts.extend(operation.step.map(|(step, of)| format!("{step}/{of}")));
    parts.join(" · ")
}

/// The repositories with a step running (a second one waits for it to end).
static RUNNING: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// `root` marked as running while this lives (also when the job is dropped or panics).
struct Running(PathBuf);

impl Running {
    fn start(root: PathBuf) -> Option<Self> {
        let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        // The guard made only once it's taken: dropping one locks the set again.
        if !running.get_or_insert_default().insert(root.clone()) {
            return None;
        }
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

/// A Git menu command doing `step` with the operation in progress.
pub(crate) struct Proceed(pub OperationStep);

impl CommandHandler for Proceed {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        in_progress(host).is_some_and(|op| op.kind.allows(self.0))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(operation) = in_progress(host).filter(|op| op.kind.allows(self.0)) else {
            host.notify(Notification::info(t("git.operation_none")));
            return false;
        };
        let step = self.0;
        if step != OperationStep::Abort {
            return start(host, step, &operation);
        }
        // Giving up throws away the resolutions made so far: asked first.
        let what = operation_text(&operation);
        let question = workbench_model::Question {
            title: tr!("git.abort_title", what = what.as_str()),
            message: t("git.abort_message"),
            icon: "rotate-ccw",
            yes: t("git.abort_yes"),
            danger: true,
            list: Vec::new(),
        };
        host.confirm(
            question,
            Box::new(move |host: &mut dyn WorkbenchHost| {
                start(host, step, &operation);
            }),
        )
    }
}

/// `step` run in the background: a spinner until it ends, then how it went in its place;
/// the changes read again.
fn start(host: &mut dyn WorkbenchHost, step: OperationStep, operation: &Operation) -> bool {
    let (Some(repo), Some(project)) = (host.repository(), host.project_id()) else {
        return false;
    };
    let Some(running) = Running::start(repo.root().to_path_buf()) else {
        host.notify(Notification::info(t("git.operation_busy")));
        return false;
    };
    let what = operation_text(operation);
    let work = Notification::progress(tr!(
        match step {
            OperationStep::Continue => "git.operation_continuing",
            OperationStep::Skip => "git.operation_skipping",
            OperationStep::Abort => "git.operation_aborting",
        },
        what = what.as_str()
    ));
    let id = work.id;
    host.notify(work);
    host.spawn(Box::new(move || {
        let result = proceed(repo.as_ref(), step);
        drop(running);
        Box::new(move |host: &mut dyn WorkbenchHost| {
            if let Some(changes) = crate::status::changes(project) {
                changes.stale();
            }
            host.update_notification(outcome(step, &what, &result).with_id(id));
        }) as Completion
    }));
    true
}

/// What the user reads after `step` with the operation called `what`.
pub(crate) fn outcome(
    step: OperationStep,
    what: &str,
    result: &Result<Proceeded, ProceedError>,
) -> Notification {
    match result {
        Ok(Proceeded::Finished) => Notification::success(tr!(
            if step == OperationStep::Abort {
                "git.operation_aborted"
            } else {
                "git.operation_done"
            },
            what = what
        )),
        Ok(Proceeded::Stopped) => Notification::warning(tr!("git.operation_stopped", what = what))
            .with_action(t("git.conflicts_resolve"), crate::SHOW_CONFLICTS),
        Err(ProceedError::Unresolved(paths)) => {
            Notification::warning(tr!("git.operation_unresolved", count = paths.len()))
                .with_action(t("git.conflicts_resolve"), crate::SHOW_CONFLICTS)
        }
        Err(ProceedError::NothingInProgress | ProceedError::NotAllowed) => {
            Notification::info(t("git.operation_none"))
        }
        Err(ProceedError::Vcs(error)) => {
            Notification::new(NotificationKind::Error, crate::error_message(error))
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/operation.rs"]
mod tests;
