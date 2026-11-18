//! A branch's or a tag's right-click menu, the same wherever a reference is listed (the log's
//! branches panel, the commit screen's places): a branch from it, renamed (a local one), a
//! tag on it, and a local branch's pushes (to what it tracks, to a remote chosen). The rows'
//! commands carry the row they were opened on (`branch.rename:3`); the view tells which
//! reference that row is.

use i18n::{t, tr};
use ui_components::{ViewMenuEntry, ViewerAction};

use crate::ref_dialog::{self, Target};

/// The commands of the rows, each followed by the row's place.
pub(crate) const NEW_BRANCH: &str = "branch.new-branch:";
pub(crate) const RENAME: &str = "branch.rename:";
pub(crate) const NEW_TAG: &str = "branch.new-tag:";
pub(crate) const PUSH: &str = "branch.push:";
/// `branch.push-to:<row>:<remote>`.
pub(crate) const PUSH_TO: &str = "branch.push-to:";

/// The reference a menu is opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuRef {
    /// Its name (`HEAD` for the detached head).
    pub name: String,
    /// A local branch (renamed, pushed).
    pub local: bool,
    /// What a local branch tracks (`origin/main`), if anything.
    pub upstream: Option<String>,
}

impl MenuRef {
    fn target(&self) -> Target {
        Target::Reference { name: self.name.clone(), local_branch: self.local }
    }
}

/// The rows for reference `on` (at row `row`), `remotes` the repository's.
pub(crate) fn rows(row: usize, on: &MenuRef, remotes: &[String]) -> Vec<ViewMenuEntry> {
    let mut rows = vec![
        ViewMenuEntry::item(format!("{NEW_BRANCH}{row}"), t("git.new_branch_from_here"))
            .with_icon("git-branch"),
    ];
    if on.local {
        rows.push(
            ViewMenuEntry::item(format!("{RENAME}{row}"), t("git.rename_branch"))
                .with_icon("file-pen"),
        );
    }
    rows.push(
        ViewMenuEntry::item(format!("{NEW_TAG}{row}"), t("git.new_tag_here")).with_icon("tag"),
    );
    if on.local && !remotes.is_empty() {
        rows.push(ViewMenuEntry::Separator);
        if let Some(upstream) = &on.upstream {
            let label = tr!("git.push_to_tracked", upstream = upstream.as_str());
            rows.push(ViewMenuEntry::item(format!("{PUSH}{row}"), label).with_icon("arrow-up"));
        }
        let entries = remotes
            .iter()
            .enumerate()
            .map(|(at, remote)| ViewMenuEntry::item(format!("{PUSH_TO}{row}:{at}"), remote.clone()))
            .collect();
        rows.push(
            ViewMenuEntry::Submenu { label: t("git.push_to_remote"), icon: None, entries }
                .with_icon("arrow-up"),
        );
    }
    rows
}

/// What row command `id` asks, `reference` telling which reference a row is: `None` if it
/// isn't one of these rows; `Some(None)` if its row is no reference any more.
pub(crate) fn chosen(
    id: &str,
    reference: impl Fn(usize) -> Option<MenuRef>,
    remotes: &[String],
) -> Option<Option<ViewerAction>> {
    let row = |prefix: &str| id.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok());
    type Opens = fn(&mut dyn workbench_model::WorkbenchHost, Option<Target>) -> bool;
    let dialog = |row: usize, open: Opens| {
        reference(row).map(|on| {
            let target = on.target();
            ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
                open(host, Some(target));
            }))
        })
    };
    if let Some(i) = row(NEW_BRANCH) {
        return Some(dialog(i, ref_dialog::new_branch));
    }
    if let Some(i) = row(RENAME) {
        return Some(dialog(i, ref_dialog::rename_branch));
    }
    if let Some(i) = row(NEW_TAG) {
        return Some(dialog(i, ref_dialog::new_tag));
    }
    let (i, remote) = if let Some(i) = row(PUSH) {
        (i, None)
    } else {
        let (i, at) = id.strip_prefix(PUSH_TO)?.split_once(':')?;
        (i.parse::<usize>().ok()?, Some(at.parse::<usize>().ok()?))
    };
    let Some(on) = reference(i).filter(|on| on.local) else { return Some(None) };
    let preset = crate::push_dialog::PushPreset {
        branch: Some(on.name),
        remote: remote.and_then(|at| remotes.get(at).cloned()),
    };
    Some(Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
        crate::push_dialog::open(host, preset);
    }))))
}
