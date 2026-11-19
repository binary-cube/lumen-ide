//! A stash's right-click menu (the commit screen's places): brought back (kept, or dropped
//! after), a branch made from it, its message copied, dropped (asked first). The rows come
//! back as the view's commands, each followed by the row's place.

use i18n::t;
use ui_components::{AppRequest, ViewMenuEntry, ViewerAction};
use version_control::Stash;

const APPLY: &str = "stash.apply:";
const POP: &str = "stash.pop:";
const BRANCH: &str = "stash.branch:";
const COPY: &str = "stash.copy:";
const DROP: &str = "stash.drop:";

/// What runs with the host once a row is chosen.
type HostJob = Box<dyn FnOnce(&mut dyn workbench_model::WorkbenchHost) + Send>;

/// The rows for the stash at row `row`.
pub(crate) fn rows(row: usize) -> Vec<ViewMenuEntry> {
    vec![
        ViewMenuEntry::item(format!("{APPLY}{row}"), t("git.stash_apply")).with_icon("archive-up"),
        ViewMenuEntry::item(format!("{POP}{row}"), t("git.stash_pop")).with_icon("archive-up"),
        ViewMenuEntry::item(format!("{BRANCH}{row}"), t("git.stash_new_branch"))
            .with_icon("git-branch"),
        ViewMenuEntry::Separator,
        ViewMenuEntry::item(format!("{COPY}{row}"), t("git.copy_message")).with_icon("copy"),
        ViewMenuEntry::Separator,
        ViewMenuEntry::item(format!("{DROP}{row}"), t("git.stash_drop_row")).with_icon("trash"),
    ]
}

/// What row command `id` asks, `stash` telling which stash a row is: `None` if it isn't one
/// of these rows; `Some(None)` if its row is no stash any more.
pub(crate) fn chosen(
    id: &str,
    stash: impl Fn(usize) -> Option<Stash>,
) -> Option<Option<ViewerAction>> {
    let at = |prefix: &str| id.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok());
    let with_host = |job: HostJob| ViewerAction::WithHost(ui_components::WithHost::once(job));
    let (row, kind) = [APPLY, POP, BRANCH, COPY, DROP]
        .into_iter()
        .find_map(|prefix| at(prefix).map(|row| (row, prefix)))?;
    let Some(stash) = stash(row) else { return Some(None) };
    Some(Some(match kind {
        APPLY | POP => {
            let pop = kind == POP;
            with_host(Box::new(move |host| {
                if let Some(repo) = host.repository() {
                    crate::stash::apply(host, repo, stash.id, pop);
                }
            }))
        }
        BRANCH => with_host(Box::new(move |host| {
            crate::ref_dialog::stash_branch(host, stash);
        })),
        COPY => ViewerAction::Request(AppRequest::CopyText(stash.message)),
        _ => with_host(Box::new(move |host| {
            if let Some(repo) = host.repository() {
                crate::stash::drop_asked(host, repo, stash);
            }
        })),
    }))
}
