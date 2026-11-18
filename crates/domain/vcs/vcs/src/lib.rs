//! Version control bounded context: commits, blame, the lines changed since the last
//! commit, and the port to a repository (implemented with git in infrastructure,
//! `git`).

use std::ops::Range;
use std::path::{Path, PathBuf};

use kernel::DomainEvent;

pub mod application;
mod conflict;
mod filter;
mod graph;
mod log;
mod push;
pub mod staging;

pub use conflict::{Choice, Hunk, Merged, OperationStep, Piece, Proceeded, Side, Stage};
pub use filter::{LogFilter, LogOrder, TextFilter, TextMatcher, day_start, parse_day};
pub use graph::{GraphBuilder, GraphRow};
pub use log::{CommitDetails, FileChange, LogEntry, RefKind, RefLabel, SyncState};
pub use push::{PushPlan, PushedBranch, Remote, url_without_password};

/// The repository's `HEAD` moved (a commit, a checkout, a reset): what's committed changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadChanged {
    pub root: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
}

impl DomainEvent for HeadChanged {}

/// The project's repository was found (looked for in the background as the project opens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryOpened {
    pub root: PathBuf,
}

impl DomainEvent for RepositoryOpened {}

/// How a group of lines differs from the committed version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Modified,
    /// Lines were removed before `lines.start` (`lines` is empty).
    Deleted,
}

/// Changed lines of the current text (0-based, end exclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineChange {
    pub lines: Range<usize>,
    pub kind: ChangeKind,
    /// The committed lines they replace (0-based, in the base text).
    pub base: Range<usize>,
}

/// The changes of `current` against `base` (the committed text), for the gutter.
pub fn line_changes(base: &str, current: &str) -> Vec<LineChange> {
    let a: Vec<&str> = base.lines().collect();
    let b: Vec<&str> = current.lines().collect();
    diff::diff_lines(&a, &b)
        .into_iter()
        .filter(diff::Chunk::is_change)
        .map(|chunk| {
            let (base, lines) = (chunk.left(), chunk.right());
            let kind = match (base.is_empty(), lines.is_empty()) {
                (true, _) => ChangeKind::Added,
                (false, true) => ChangeKind::Deleted,
                (false, false) => ChangeKind::Modified,
            };
            LineChange { lines, kind, base }
        })
        .collect()
}

/// A commit, as shown in history and blame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The full id (hex).
    pub id: String,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    /// The first line of the message.
    pub summary: String,
}

impl Commit {
    /// The short id (7 characters), as git shows it.
    pub fn short(&self) -> &str {
        &self.id[..self.id.len().min(7)]
    }
}

/// Lines of the current file attributed to a commit by blame (0-based, end exclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameHunk {
    pub lines: Range<usize>,
    pub commit: Commit,
}

/// For each line of the current text, the blame hunk (index in `blame`) of the committed
/// line it still is; `None` for lines added or changed since the commit.
pub fn blame_lines(
    changes: &[LineChange],
    blame: &[BlameHunk],
    line_count: usize,
) -> Vec<Option<usize>> {
    let hunk_of = |base: usize| {
        let i = blame.partition_point(|h| h.lines.end <= base);
        blame.get(i).filter(|h| h.lines.contains(&base)).map(|_| i)
    };
    let mut out = Vec::with_capacity(line_count);
    let mut next = 0;
    // Committed line = current line - shift (lines added minus removed before it).
    let mut shift: isize = 0;
    for line in 0..line_count {
        // The changes entirely before this line (a deletion at `s` counts from line `s`).
        while let Some(change) = changes.get(next).filter(|c| c.lines.end <= line) {
            shift += change.lines.len() as isize - change.base.len() as isize;
            next += 1;
        }
        let changed = changes.get(next).is_some_and(|c| c.lines.contains(&line));
        out.push(if changed { None } else { hunk_of((line as isize - shift).max(0) as usize) });
    }
    out
}

/// The calendar date (year, month, day, UTC) of `seconds` since the Unix epoch.
pub fn date(seconds: i64) -> (i64, u32, u32) {
    // Days → civil date (Howard Hinnant's algorithm).
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// How a file differs from the last commit (the working tree and what's staged).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileState {
    Modified,
    Added,
    Deleted,
    Renamed,
    /// Not tracked yet (new, never added).
    Untracked,
    /// A merge left conflicts in it.
    Conflicted,
}

/// A changed file of the working tree (path relative to the root, with `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    pub path: String,
    pub state: FileState,
}

/// A changed file as the staging area sees it: what's staged of it, what isn't (a file edited
/// again after staging has both).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile {
    pub path: String,
    pub staged: Option<FileState>,
    pub unstaged: Option<FileState>,
}

/// Where a patch goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchTarget {
    /// The staging area.
    Staged,
    /// The file on disk.
    WorkingTree,
}

/// Why a version control operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcsError {
    /// The tool the operation needs isn't installed (e.g. `git`, to write commits).
    ToolMissing(String),
    /// The repository has no remote to push to / pull from.
    NoRemote,
    /// The remote asked for credentials nobody can type here (no credential helper / key).
    Authentication(String),
    /// It took too long (a network that doesn't answer); the operation was stopped.
    TimedOut,
    /// The operation itself failed (its message).
    Failed(String),
    /// The repository's own settings run programs (its `.git/config`'s lines that do): git
    /// runs in it only once the user trusts it.
    Untrusted(Vec<String>),
}

impl std::fmt::Display for VcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ToolMissing(tool) => write!(f, "{tool} is not installed"),
            Self::NoRemote => f.write_str("no remote"),
            Self::Authentication(message) => write!(f, "authentication failed: {message}"),
            Self::TimedOut => f.write_str("timed out"),
            Self::Failed(message) => f.write_str(message),
            Self::Untrusted(lines) => write!(f, "not trusted: {}", lines.join("; ")),
        }
    }
}

/// What a push / pull / fetch did (its output, for the user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteOutcome {
    /// Nothing to do (already up to date / everything pushed).
    pub up_to_date: bool,
    pub message: String,
}

/// A revision to read a file at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    Head,
    /// What is staged (the index).
    Index,
    /// A commit id or a branch name.
    Named(String),
    /// One of the versions of a conflicted file (while the conflict lasts).
    Conflict(Stage),
}

/// An operation left in progress in the repository (by us or another program: SourceTree, the
/// terminal), until it's continued or given up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub kind: OperationKind,
    /// The branch it works on (a rebase's: the one being rebased), when it says.
    pub branch: Option<String>,
    /// Its step and how many there are (a rebase's commits), when it says.
    pub step: Option<(u32, u32)>,
}

/// What's in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Rebase,
    Merge,
    CherryPick,
    Revert,
    Bisect,
    /// Patches being applied (`git am`).
    ApplyMailbox,
}

impl Operation {
    /// A rebase, from what git keeps of it: the branch's ref (`refs/heads/feature`), the
    /// step done and the steps (`msgnum` / `end`, or `next` / `last`), as written there.
    pub fn rebase(head_name: Option<&str>, done: Option<&str>, total: Option<&str>) -> Self {
        let number = |s: Option<&str>| s.and_then(|s| s.trim().parse::<u32>().ok());
        let branch = head_name
            .map(str::trim)
            .filter(|h| !h.is_empty() && *h != "detached HEAD")
            .map(|h| h.strip_prefix("refs/heads/").unwrap_or(h).to_owned());
        let step = number(done).zip(number(total)).filter(|(_, t)| *t > 0);
        Self { kind: OperationKind::Rebase, branch, step }
    }
}

/// The port to a repository. `path` is relative to the repository's root, with `/`.
pub trait Repository: Send + Sync + std::fmt::Debug {
    /// The repository's working directory.
    fn root(&self) -> &Path;
    /// The current branch (or the short commit id when detached).
    fn branch(&self) -> Option<String>;
    /// `HEAD` points at a commit, not a branch (during a rebase, after checking a commit out).
    fn detached(&self) -> bool {
        false
    }
    /// An operation left in progress (a rebase, a merge…), if any.
    fn operation(&self) -> Option<Operation> {
        None
    }
    /// The id of `HEAD` (changes on commit, checkout, reset): cached texts are reread.
    fn head_id(&self) -> Option<String>;
    /// The file's text at `revision` (`None`: not there, e.g. a new file).
    fn text_at(&self, revision: &Revision, path: &str) -> Option<String>;
    /// The commits that changed the file, newest first, at most `limit`.
    fn history(&self, path: &str, limit: usize) -> Vec<Commit>;
    /// Who last changed each line of the committed file.
    fn blame(&self, path: &str) -> Vec<BlameHunk>;
    /// The local branches.
    fn branches(&self) -> Vec<String>;
    /// The changed files of the working tree.
    fn status(&self) -> Result<Vec<FileStatus>, VcsError>;
    /// The changed files, split by the staging area.
    fn staged_files(&self) -> Result<Vec<StagedFile>, VcsError> {
        Err(VcsError::Failed("no staging area".into()))
    }
    /// `paths`' changes put on the staging area.
    fn stage(&self, paths: &[String]) -> Result<(), VcsError> {
        let _ = paths;
        Err(VcsError::Failed("no staging area".into()))
    }
    /// `paths` taken off the staging area (their files untouched).
    fn unstage(&self, paths: &[String]) -> Result<(), VcsError> {
        let _ = paths;
        Err(VcsError::Failed("no staging area".into()))
    }
    /// `paths`' changes not staged thrown away (a new file: deleted). It can't be undone.
    fn discard(&self, paths: &[String]) -> Result<(), VcsError> {
        let _ = paths;
        Err(VcsError::Failed("no staging area".into()))
    }
    /// `paths` as they were at `commit`, on the staging area and on disk (their changes are
    /// lost). It can't be undone.
    fn reset_to(&self, paths: &[String], commit: &str) -> Result<(), VcsError> {
        let _ = (paths, commit);
        Err(VcsError::Failed("no staging area".into()))
    }
    /// A unified diff applied to `target` (`reverse`: taken back): a part of a file.
    fn apply_patch(&self, patch: &str, target: PatchTarget, reverse: bool) -> Result<(), VcsError> {
        let _ = (patch, target, reverse);
        Err(VcsError::Failed("no staging area".into()))
    }
    /// Commits what's staged with `message`; returns the new commit's id.
    fn commit_staged(&self, message: &str) -> Result<String, VcsError> {
        let _ = message;
        Err(VcsError::Failed("no staging area".into()))
    }
    /// Commits the changes of `paths` (relative, with `/`) with `message`; returns the new
    /// commit's id.
    fn commit(&self, paths: &[String], message: &str) -> Result<String, VcsError>;
    /// The log of every branch (local and remote) and `HEAD` — or only of the filter's
    /// references (a branch, `origin/main`, a tag) — in the filter's order, only the commits
    /// it lets through: `limit` of them after skipping `skip` (read in pages).
    fn log(&self, filter: &LogFilter, skip: usize, limit: usize) -> Vec<LogEntry>;
    /// Every author of the history (their names, sorted), as far as `limit` commits back.
    fn authors(&self, limit: usize) -> Vec<String> {
        let _ = limit;
        Vec::new()
    }
    /// Who commits here (`user.name` of git's settings), if set.
    fn user_name(&self) -> Option<String> {
        None
    }
    /// The files git tracks (the index's paths, relative, with `/`).
    fn tracked_files(&self) -> Vec<String> {
        Vec::new()
    }
    /// The last commit's whole message (`None`: no commit yet).
    fn last_commit_message(&self) -> Option<String> {
        None
    }
    /// The last commit was pushed already (a remote branch holds it): amending it rewrites
    /// published history.
    fn head_pushed(&self) -> bool {
        false
    }
    /// The last commit made again with `message` and the changes of `paths` (none: only the
    /// message changes); returns its new id.
    fn amend(&self, paths: &[String], message: &str) -> Result<String, VcsError> {
        let _ = (paths, message);
        Err(VcsError::Failed("amend not supported".into()))
    }
    /// Branch `name` made at `from` (a commit, a branch; `None`: `HEAD`), checked out after
    /// when `checkout`.
    fn create_branch(
        &self,
        name: &str,
        from: Option<&str>,
        checkout: bool,
    ) -> Result<(), VcsError> {
        let _ = (name, from, checkout);
        Err(VcsError::Failed("branches not supported".into()))
    }
    /// Local branch `from` renamed `to`.
    fn rename_branch(&self, from: &str, to: &str) -> Result<(), VcsError> {
        let _ = (from, to);
        Err(VcsError::Failed("branches not supported".into()))
    }
    /// Tag `name` on `on` (`None`: `HEAD`); with a `message`, an annotated one.
    fn create_tag(
        &self,
        name: &str,
        on: Option<&str>,
        message: Option<&str>,
    ) -> Result<(), VcsError> {
        let _ = (name, on, message);
        Err(VcsError::Failed("tags not supported".into()))
    }
    /// The branches (local and remote) whose history holds commit `id`.
    fn containing_branches(&self, id: &str) -> Vec<String> {
        let _ = id;
        Vec::new()
    }
    /// Every local branch tracking another one, against it (ahead / behind).
    fn branch_states(&self) -> Vec<SyncState> {
        Vec::new()
    }
    /// The local branches, the remote ones and the tags (by kind, then name).
    fn refs(&self) -> Vec<RefLabel>;
    /// A commit's message and changed files.
    fn commit_details(&self, id: &str) -> Option<CommitDetails>;
    /// The current branch against its upstream (`None`: detached, or no branch yet).
    fn sync_state(&self) -> Option<SyncState>;
    /// Downloads what the remotes have (nothing local changes).
    fn fetch(&self) -> Result<RemoteOutcome, VcsError>;
    /// Brings the upstream's commits into the current branch.
    fn pull(&self) -> Result<RemoteOutcome, VcsError>;
    /// Sends the current branch's commits to its upstream (publishing the branch if it
    /// tracks nothing yet).
    fn push(&self) -> Result<RemoteOutcome, VcsError>;
    /// The remotes, by name.
    fn remotes(&self) -> Vec<Remote> {
        Vec::new()
    }
    /// The stashes, newest first.
    fn stashes(&self) -> Vec<Stash> {
        Vec::new()
    }
    /// The submodules' folders (relative to the root, with `/`), by name.
    fn submodules(&self) -> Vec<String> {
        Vec::new()
    }
    /// Sends what `plan` says.
    fn push_plan(&self, plan: &PushPlan) -> Result<RemoteOutcome, VcsError> {
        let _ = plan;
        Err(VcsError::NoRemote)
    }
    /// The conflicted `paths` resolved by taking `side`'s whole version (removed where that
    /// side deleted the file), marked resolved.
    fn take_side(&self, paths: &[String], side: Side) -> Result<(), VcsError> {
        let _ = (paths, side);
        Err(VcsError::Failed("conflicts not supported".into()))
    }
    /// The conflicted `paths` marked resolved as they are now in the working tree (removed
    /// ones: removed).
    fn mark_resolved(&self, paths: &[String]) -> Result<(), VcsError> {
        let _ = paths;
        Err(VcsError::Failed("conflicts not supported".into()))
    }
    /// `step` done with the operation in progress (see [`OperationKind::allows`]).
    fn proceed(&self, step: OperationStep) -> Result<Proceeded, VcsError> {
        let _ = step;
        Err(VcsError::Failed("operations not supported".into()))
    }
}

/// The extension point of version control systems (contributed by modules, e.g. git):
/// opens the repository a project is in.
pub trait VcsProvider: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &'static str;
    /// The repository containing `dir`, if any (never fails loudly: no repository = `None`).
    fn open(&self, dir: &Path) -> Option<std::sync::Arc<dyn Repository>>;
    /// The folder a repository keeps its own data in (`.git`): a change inside means what's
    /// committed may differ; never shown in the tree.
    fn metadata_dir(&self) -> Option<&'static str> {
        None
    }
}

/// Why a branch or tag name can't be one (as `git check-ref-format --branch` says).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefNameError {
    Empty,
    /// A space, a control character or one of `~ ^ : ? * [ \`.
    BadCharacter,
    /// `..`, `@{`, `//`; or it starts with `-` or `/`, ends with `/`, `.` or `.lock`, a
    /// part starts with `.`; or it's `@`.
    BadShape,
    /// A reference of that name exists already.
    Exists,
}

/// Whether `name` can name a branch or a tag (git's rules for references' names).
pub fn check_ref_name(name: &str) -> Result<(), RefNameError> {
    if name.is_empty() {
        return Err(RefNameError::Empty);
    }
    if name.chars().any(|c| c.is_control() || c == ' ' || "~^:?*[\\".contains(c)) {
        return Err(RefNameError::BadCharacter);
    }
    let bad = name == "@"
        || name.contains("..")
        || name.contains("@{")
        || name.contains("//")
        || name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with('.')
        || name.ends_with(".lock")
        || name.split('/').any(|part| part.starts_with('.') || part.ends_with(".lock"));
    if bad {
        return Err(RefNameError::BadShape);
    }
    Ok(())
}

/// `path` relative to the repository at `root`, with `/` (Windows too); `None` outside it.
pub fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> =
        relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A stash: its name (`stash@{0}`) and its message (`On main: wip`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub name: String,
    pub message: String,
}

/// `git stash list --format=%gd%x1f%s`'s lines as stashes.
pub fn parse_stashes(out: &str) -> Vec<Stash> {
    out.lines()
        .filter_map(|line| {
            let (name, message) = line.split_once('\u{1f}')?;
            Some(Stash { name: name.to_owned(), message: message.to_owned() })
        })
        .collect()
}

/// `git config --file .gitmodules --get-regexp path`'s lines as the submodules' folders.
pub fn parse_submodules(out: &str) -> Vec<String> {
    let mut paths: Vec<String> = out
        .lines()
        .filter_map(|line| line.split_once(' ').map(|(_, path)| path.trim().to_owned()))
        .filter(|path| !path.is_empty())
        .collect();
    paths.sort();
    paths
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/operation.rs"]
mod operation_tests;
