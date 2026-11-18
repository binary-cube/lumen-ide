//! A change in the editor (the lines a gutter bar marks): revert it to the committed lines,
//! or compare the file with its last commit — from the Git menu, a shortcut or a click on
//! the bar (a small menu with both).

use i18n::{t, tr};
use version_control::Revision;
use version_control::application::revert_change;
use workbench_model::{ChoiceItem, ChoiceList, CommandHandler, WorkbenchHost};

/// Reverts the change at the cursor (one undo step: ⌘Z brings it back).
pub(crate) struct RevertChange;

impl CommandHandler for RevertChange {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some() && crate::decorations::committed_text(host).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        revert_at_cursor(host)
    }
}

pub(crate) fn revert_at_cursor(host: &mut dyn WorkbenchHost) -> bool {
    let (Some(file), Some(base)) = (host.active_file(), crate::decorations::committed_text(host))
    else {
        host.notify(workbench_model::Notification::info(t("git.no_committed_version")));
        return false;
    };
    let Some(revert) = revert_change(&base, &file.text, file.line) else {
        host.notify(workbench_model::Notification::info(t("git.no_change_here")));
        return false;
    };
    let reverted = host.edit_active(revert.range, &revert.text);
    if reverted {
        host.notify(workbench_model::Notification::success(t("git.change_reverted")));
    }
    reverted
}

/// The change at the cursor's actions, as a small menu.
pub(crate) struct ChangeActions;

impl CommandHandler for ChangeActions {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some() && host.active_file().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(file) = host.active_file() else { return false };
        let can_revert = crate::decorations::committed_text(host)
            .and_then(|base| revert_change(&base, &file.text, file.line))
            .map(|r| r.change);
        let mut items = Vec::new();
        let mut actions: Vec<fn(&mut dyn WorkbenchHost)> = Vec::new();
        if can_revert.is_some() {
            items.push(ChoiceItem {
                icon: "rotate-ccw",
                label: t("git.revert_change"),
                detail: t("git.revert_detail"),
            });
            actions.push(|host| {
                revert_at_cursor(host);
            });
        }
        if workbench_model::presence::compare() {
            items.push(ChoiceItem {
                icon: "compare",
                label: t("git.compare_head_action"),
                detail: t("git.compare_head_detail"),
            });
            actions.push(|host| {
                crate::compare(host, &Revision::Head, "HEAD");
            });
        }
        let subject = match &can_revert {
            Some(change) if change.lines.len() > 1 => {
                tr!("git.lines", from = change.lines.start + 1, to = change.lines.end)
            }
            Some(change) => tr!("git.line", line = change.lines.start + 1),
            None => crate::file_name(host),
        };
        let list = ChoiceList {
            title: t("git.change_title"),
            subject,
            icon: "git-commit",
            action: t("git.run_action"),
            items,
        };
        host.choose(
            list,
            Box::new(move |i, host| {
                if let Some(action) = actions.get(i) {
                    action(host);
                }
            }),
        )
    }
}

#[cfg(test)]
#[path = "../tests/unit/changes.rs"]
mod tests;
