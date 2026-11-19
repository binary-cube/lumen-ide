//! Switching branches (a branch's double click, its menu's Switch to, the top bar's list):
//! the files open with unsaved changes asked about first, a tag or a commit only after
//! saying `HEAD` will be detached; then, on a worker, what the switch would do to the files
//! being worked on — free (they stay as they are and the switch runs at once), in its way
//! (a dialog says which and offers to put them aside, throw them away or commit first), or
//! waiting for an operation in progress. A remote branch is switched to through the local
//! one tracking it, else a new one made to track it.
//!
//! The work put aside to switch is remembered per repository: the notification after the
//! switch brings it back with one click.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use i18n::{t, tr};
use version_control::{
    FileState, Operation, RefKind, RemoteSwitch, Repository, StashOptions, SwitchPlan,
    SwitchTarget, VcsError, is_untracked, remote_switch, switch_plan,
};
use workbench_model::{CommandHandler, Completion, Notification, Question, WorkbenchHost};

/// What was asked to switch to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wanted {
    /// A local branch.
    Branch(String),
    /// A remote branch (`origin/feature`).
    Remote(String),
    /// A tag.
    Tag(String),
    /// A commit: its id and what's shown of it.
    Commit { id: String, short: String },
}

impl Wanted {
    /// A reference of the log's or the places' as what's switched to (`None`: `HEAD`).
    pub(crate) fn of(name: &str, kind: RefKind) -> Option<Self> {
        match kind {
            RefKind::Branch => Some(Self::Branch(name.to_owned())),
            RefKind::Remote => Some(Self::Remote(name.to_owned())),
            RefKind::Tag => Some(Self::Tag(name.to_owned())),
            _ => None,
        }
    }

    fn detaches(&self) -> bool {
        matches!(self, Self::Tag(_) | Self::Commit { .. })
    }

    fn label(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Remote(name) | Self::Tag(name) => name,
            Self::Commit { short, .. } => short,
        }
    }
}

/// A file in a switch's way: its path and its state (staged first, else in the tree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InTheWay {
    pub path: String,
    pub state: FileState,
}

/// What the check found, for the UI thread.
enum Found {
    /// Already there.
    Already(String),
    /// Switched (to what's said), `carried` files staying as they were.
    Switched {
        label: String,
        carried: usize,
    },
    /// In the way.
    Blocked {
        target: SwitchTarget,
        label: String,
        files: Vec<InTheWay>,
    },
    /// An operation in progress.
    Busy(Operation),
    /// The remote branch's name is a local branch tracking something else: a name is asked.
    NameTaken(String),
    Failed(String),
}

/// Starts switching to `wanted` (see the module's doc). `false`: no repository.
pub(crate) fn switch(host: &mut dyn WorkbenchHost, wanted: Wanted) -> bool {
    let Some(repo) = host.repository() else { return false };
    let root = repo.root().to_path_buf();
    let unsaved: Vec<String> = host
        .unsaved_files()
        .iter()
        .filter_map(|p| version_control::relative_path(&root, p))
        .collect();
    let then = move |host: &mut dyn WorkbenchHost| detach_asked(host, repo, wanted);
    if unsaved.is_empty() {
        then(host);
        return true;
    }
    let question = Question {
        title: t("git.switch_unsaved_title"),
        message: t("git.switch_unsaved_message"),
        icon: "file-pen",
        yes: t("git.switch_anyway"),
        danger: false,
        list: unsaved,
    };
    if !host.confirm(question, Box::new(then)) {
        host.notify(Notification::warning(t("git.switch_unsaved_title")));
    }
    true
}

/// A tag or a commit: said first that `HEAD` will be detached.
fn detach_asked(host: &mut dyn WorkbenchHost, repo: Arc<dyn Repository>, wanted: Wanted) {
    if !wanted.detaches() {
        check(host, repo, wanted);
        return;
    }
    let question = Question {
        title: tr!("git.switch_detached_title", name = wanted.label()),
        message: t("git.switch_detached_message"),
        icon: "git-commit",
        yes: t("git.switch_detached_go"),
        danger: false,
        list: Vec::new(),
    };
    let go = move |host: &mut dyn WorkbenchHost| check(host, repo, wanted);
    host.confirm(question, Box::new(go));
}

/// The check on a worker, then what it found shown.
fn check(host: &mut dyn WorkbenchHost, repo: Arc<dyn Repository>, wanted: Wanted) {
    host.spawn(Box::new(move || {
        let found = find(repo.as_ref(), &wanted);
        if matches!(found, Found::Switched { .. }) {
            crate::refs_store::moved(repo.root());
        }
        Box::new(move |host: &mut dyn WorkbenchHost| show(host, repo, found)) as Completion
    }));
}

/// What switching to `wanted` means here, and the switch itself when it's free.
fn find(repo: &dyn Repository, wanted: &Wanted) -> Found {
    let current = repo.branch().filter(|_| !repo.detached());
    let target = match wanted {
        Wanted::Branch(name) if current.as_deref() == Some(name) => {
            return Found::Already(name.clone());
        }
        Wanted::Branch(name) => SwitchTarget::Branch(name.clone()),
        Wanted::Remote(remote) => {
            let locals: Vec<String> = repo
                .refs()
                .into_iter()
                .filter(|r| r.kind == RefKind::Branch)
                .map(|r| r.name)
                .collect();
            match remote_switch(remote, &repo.branch_states(), &locals) {
                RemoteSwitch::Existing(local) if current.as_deref() == Some(&local) => {
                    return Found::Already(local);
                }
                RemoteSwitch::Existing(local) => SwitchTarget::Branch(local),
                RemoteSwitch::New { taken: true, .. } => return Found::NameTaken(remote.clone()),
                RemoteSwitch::New { name, .. } => {
                    SwitchTarget::NewBranch { name, start: remote.clone(), track: true }
                }
            }
        }
        Wanted::Tag(name) => SwitchTarget::Detached(name.clone()),
        Wanted::Commit { id, .. } => SwitchTarget::Detached(id.clone()),
    };
    let label = match &target {
        SwitchTarget::Branch(name) | SwitchTarget::NewBranch { name, .. } => name.clone(),
        SwitchTarget::Detached(_) => wanted.label().to_owned(),
    };
    match plan(repo, &target) {
        Ok(SwitchPlan::Free { carried }) => match repo.switch(&target, false) {
            Ok(()) => Found::Switched { label, carried },
            // Something changed on disk meanwhile: git's own refusal, said as it is.
            Err(e) => Found::Failed(crate::error_message(&e)),
        },
        Ok(SwitchPlan::Blocked { files }) => {
            let files = in_the_way(repo, files);
            Found::Blocked { target, label, files }
        }
        Ok(SwitchPlan::Busy(operation)) => Found::Busy(operation),
        Err(e) => Found::Failed(crate::error_message(&e)),
    }
}

/// What a switch to `target` would do to the files being worked on.
pub(crate) fn plan(repo: &dyn Repository, target: &SwitchTarget) -> Result<SwitchPlan, VcsError> {
    let operation = repo.operation();
    let dirty = repo.staged_files()?;
    if operation.is_some() || dirty.is_empty() {
        return Ok(switch_plan(operation, &dirty, &[], &[]));
    }
    let (untracked, tracked): (Vec<_>, Vec<_>) = dirty.iter().partition(|f| is_untracked(f));
    let paths = |files: &[&version_control::StagedFile]| -> Vec<String> {
        files.iter().map(|f| f.path.clone()).collect()
    };
    let differs = repo.changed_between(target.revision(), &paths(&tracked))?;
    let present = if untracked.is_empty() {
        Vec::new()
    } else {
        repo.present_in(target.revision(), &paths(&untracked))?
    };
    Ok(switch_plan(None, &dirty, &differs, &present))
}

/// The files in the way, with their states.
fn in_the_way(repo: &dyn Repository, files: Vec<String>) -> Vec<InTheWay> {
    let states = repo.staged_files().unwrap_or_default();
    files
        .into_iter()
        .map(|path| {
            let state = states
                .iter()
                .find(|f| f.path == path)
                .and_then(|f| f.staged.or(f.unstaged))
                .unwrap_or(FileState::Modified);
            InTheWay { path, state }
        })
        .collect()
}

fn show(host: &mut dyn WorkbenchHost, repo: Arc<dyn Repository>, found: Found) {
    match found {
        Found::Already(name) => {
            host.notify(Notification::info(tr!("git.switch_already", name = name.as_str())));
        }
        Found::Switched { label, carried } => host.notify(switched(&label, carried, false)),
        Found::Blocked { target, label, files } => {
            let request = crate::switch_dialog::SwitchRequest { repo, target, label, files };
            host.open_modal(
                &t("git.switch_blocked_title"),
                crate::switch_dialog::EXTENSION,
                &request,
            );
        }
        Found::Busy(operation) => {
            let what = crate::operation::operation_text(&operation);
            host.notify(
                Notification::warning(tr!("git.switch_busy", operation = what.as_str()))
                    .with_detail(t("git.switch_busy_detail"))
                    .with_action(t("git.resolve_conflicts"), crate::SHOW_CONFLICTS),
            );
        }
        Found::NameTaken(remote) => {
            let target = crate::ref_dialog::Target::Reference { name: remote, local_branch: false };
            crate::ref_dialog::new_branch(host, Some(target));
        }
        Found::Failed(message) => {
            host.notify(Notification::error(t("git.switch_failed")).with_detail(message));
        }
    }
}

/// What's said once switched to `label`: the files that stayed, or the work put aside (with
/// its button bringing it back).
pub(crate) fn switched(label: &str, carried: usize, stashed: bool) -> Notification {
    let note = Notification::success(tr!("git.switched", name = label));
    if stashed {
        return note
            .with_detail(t("git.switched_stashed"))
            .with_action(t("git.stash_bring_back"), crate::POP_SWITCH_STASH);
    }
    if carried > 0 {
        return note.with_detail(tr!("git.switched_carried", count = carried));
    }
    note
}

/// The stash made to switch, per repository: what "bring it back" applies.
static PUT_ASIDE: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// The work in `root` put aside to switch (its stash's commit).
pub(crate) fn remember_put_aside(root: &Path, id: String) {
    let mut held = PUT_ASIDE.lock().unwrap_or_else(PoisonError::into_inner);
    held.get_or_insert_with(HashMap::new).insert(root.to_path_buf(), id);
}

fn take_put_aside(root: &Path) -> Option<String> {
    let mut held = PUT_ASIDE.lock().unwrap_or_else(PoisonError::into_inner);
    held.as_mut()?.remove(root)
}

/// What the work put aside to switch is stashed with.
pub(crate) fn put_aside_options(label: &str) -> StashOptions {
    StashOptions {
        message: Some(tr!("git.stash_before_switch", name = label)),
        untracked: true,
        ..Default::default()
    }
}

/// The notification's "bring it back": the stash made to switch applied and dropped.
pub(crate) struct PopSwitchStash;

impl CommandHandler for PopSwitchStash {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(repo) = host.repository() else { return false };
        let Some(id) = take_put_aside(repo.root()) else {
            host.notify(Notification::info(t("git.stash_gone")));
            return false;
        };
        crate::stash::apply(host, repo, id, true);
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/switch.rs"]
mod tests;
