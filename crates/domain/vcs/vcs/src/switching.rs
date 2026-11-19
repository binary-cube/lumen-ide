//! Switching branches and putting work aside: what a switch would do to the files being
//! worked on (it's free, or some stand in its way), and the options a stash is made with.
//! Pure: the repository gives the facts (what's changed, what differs on the target), these
//! rules decide.

use std::collections::HashSet;

use crate::{FileState, Operation, StagedFile, SyncState};

/// What to switch to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchTarget {
    /// A local branch.
    Branch(String),
    /// A new local branch `name` made at `start` (a remote branch: tracking it when
    /// `track`).
    NewBranch { name: String, start: String, track: bool },
    /// A tag or a commit: no branch (a detached `HEAD`).
    Detached(String),
}

impl SwitchTarget {
    /// The revision its files come from (to compare with `HEAD`).
    pub fn revision(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Detached(name) => name,
            Self::NewBranch { start, .. } => start,
        }
    }
}

/// What a switch would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchPlan {
    /// Nothing in its way: the files being worked on (`carried` of them) stay as they are.
    Free { carried: usize },
    /// These files' changes would be lost (they differ on the target too; an untracked one
    /// exists there).
    Blocked { files: Vec<String> },
    /// An operation is in progress (a merge, a rebase): it's finished or given up first.
    Busy(Operation),
}

/// What a switch would do, from:
/// - `operation`: the operation in progress, if any;
/// - `dirty`: the files being worked on (staged or not);
/// - `differs`: those of them whose committed content differs between `HEAD` and the target;
/// - `present`: those of the untracked ones that exist on the target.
pub fn switch_plan(
    operation: Option<Operation>,
    dirty: &[StagedFile],
    differs: &[String],
    present: &[String],
) -> SwitchPlan {
    if let Some(operation) = operation {
        return SwitchPlan::Busy(operation);
    }
    let differs: HashSet<&str> = differs.iter().map(String::as_str).collect();
    let present: HashSet<&str> = present.iter().map(String::as_str).collect();
    let files: Vec<String> = dirty
        .iter()
        .filter(|f| {
            let path = f.path.as_str();
            if is_untracked(f) { present.contains(path) } else { differs.contains(path) }
        })
        .map(|f| f.path.clone())
        .collect();
    if files.is_empty() {
        SwitchPlan::Free { carried: dirty.len() }
    } else {
        SwitchPlan::Blocked { files }
    }
}

/// Only in the working tree, never added.
pub fn is_untracked(file: &StagedFile) -> bool {
    file.staged.is_none() && file.unstaged == Some(FileState::Untracked)
}

/// What switching to remote branch `remote` (`origin/feature`) does: the local branch
/// already tracking it, else a new one tracking it named as it is on the remote
/// (`feature`) — `taken` when that name is a local branch tracking something else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteSwitch {
    Existing(String),
    New { name: String, taken: bool },
}

/// See [`RemoteSwitch`]: `states` are the local branches with what they track, `locals` all
/// the local branches.
pub fn remote_switch(remote: &str, states: &[SyncState], locals: &[String]) -> RemoteSwitch {
    if let Some(state) = states.iter().find(|s| s.upstream.as_deref() == Some(remote)) {
        return RemoteSwitch::Existing(state.branch.clone());
    }
    let name = remote.split_once('/').map_or(remote, |(_, name)| name).to_owned();
    let taken = locals.contains(&name);
    RemoteSwitch::New { name, taken }
}

/// The branches last checked out, newest first, at most `limit` (without `current`): from
/// the reflog's subjects (`checkout: moving from main to feature`), newest first.
pub fn recent_checkouts(subjects: &str, current: Option<&str>, limit: usize) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in subjects.lines() {
        let Some(moves) = line.strip_prefix("checkout: moving from ") else { continue };
        let Some((from, to)) = moves.split_once(" to ") else { continue };
        for name in [to.trim(), from.trim()] {
            // A commit id (a detached head) isn't a branch.
            let id = name.len() >= 7 && name.chars().all(|c| c.is_ascii_hexdigit());
            if name.is_empty() || id || Some(name) == current || found.iter().any(|f| f == name) {
                continue;
            }
            found.push(name.to_owned());
        }
        if found.len() >= limit {
            break;
        }
    }
    found.truncate(limit);
    found
}

/// How a stash is made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StashOptions {
    /// Its message (`None`: git's own, `WIP on <branch>`).
    pub message: Option<String>,
    /// What's staged stays staged (and in the working tree) too.
    pub keep_index: bool,
    /// The untracked files go with it.
    pub untracked: bool,
    /// Only what's staged goes (git 2.35 and newer).
    pub staged_only: bool,
    /// Only these files go (none: all).
    pub paths: Vec<String>,
}

/// Why a stash's options don't go together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StashConflict {
    /// Only what's staged, and what's staged kept: nothing would go.
    StagedAndKept,
    /// Only what's staged, with the untracked files (never staged).
    StagedAndUntracked,
    /// Only what's staged, of some files only.
    StagedAndPaths,
}

impl StashOptions {
    /// Whether these options go together.
    pub fn check(&self) -> Result<(), StashConflict> {
        if self.staged_only && self.keep_index {
            return Err(StashConflict::StagedAndKept);
        }
        if self.staged_only && self.untracked {
            return Err(StashConflict::StagedAndUntracked);
        }
        if self.staged_only && !self.paths.is_empty() {
            return Err(StashConflict::StagedAndPaths);
        }
        Ok(())
    }
}

/// The stash `id` (its commit) in `stashes`, as listed now: its name may have moved
/// (`stash@{0}` becomes `stash@{1}` when another is made).
pub fn stash_named<'a>(stashes: &'a [crate::Stash], id: &str) -> Option<&'a crate::Stash> {
    stashes.iter().find(|s| !id.is_empty() && s.id == id)
}

#[cfg(test)]
#[path = "../tests/unit/switching.rs"]
mod tests;
