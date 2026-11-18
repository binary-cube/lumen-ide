//! The Push… dialog (a module modal, closed only by ×, Esc or its buttons): the remote, which
//! local branches go under which names there and which of them track it, the tags, a force
//! push. What it needs is read in the background before it opens; the push itself runs like
//! the Git menu's, its outcome told in the status bar.

mod form;
mod view;

use std::path::Path;
use std::sync::Arc;

use i18n::t;
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use version_control::{Remote, Repository, SyncState, VcsError};
use workbench_model::{CommandHandler, Completion, Notification, WorkbenchHost};

pub(crate) use form::PushForm;
use view::PushDialog;

/// The extension of the dialog's view.
pub const EXTENSION: &str = "view-git-push";

/// What the dialog starts from, when it's asked for from a branch's menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushPreset {
    pub branch: Option<String>,
    pub remote: Option<String>,
}

/// What the dialog is opened with.
#[derive(Clone)]
pub struct PushRequest {
    pub repo: Arc<dyn Repository>,
    pub remotes: Vec<Remote>,
    pub branches: Vec<String>,
    /// The branches tracking another one.
    pub states: Vec<SyncState>,
    /// The branch checked at first, and the remote chosen, if asked for.
    pub branch: Option<String>,
    pub remote: Option<String>,
}

#[derive(Debug)]
pub(crate) struct PushDialogProvider;

impl ViewerProvider for PushDialogProvider {
    fn id(&self) -> &'static str {
        "git.push-dialog"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(PushDialog::default()))
    }
}

/// Reads what the dialog shows in the background, then opens it (no remote: says so).
pub(crate) fn open(host: &mut dyn WorkbenchHost, preset: PushPreset) -> bool {
    let Some(repo) = host.repository() else { return false };
    host.spawn(Box::new(move || {
        let remotes = repo.remotes();
        let branches = repo.branches();
        let states = repo.branch_states();
        let branch = preset.branch.or_else(|| repo.branch());
        let request =
            PushRequest { repo, remotes, branches, states, branch, remote: preset.remote };
        Box::new(move |host: &mut dyn WorkbenchHost| {
            if request.remotes.is_empty() {
                host.notify(Notification::error(crate::error_message(&VcsError::NoRemote)));
            } else {
                host.open_modal(&t("git.push_title"), EXTENSION, &request);
            }
        }) as Completion
    }));
    true
}

/// The Git menu's Push…
pub(crate) struct OpenPush;

impl CommandHandler for OpenPush {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        open(host, PushPreset::default())
    }
}
