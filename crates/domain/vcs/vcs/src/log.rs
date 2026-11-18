//! The repository's history as a log: commits with their parents and the references
//! pointing at them, a commit's details (message, changed files), and how the current
//! branch stands against its upstream.

use crate::{Commit, FileState};

/// What a reference is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefKind {
    /// `HEAD` (the checked-out commit).
    Head,
    /// A local branch.
    Branch,
    /// A remote-tracking branch (`origin/main`).
    Remote,
    Tag,
}

/// A reference shown next to its commit (`main`, `origin/main`, `v1.2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

/// A commit in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub commit: Commit,
    /// The parents' ids (the first one is the branch it continues).
    pub parents: Vec<String>,
    pub refs: Vec<RefLabel>,
}

/// A file a commit changed (paths relative to the root, with `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// Where it was before, for a rename.
    pub old_path: Option<String>,
    pub state: FileState,
}

/// Everything about one commit, for its details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    pub commit: Commit,
    pub email: String,
    /// The whole message.
    pub message: String,
    pub parents: Vec<String>,
    /// Against the first parent (everything, for the first commit).
    pub files: Vec<FileChange>,
}

/// The current branch against the one it tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    pub branch: String,
    /// `origin/main`; `None` when the branch tracks nothing (a push publishes it).
    pub upstream: Option<String>,
    /// Commits here and not there (to push), and there and not here (to pull).
    pub ahead: usize,
    pub behind: usize,
}
