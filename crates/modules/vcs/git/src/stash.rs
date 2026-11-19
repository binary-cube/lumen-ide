//! Stashes: the changes put aside (all, what's staged, chosen files: the Stash dialog) and
//! brought back, dropped or turned into a branch (the places' stash rows and their menu).
//! The work runs on a worker; a stash is named by its commit, found again when it runs.

use std::sync::Arc;

use i18n::{t, tr};
use version_control::{Repository, Stash};
use workbench_model::{Completion, Notification, Question, WorkbenchHost};

/// After a stash's work: every view of the references and the changes reads again.
fn moved(repo: &dyn Repository) {
    crate::refs_store::moved(repo.root());
}

/// Stash `id` applied (`pop`: dropped after; git keeps it when it conflicted).
pub(crate) fn apply(
    host: &mut dyn WorkbenchHost,
    repo: Arc<dyn Repository>,
    id: String,
    pop: bool,
) {
    host.spawn(Box::new(move || {
        let result = repo.stash_apply(&id, pop);
        moved(repo.as_ref());
        Box::new(move |host: &mut dyn WorkbenchHost| match result {
            Ok(()) => {
                let done = if pop { t("git.stash_popped") } else { t("git.stash_applied") };
                host.notify(Notification::success(done));
            }
            Err(e) => {
                let message = crate::error_message(&e);
                let conflicted = message.contains("CONFLICT") || message.contains("onflict");
                let mut note =
                    Notification::error(t("git.stash_apply_failed")).with_detail(message);
                if conflicted {
                    note = Notification::warning(t("git.stash_conflicts"))
                        .with_detail(if pop { t("git.stash_kept") } else { String::new() })
                        .with_action(t("git.resolve_conflicts"), crate::SHOW_CONFLICTS);
                }
                host.notify(note);
            }
        }) as Completion
    }));
}

/// Stash `stash` dropped, once confirmed (it can't be brought back).
pub(crate) fn drop_asked(host: &mut dyn WorkbenchHost, repo: Arc<dyn Repository>, stash: Stash) {
    let question = Question {
        title: tr!("git.stash_drop_title", name = stash.message.as_str()),
        message: t("git.stash_drop_message"),
        icon: "trash",
        yes: t("git.stash_drop"),
        danger: true,
        list: Vec::new(),
    };
    let go = move |host: &mut dyn WorkbenchHost| {
        host.spawn(Box::new(move || {
            let result = repo.stash_drop(&stash.id);
            moved(repo.as_ref());
            Box::new(move |host: &mut dyn WorkbenchHost| match result {
                Ok(()) => host.notify(Notification::success(t("git.stash_dropped"))),
                Err(e) => host.notify(
                    Notification::error(t("git.stash_drop_failed"))
                        .with_detail(crate::error_message(&e)),
                ),
            }) as Completion
        }));
    };
    host.confirm(question, Box::new(go));
}
