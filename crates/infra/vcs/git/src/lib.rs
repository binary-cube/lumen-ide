//! The git adapter of [`version_control::Repository`]. Reading (file texts at a revision, a
//! file's history, blame, branches, the log with its references, a commit's changed files,
//! how far the branch is from its upstream) uses gitoxide: pure Rust, no `git` executable
//! needed (Windows too), no processes. The working tree's status, commits and the remote
//! (fetch, pull, push) use the installed `git` (so the user's hooks, signing, credential
//! helpers and SSH keys apply), never waiting for a prompt nobody sees and stopped if the
//! network doesn't answer; without `git` they report [`VcsError::ToolMissing`] and nothing
//! else is affected.

mod process;
mod switching;
pub mod trust;

/// The process ids of the `git`s running now.
pub fn running() -> Vec<u32> {
    process::RUNNING.pids()
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gix::bstr::{BStr, ByteSlice};
use version_control::{
    BlameHunk, Commit, CommitDetails, FileChange, FileState, FileStatus, LogEntry, LogFilter,
    LogOrder, OperationKind, OperationStep, PatchTarget, Proceeded, PushPlan, RefKind, RefLabel,
    Remote, RemoteOutcome, Repository, Revision, Side, Stage, StagedFile, SyncState, TextMatcher,
    VcsError,
};

/// A git repository (shared between threads; each call works on a thread-local handle).
pub struct GitRepository {
    repo: gix::ThreadSafeRepository,
    root: PathBuf,
}

impl std::fmt::Debug for GitRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitRepository").field("root", &self.root).finish()
    }
}

/// How many commits [`Repository::history`] looks through at most (a file untouched for a
/// long time doesn't make it walk the whole history).
const HISTORY_SCAN: usize = 20_000;

impl GitRepository {
    /// The repository containing `dir` (it or a parent), if any, with a working tree.
    pub fn discover(dir: &Path) -> Option<Self> {
        let repo = gix::ThreadSafeRepository::discover(dir).ok()?;
        let root = repo.to_thread_local().workdir()?.to_path_buf();
        let root = root.canonicalize().unwrap_or(root);
        Some(Self { repo, root })
    }

    fn local(&self) -> gix::Repository {
        self.repo.to_thread_local()
    }

    /// Runs `git` in the working tree (local work: a minute at most); its output.
    fn git(&self, args: &[&str]) -> Result<String, VcsError> {
        process::run(&self.root, args, LOCAL_TIMEOUT, false).map(|out| out.stdout)
    }

    /// `git <command> <paths…>`, each path taken as it is (no glob in a name is expanded).
    fn on_paths(&self, command: &[&str], paths: &[String]) -> Result<(), VcsError> {
        let mut args = vec!["--literal-pathspecs"];
        args.extend_from_slice(command);
        args.extend(paths.iter().map(String::as_str));
        self.git(&args).map(|_| ())
    }

    /// Runs `git` talking to a remote (longer timeout, SSH never asking for a passphrase
    /// unless the user configured how).
    fn git_remote(&self, args: &[&str]) -> Result<process::Output, VcsError> {
        let ssh_configured = self.local().config_snapshot().string("core.sshCommand").is_some();
        process::run(&self.root, args, REMOTE_TIMEOUT, !ssh_configured)
    }

    /// Every reference by the commit it points at (for the log's labels).
    fn labels(&self, repo: &gix::Repository) -> HashMap<gix::ObjectId, Vec<RefLabel>> {
        let mut labels: HashMap<gix::ObjectId, Vec<RefLabel>> = HashMap::new();
        if let Ok(head) = repo.head_id() {
            labels
                .entry(head.detach())
                .or_default()
                .push(RefLabel { name: "HEAD".into(), kind: RefKind::Head });
        }
        let Ok(references) = repo.references() else { return labels };
        let Ok(all) = references.all() else { return labels };
        for mut reference in all.flatten() {
            let Some((category, short)) = reference.name().category_and_short_name() else {
                continue;
            };
            let kind = match category {
                gix::refs::Category::LocalBranch => RefKind::Branch,
                gix::refs::Category::RemoteBranch => RefKind::Remote,
                gix::refs::Category::Tag => RefKind::Tag,
                _ => continue,
            };
            let name = short.to_str_lossy().into_owned();
            // `origin/HEAD` only repeats the remote's default branch.
            if kind == RefKind::Remote && name.ends_with("/HEAD") {
                continue;
            }
            if let Ok(id) = reference.peel_to_id() {
                labels.entry(id.detach()).or_default().push(RefLabel { name, kind });
            }
        }
        for list in labels.values_mut() {
            list.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
        }
        labels
    }
}

/// How long local `git` work (status, commit) may take.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a fetch / pull / push may take before it's stopped.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(180);
/// Counting ahead / behind stops here (a branch thousands of commits away says "10000+").
const COUNT_LIMIT: usize = 10_000;

/// The commits reachable from `from` and not from `hidden`, counted up to the limit.
fn count_between(repo: &gix::Repository, from: gix::ObjectId, hidden: gix::ObjectId) -> usize {
    repo.rev_walk([from])
        .with_hidden([hidden])
        .all()
        .map(|walk| walk.take(COUNT_LIMIT).filter(Result::is_ok).count())
        .unwrap_or(0)
}

/// A tree change as a changed file (directories left out).
fn file_change(change: &gix::object::tree::diff::ChangeDetached) -> Option<FileChange> {
    use gix::object::tree::diff::ChangeDetached as Change;
    if change.entry_mode().is_tree() {
        return None;
    }
    let path = change.location().to_str_lossy().into_owned();
    let (state, old_path) = match change {
        Change::Addition { .. } => (FileState::Added, None),
        Change::Deletion { .. } => (FileState::Deleted, None),
        Change::Modification { .. } => (FileState::Modified, None),
        Change::Rewrite { source_location, copy, .. } => {
            let state = if *copy { FileState::Added } else { FileState::Renamed };
            (state, Some(source_location.to_str_lossy().into_owned()))
        }
    };
    Some(FileChange { path, old_path, state })
}

/// The entries of `git status --porcelain=v1 -z` (`XY path`, NUL-separated; a rename is
/// followed by its old path).
pub fn parse_status(output: &str) -> Vec<FileStatus> {
    let mut entries = output.split('\0').filter(|e| !e.is_empty());
    let mut found = Vec::new();
    while let Some(entry) = entries.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else { continue };
        let (x, y) = (code.as_bytes()[0], code.as_bytes()[1]);
        let state = match (x, y) {
            (b'?', b'?') => FileState::Untracked,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => FileState::Conflicted,
            (b'R', _) | (b'C', _) => {
                // The old path follows.
                entries.next();
                FileState::Renamed
            }
            (b'D', _) | (_, b'D') => FileState::Deleted,
            (b'A', _) => FileState::Added,
            _ => FileState::Modified,
        };
        found.push(FileStatus { path: path.to_owned(), state });
    }
    found
}

/// `git status --porcelain=v1 -z`'s output, split by the staging area.
pub fn parse_staged(output: &str) -> Vec<StagedFile> {
    let state = |code: u8| match code {
        b'A' => Some(FileState::Added),
        b'D' => Some(FileState::Deleted),
        b'R' | b'C' => Some(FileState::Renamed),
        b'M' | b'T' => Some(FileState::Modified),
        _ => None,
    };
    let mut entries = output.split('\0').filter(|e| !e.is_empty());
    let mut found = Vec::new();
    while let Some(entry) = entries.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else { continue };
        let (x, y) = (code.as_bytes()[0], code.as_bytes()[1]);
        if matches!(x, b'R' | b'C') {
            // The old path follows.
            entries.next();
        }
        let (staged, unstaged) = match (x, y) {
            (b'?', b'?') => (None, Some(FileState::Untracked)),
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => {
                (None, Some(FileState::Conflicted))
            }
            _ => (state(x), state(y)),
        };
        found.push(StagedFile { path: path.to_owned(), staged, unstaged });
    }
    found
}

/// The index's stage holding `stage`'s version of a conflicted file.
fn index_stage(stage: Stage) -> gix::index::entry::Stage {
    match stage {
        Stage::Base => gix::index::entry::Stage::Base,
        Stage::Ours => gix::index::entry::Stage::Ours,
        Stage::Theirs => gix::index::entry::Stage::Theirs,
    }
}

/// The `git` command doing `step` with an operation of `kind` (`None`: it has no such step).
/// A merge is continued by committing it (its prepared message kept).
pub fn operation_args(kind: OperationKind, step: OperationStep) -> Option<&'static [&'static str]> {
    use OperationKind::{ApplyMailbox, Bisect, CherryPick, Merge, Rebase, Revert};
    use OperationStep::{Abort, Continue, Skip};
    if !kind.allows(step) {
        return None;
    }
    Some(match (kind, step) {
        (Merge, Continue) => &["commit", "--no-edit"],
        (Merge, _) => &["merge", "--abort"],
        (Bisect, _) => &["bisect", "reset"],
        (Rebase, Continue) => &["rebase", "--continue"],
        (Rebase, Skip) => &["rebase", "--skip"],
        (Rebase, Abort) => &["rebase", "--abort"],
        (CherryPick, Continue) => &["cherry-pick", "--continue"],
        (CherryPick, Skip) => &["cherry-pick", "--skip"],
        (CherryPick, Abort) => &["cherry-pick", "--abort"],
        (Revert, Continue) => &["revert", "--continue"],
        (Revert, Skip) => &["revert", "--skip"],
        (Revert, Abort) => &["revert", "--abort"],
        (ApplyMailbox, Continue) => &["am", "--continue"],
        (ApplyMailbox, Skip) => &["am", "--skip"],
        (ApplyMailbox, Abort) => &["am", "--abort"],
    })
}

/// The blob id of `path` in commit `id`'s tree.
fn blob_in(repo: &gix::Repository, id: gix::ObjectId, path: &str) -> Option<gix::ObjectId> {
    let commit = repo.find_commit(id).ok()?;
    let entry = commit.tree().ok()?.lookup_entry_by_path(path).ok()??;
    Some(entry.object_id())
}

/// Commit `id` (with `parents`) as the log shows it, if `filter` lets it through (`text`:
/// its text filter compiled).
fn log_entry(
    repo: &gix::Repository,
    labels: &HashMap<gix::ObjectId, Vec<RefLabel>>,
    filter: &LogFilter,
    text: Option<&TextMatcher>,
    id: gix::ObjectId,
    parents: &[gix::ObjectId],
) -> Option<LogEntry> {
    let commit = commit_info(repo, id)?;
    if !filter.keeps_every_commit() {
        let object = repo.find_commit(id).ok()?;
        let email =
            object.author().ok().map(|a| a.email.to_str_lossy().into_owned()).unwrap_or_default();
        let message = object.message_raw_sloppy().to_str_lossy().into_owned();
        if !filter.matches(text, &commit, &email, &message)
            || (!filter.paths.is_empty() && !changes_any(repo, id, parents.first(), &filter.paths))
        {
            return None;
        }
    }
    Some(LogEntry {
        commit,
        parents: parents.iter().map(ToString::to_string).collect(),
        refs: labels.get(&id).cloned().unwrap_or_default(),
    })
}

/// Commit `id` changed one of `paths` (a file, or anything in a folder) against its first
/// parent (`None`: a root commit, which adds them).
fn changes_any(
    repo: &gix::Repository,
    id: gix::ObjectId,
    parent: Option<&gix::ObjectId>,
    paths: &[String],
) -> bool {
    let tree = |c: gix::ObjectId| repo.find_commit(c).ok()?.tree().ok();
    let Some(mine) = tree(id) else { return false };
    let theirs = parent.and_then(|p| tree(*p));
    let at = |tree: &gix::Tree<'_>, path: &str| {
        tree.lookup_entry_by_path(path).ok().flatten().map(|e| e.object_id())
    };
    paths.iter().any(|path| {
        let path = path.trim_end_matches('/');
        let here = at(&mine, path);
        let before = theirs.as_ref().and_then(|t| at(t, path));
        here != before
    })
}

fn commit_info(repo: &gix::Repository, id: gix::ObjectId) -> Option<Commit> {
    let commit = repo.find_commit(id).ok()?;
    let author =
        commit.author().ok().map(|a| a.name.to_str_lossy().into_owned()).unwrap_or_default();
    let time = commit.time().map(|t| t.seconds).unwrap_or_default();
    let summary =
        commit.message().map(|m| m.summary().to_str_lossy().into_owned()).unwrap_or_default();
    Some(Commit { id: id.to_string(), author, time, summary })
}

impl Repository for GitRepository {
    fn root(&self) -> &Path {
        &self.root
    }

    fn branch(&self) -> Option<String> {
        let repo = self.local();
        match repo.head_name().ok()? {
            Some(name) => Some(name.shorten().to_str_lossy().into_owned()),
            None => repo.head_id().ok().map(|id| id.to_string().chars().take(7).collect()),
        }
    }

    fn head_id(&self) -> Option<String> {
        self.local().head_id().ok().map(|id| id.to_string())
    }

    fn detached(&self) -> bool {
        self.local().head_name().ok().is_some_and(|name| name.is_none())
    }

    /// What git keeps of an operation left in progress (its state files).
    fn operation(&self) -> Option<version_control::Operation> {
        use gix::state::InProgress;
        use version_control::Operation;
        let repo = self.local();
        let state = repo.state()?;
        let dir = repo.git_dir();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        let plain = |kind| Operation { kind, branch: None, step: None };
        Some(match state {
            InProgress::Rebase | InProgress::RebaseInteractive => {
                let (head, done, total) = (
                    read("rebase-merge/head-name"),
                    read("rebase-merge/msgnum"),
                    read("rebase-merge/end"),
                );
                Operation::rebase(head.as_deref(), done.as_deref(), total.as_deref())
            }
            InProgress::ApplyMailboxRebase => {
                let (head, done, total) = (
                    read("rebase-apply/head-name"),
                    read("rebase-apply/next"),
                    read("rebase-apply/last"),
                );
                Operation::rebase(head.as_deref(), done.as_deref(), total.as_deref())
            }
            InProgress::ApplyMailbox => plain(OperationKind::ApplyMailbox),
            InProgress::Merge => plain(OperationKind::Merge),
            InProgress::CherryPick | InProgress::CherryPickSequence => {
                plain(OperationKind::CherryPick)
            }
            InProgress::Revert | InProgress::RevertSequence => plain(OperationKind::Revert),
            InProgress::Bisect => plain(OperationKind::Bisect),
        })
    }

    fn text_at(&self, revision: &Revision, path: &str) -> Option<String> {
        let repo = self.local();
        let blob = match revision {
            Revision::Index => {
                let index = repo.index_or_empty().ok()?;
                index.entry_by_path(BStr::new(path))?.id
            }
            Revision::Conflict(stage) => {
                let index = repo.index_or_empty().ok()?;
                index.entry_by_path_and_stage(BStr::new(path), index_stage(*stage))?.id
            }
            Revision::Head => blob_in(&repo, repo.head_id().ok()?.detach(), path)?,
            Revision::Named(name) => {
                let id = repo.rev_parse_single(name.as_str()).ok()?;
                let commit = id.object().ok()?.peel_to_commit().ok()?;
                blob_in(&repo, commit.id, path)?
            }
        };
        let data = repo.find_blob(blob).ok()?;
        Some(String::from_utf8_lossy(&data.data).into_owned())
    }

    fn history(&self, path: &str, limit: usize) -> Vec<Commit> {
        let repo = self.local();
        let Ok(head) = repo.head_id() else { return Vec::new() };
        let Ok(walk) = repo.rev_walk([head.detach()]).all() else { return Vec::new() };
        let mut found = Vec::new();
        for info in walk.take(HISTORY_SCAN).flatten() {
            let mine = blob_in(&repo, info.id, path);
            // Changed here: different from the first parent (or added by a root commit).
            let parent = info.parent_ids.first().and_then(|p| blob_in(&repo, *p, path));
            if mine.is_some() && mine != parent {
                if let Some(commit) = commit_info(&repo, info.id) {
                    found.push(commit);
                }
                if found.len() >= limit {
                    break;
                }
            }
        }
        found
    }

    fn blame(&self, path: &str) -> Vec<BlameHunk> {
        let repo = self.local();
        let Ok(head) = repo.head_id() else { return Vec::new() };
        let outcome = match repo.blame_file(BStr::new(path), head.detach(), Default::default()) {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::debug!(%path, error = %e, "blame failed");
                return Vec::new();
            }
        };
        let mut commits: std::collections::HashMap<gix::ObjectId, Commit> = Default::default();
        outcome
            .entries
            .iter()
            .filter_map(|entry| {
                let commit = match commits.get(&entry.commit_id) {
                    Some(commit) => commit.clone(),
                    None => {
                        let commit = commit_info(&repo, entry.commit_id)?;
                        commits.insert(entry.commit_id, commit.clone());
                        commit
                    }
                };
                let start = entry.start_in_blamed_file as usize;
                Some(BlameHunk { lines: start..start + entry.len.get() as usize, commit })
            })
            .collect()
    }

    fn status(&self) -> Result<Vec<FileStatus>, VcsError> {
        let output = self.git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        Ok(parse_status(&output))
    }

    fn staged_files(&self) -> Result<Vec<StagedFile>, VcsError> {
        let output = self.git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        Ok(parse_staged(&output))
    }

    fn stage(&self, paths: &[String]) -> Result<(), VcsError> {
        self.on_paths(&["add", "-A", "--"], paths)
    }

    fn unstage(&self, paths: &[String]) -> Result<(), VcsError> {
        // `reset`, not `restore --staged`: it also works before the first commit.
        self.on_paths(&["reset", "-q", "--"], paths)
    }

    fn discard(&self, paths: &[String]) -> Result<(), VcsError> {
        let status = self.staged_files()?;
        let untracked = |path: &String| {
            status.iter().any(|f| f.path == *path && f.unstaged == Some(FileState::Untracked))
        };
        let (new, known): (Vec<String>, Vec<String>) = paths.iter().cloned().partition(untracked);
        if !new.is_empty() {
            self.on_paths(&["clean", "-f", "-q", "--"], &new)?;
        }
        if !known.is_empty() {
            self.on_paths(&["checkout", "--"], &known)?;
        }
        Ok(())
    }

    fn reset_to(&self, paths: &[String], commit: &str) -> Result<(), VcsError> {
        self.on_paths(&["checkout", commit, "--"], paths)
    }

    fn apply_patch(&self, patch: &str, target: PatchTarget, reverse: bool) -> Result<(), VcsError> {
        let file = std::env::temp_dir().join(format!("git-patch-{}.diff", kernel::id::new_uuid()));
        std::fs::write(&file, patch).map_err(|e| VcsError::Failed(e.to_string()))?;
        let mut args = vec!["apply", "--whitespace=nowarn"];
        if target == PatchTarget::Staged {
            args.push("--cached");
        }
        if reverse {
            args.push("--reverse");
        }
        let path = file.to_string_lossy().into_owned();
        args.push(&path);
        let applied = self.git(&args).map(|_| ());
        let _ = std::fs::remove_file(&file);
        applied
    }

    fn commit_staged(&self, message: &str) -> Result<String, VcsError> {
        self.git(&["commit", "-m", message])?;
        Ok(self.git(&["rev-parse", "HEAD"])?.trim().to_owned())
    }

    fn commit(&self, paths: &[String], message: &str) -> Result<String, VcsError> {
        let mut add = vec!["add", "-A", "--"];
        add.extend(paths.iter().map(String::as_str));
        self.git(&add)?;
        // Only these paths: whatever else is staged stays out of this commit.
        let mut commit = vec!["commit", "-m", message, "--"];
        commit.extend(paths.iter().map(String::as_str));
        self.git(&commit)?;
        Ok(self.git(&["rev-parse", "HEAD"])?.trim().to_owned())
    }

    fn log(&self, filter: &LogFilter, skip: usize, limit: usize) -> Vec<LogEntry> {
        let repo = self.local();
        let labels = self.labels(&repo);
        let mut tips: Vec<gix::ObjectId> = if filter.references.is_empty() {
            // Every branch, tag and `HEAD`: the whole graph, not only the current line.
            labels.keys().copied().filter(|id| repo.find_commit(*id).is_ok()).collect()
        } else {
            // Those references' lines (`main`, `origin/main`, `v1.2`).
            filter
                .references
                .iter()
                .filter_map(|name| {
                    let id = repo.rev_parse_single(name.as_str()).ok()?;
                    Some(id.object().ok()?.peel_to_commit().ok()?.id)
                })
                .collect()
        };
        tips.sort_unstable();
        tips.dedup();
        if tips.is_empty() {
            return Vec::new();
        }
        let text = filter.text.as_ref().and_then(|t| TextMatcher::new(t).ok().flatten());
        let found = |(id, parents): (gix::ObjectId, Vec<gix::ObjectId>)| {
            log_entry(&repo, &labels, filter, text.as_ref(), id, &parents)
        };
        match filter.order {
            LogOrder::Date => {
                let order = gix::traverse::commit::simple::CommitTimeOrder::NewestFirst;
                // A time window: the walk stops at its start.
                let sorting = match filter.since {
                    Some(seconds) => {
                        gix::revision::walk::Sorting::ByCommitTimeCutoff { order, seconds }
                    }
                    None => gix::revision::walk::Sorting::ByCommitTime(order),
                };
                let Ok(walk) = repo.rev_walk(tips).sorting(sorting).all() else {
                    return Vec::new();
                };
                walk.flatten()
                    .map(|info| (info.id, info.parent_ids.to_vec()))
                    .filter_map(found)
                    .skip(skip)
                    .take(limit)
                    .collect()
            }
            LogOrder::Topological => {
                let walk = gix::traverse::commit::topo::Builder::from_iters(
                    &repo.objects,
                    tips,
                    None::<Vec<gix::ObjectId>>,
                )
                .sorting(gix::traverse::commit::topo::Sorting::TopoOrder)
                .build();
                let Ok(walk) = walk else { return Vec::new() };
                walk.flatten()
                    .map(|info| (info.id, info.parent_ids.to_vec()))
                    .filter_map(found)
                    .skip(skip)
                    .take(limit)
                    .collect()
            }
        }
    }

    fn authors(&self, limit: usize) -> Vec<String> {
        let repo = self.local();
        let tips: Vec<gix::ObjectId> = self.labels(&repo).into_keys().collect();
        let Ok(walk) = repo.rev_walk(tips).all() else { return Vec::new() };
        let mut names = std::collections::BTreeSet::new();
        for info in walk.take(limit).flatten() {
            if let Ok(commit) = repo.find_commit(info.id)
                && let Ok(author) = commit.author()
            {
                let name = author.name.to_str_lossy();
                if !names.contains(name.as_ref()) {
                    names.insert(name.into_owned());
                }
            }
        }
        names.into_iter().collect()
    }

    fn user_name(&self) -> Option<String> {
        let repo = self.local();
        let name = repo.config_snapshot().string("user.name")?.to_str_lossy().trim().to_owned();
        (!name.is_empty()).then_some(name)
    }

    fn tracked_files(&self) -> Vec<String> {
        let repo = self.local();
        let Ok(index) = repo.index_or_empty() else { return Vec::new() };
        index.entries().iter().map(|e| e.path(&index).to_str_lossy().into_owned()).collect()
    }

    fn last_commit_message(&self) -> Option<String> {
        let repo = self.local();
        let head = repo.head_id().ok()?.detach();
        let commit = repo.find_commit(head).ok()?;
        Some(commit.message_raw_sloppy().to_str_lossy().trim_end().to_owned())
    }

    fn head_pushed(&self) -> bool {
        let Some(head) = self.head_id() else { return false };
        let repo = self.local();
        let remotes: Vec<String> = self
            .labels(&repo)
            .into_values()
            .flatten()
            .filter(|l| l.kind == RefKind::Remote)
            .map(|l| l.name)
            .collect();
        !remotes.is_empty() && self.containing_branches(&head).iter().any(|b| remotes.contains(b))
    }

    fn amend(&self, paths: &[String], message: &str) -> Result<String, VcsError> {
        if paths.is_empty() {
            // Only the message: whatever is staged stays out.
            self.git(&["commit", "--amend", "--only", "-m", message])?;
        } else {
            let mut add = vec!["add", "-A", "--"];
            add.extend(paths.iter().map(String::as_str));
            self.git(&add)?;
            let mut commit = vec!["commit", "--amend", "-m", message, "--"];
            commit.extend(paths.iter().map(String::as_str));
            self.git(&commit)?;
        }
        Ok(self.git(&["rev-parse", "HEAD"])?.trim().to_owned())
    }

    fn create_branch(
        &self,
        name: &str,
        from: Option<&str>,
        checkout: bool,
    ) -> Result<(), VcsError> {
        let from = from.unwrap_or("HEAD");
        if checkout {
            self.git(&["checkout", "-b", name, from])?;
        } else {
            self.git(&["branch", name, from])?;
        }
        Ok(())
    }

    fn rename_branch(&self, from: &str, to: &str) -> Result<(), VcsError> {
        self.git(&["branch", "-m", from, to]).map(|_| ())
    }

    fn create_tag(
        &self,
        name: &str,
        on: Option<&str>,
        message: Option<&str>,
    ) -> Result<(), VcsError> {
        let on = on.unwrap_or("HEAD");
        match message.map(str::trim).filter(|m| !m.is_empty()) {
            Some(message) => self.git(&["tag", "-a", "-m", message, name, on])?,
            None => self.git(&["tag", name, on])?,
        };
        Ok(())
    }

    fn containing_branches(&self, id: &str) -> Vec<String> {
        let repo = self.local();
        let Ok(target) = gix::ObjectId::from_hex(id.as_bytes()) else { return Vec::new() };
        let Some(time) = repo.find_commit(target).ok().and_then(|c| c.time().ok()) else {
            return Vec::new();
        };
        let mut names = Vec::new();
        let mut tips: Vec<(String, gix::ObjectId)> = Vec::new();
        for (tip, labels) in self.labels(&repo) {
            for label in
                labels.into_iter().filter(|l| matches!(l.kind, RefKind::Branch | RefKind::Remote))
            {
                tips.push((label.name, tip));
            }
        }
        tips.sort();
        for (name, tip) in tips {
            // Only as far back as the commit's time: older ones can't lead to it.
            let order = gix::traverse::commit::simple::CommitTimeOrder::NewestFirst;
            let sorting =
                gix::revision::walk::Sorting::ByCommitTimeCutoff { order, seconds: time.seconds };
            let found = repo
                .rev_walk([tip])
                .sorting(sorting)
                .all()
                .is_ok_and(|mut walk| walk.any(|info| info.is_ok_and(|i| i.id == target)));
            if found {
                names.push(name);
            }
        }
        names
    }

    fn branch_states(&self) -> Vec<SyncState> {
        let repo = self.local();
        let Ok(references) = repo.references() else { return Vec::new() };
        let Ok(branches) = references.local_branches() else { return Vec::new() };
        let mut states = Vec::new();
        for mut branch in branches.flatten() {
            let Ok(head) = branch.peel_to_id().map(gix::Id::detach) else { continue };
            let name = branch.name().to_owned();
            let upstream = repo
                .branch_remote_tracking_ref_name(name.as_ref(), gix::remote::Direction::Fetch)
                .and_then(Result::ok)
                .and_then(|up| {
                    let mut reference = repo.find_reference(up.as_ref()).ok()?;
                    let id = reference.peel_to_id().ok()?.detach();
                    Some((up.shorten().to_str_lossy().into_owned(), id))
                });
            let Some((upstream, up)) = upstream else { continue };
            states.push(SyncState {
                branch: name.shorten().to_str_lossy().into_owned(),
                upstream: Some(upstream),
                ahead: count_between(&repo, head, up),
                behind: count_between(&repo, up, head),
            });
        }
        states.sort_by(|a, b| a.branch.cmp(&b.branch));
        states
    }

    fn commit_details(&self, id: &str) -> Option<CommitDetails> {
        let repo = self.local();
        let id = gix::ObjectId::from_hex(id.as_bytes()).ok()?;
        let commit = repo.find_commit(id).ok()?;
        let info = commit_info(&repo, id)?;
        let email =
            commit.author().ok().map(|a| a.email.to_str_lossy().into_owned()).unwrap_or_default();
        let message = commit.message_raw_sloppy().to_str_lossy().trim_end().to_owned();
        let parents: Vec<gix::ObjectId> = commit.parent_ids().map(|p| p.detach()).collect();
        let tree = commit.tree().ok()?;
        let parent_tree = parents.first().and_then(|p| repo.find_commit(*p).ok()?.tree().ok());
        let changes = repo.diff_tree_to_tree(parent_tree.as_ref(), &tree, None).unwrap_or_default();
        let mut files: Vec<FileChange> = changes.iter().filter_map(file_change).collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Some(CommitDetails {
            commit: info,
            email,
            message,
            parents: parents.iter().map(ToString::to_string).collect(),
            files,
        })
    }

    fn sync_state(&self) -> Option<SyncState> {
        let repo = self.local();
        let head_name = repo.head_name().ok()??;
        let branch = head_name.shorten().to_str_lossy().into_owned();
        let head = repo.head_id().ok()?.detach();
        let upstream = repo
            .branch_remote_tracking_ref_name(head_name.as_ref(), gix::remote::Direction::Fetch)
            .and_then(Result::ok)
            .and_then(|name| {
                let mut reference = repo.find_reference(name.as_ref()).ok()?;
                let id = reference.peel_to_id().ok()?.detach();
                Some((name.shorten().to_str_lossy().into_owned(), id))
            });
        let Some((name, up)) = upstream else {
            return Some(SyncState { branch, upstream: None, ahead: 0, behind: 0 });
        };
        Some(SyncState {
            branch,
            upstream: Some(name),
            ahead: count_between(&repo, head, up),
            behind: count_between(&repo, up, head),
        })
    }

    fn fetch(&self) -> Result<RemoteOutcome, VcsError> {
        if self.local().remote_names().is_empty() {
            return Err(VcsError::NoRemote);
        }
        let out = self.git_remote(&["fetch", "--all"])?;
        Ok(RemoteOutcome { up_to_date: out.stderr.trim().is_empty(), message: out.summary() })
    }

    fn pull(&self) -> Result<RemoteOutcome, VcsError> {
        if self.local().remote_names().is_empty() {
            return Err(VcsError::NoRemote);
        }
        // `--no-edit`: a merge commit takes its default message (no editor opens).
        let out = self.git_remote(&["pull", "--no-edit"])?;
        let up_to_date =
            out.stdout.contains("Already up to date") || out.stdout.contains("Already up-to-date");
        Ok(RemoteOutcome { up_to_date, message: out.summary() })
    }

    fn push(&self) -> Result<RemoteOutcome, VcsError> {
        let repo = self.local();
        if repo.remote_names().is_empty() {
            return Err(VcsError::NoRemote);
        }
        let state = self.sync_state();
        let out = match state.as_ref().filter(|s| s.upstream.is_none()) {
            // Not tracking anything yet: published to the default remote, tracked from now.
            Some(state) => {
                let remote = repo
                    .remote_default_name(gix::remote::Direction::Push)
                    .map(|n| n.to_str_lossy().into_owned())
                    .unwrap_or_else(|| "origin".into());
                self.git_remote(&["push", "--set-upstream", &remote, &state.branch])?
            }
            None => self.git_remote(&["push"])?,
        };
        let up_to_date = out.stderr.contains("Everything up-to-date");
        Ok(RemoteOutcome { up_to_date, message: out.summary() })
    }

    fn stashes(&self) -> Vec<version_control::Stash> {
        let out = self.git(&["stash", "list", "--format=%gd%x1f%s%x1f%H"]).unwrap_or_default();
        version_control::parse_stashes(&out)
    }

    fn submodules(&self) -> Vec<String> {
        if !self.root.join(".gitmodules").is_file() {
            return Vec::new();
        }
        let args = ["config", "--file", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$"];
        version_control::parse_submodules(&self.git(&args).unwrap_or_default())
    }

    fn remotes(&self) -> Vec<Remote> {
        // Opened anew: this handle's settings are from when it was made, remotes added since
        // (in a terminal, by another program) aren't in them.
        let repo = gix::open(&self.root).unwrap_or_else(|_| self.local());
        let mut remotes: Vec<Remote> = repo
            .remote_names()
            .iter()
            .filter_map(|name| {
                let remote = repo.find_remote(name.as_bstr()).ok()?;
                let url = remote
                    .url(gix::remote::Direction::Push)
                    .map(|url| url.to_bstring().to_str_lossy().into_owned())
                    .unwrap_or_default();
                Some(Remote {
                    name: name.to_str_lossy().into_owned(),
                    url: version_control::url_without_password(&url),
                })
            })
            .collect();
        remotes.sort_by(|a, b| a.name.cmp(&b.name));
        remotes
    }

    fn push_plan(&self, plan: &PushPlan) -> Result<RemoteOutcome, VcsError> {
        let refspecs: Vec<String> = plan
            .branches
            .iter()
            .map(|b| format!("refs/heads/{}:refs/heads/{}", b.local, b.remote))
            .collect();
        let mut args = vec!["push"];
        if plan.force {
            args.push("--force-with-lease");
        }
        if plan.tags {
            args.push("--tags");
        }
        args.push(&plan.remote);
        args.extend(refspecs.iter().map(String::as_str));
        let out = self.git_remote(&args)?;
        // Each branch says what it follows only as itself (`--set-upstream` would be for all).
        for branch in plan.branches.iter().filter(|b| b.track) {
            self.git(&["config", &format!("branch.{}.remote", branch.local), &plan.remote])?;
            let merge = format!("refs/heads/{}", branch.remote);
            self.git(&["config", &format!("branch.{}.merge", branch.local), &merge])?;
        }
        let up_to_date = out.stderr.contains("Everything up-to-date");
        Ok(RemoteOutcome { up_to_date, message: out.summary() })
    }

    fn take_side(&self, paths: &[String], side: Side) -> Result<(), VcsError> {
        let index = self.local().index_or_empty().map_err(|e| VcsError::Failed(e.to_string()))?;
        let (kept, gone): (Vec<&str>, Vec<&str>) =
            paths.iter().map(String::as_str).partition(|path| {
                index.entry_by_path_and_stage(BStr::new(*path), index_stage(side.into())).is_some()
            });
        if !kept.is_empty() {
            let flag = match side {
                Side::Ours => "--ours",
                Side::Theirs => "--theirs",
            };
            self.git(&[&["checkout", flag, "--"], kept.as_slice()].concat())?;
            self.git(&[&["add", "--"], kept.as_slice()].concat())?;
        }
        if !gone.is_empty() {
            // That side deleted the file: so is it here.
            self.git(&[&["rm", "-q", "--ignore-unmatch", "--"], gone.as_slice()].concat())?;
        }
        Ok(())
    }

    fn mark_resolved(&self, paths: &[String]) -> Result<(), VcsError> {
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        self.git(&[&["add", "-A", "--"], paths.as_slice()].concat()).map(|_| ())
    }

    fn changed_between(&self, revision: &str, paths: &[String]) -> Result<Vec<String>, VcsError> {
        GitRepository::changed_between(self, revision, paths)
    }

    fn present_in(&self, revision: &str, paths: &[String]) -> Result<Vec<String>, VcsError> {
        GitRepository::present_in(self, revision, paths)
    }

    fn switch(
        &self,
        target: &version_control::SwitchTarget,
        discard: bool,
    ) -> Result<(), VcsError> {
        self.switch_to(target, discard)
    }

    fn stash_push(&self, options: &version_control::StashOptions) -> Result<(), VcsError> {
        GitRepository::stash_push(self, options)
    }

    fn stash_apply(&self, id: &str, pop: bool) -> Result<(), VcsError> {
        GitRepository::stash_apply(self, id, pop)
    }

    fn stash_drop(&self, id: &str) -> Result<(), VcsError> {
        GitRepository::stash_drop(self, id)
    }

    fn stash_branch(&self, id: &str, branch: &str) -> Result<(), VcsError> {
        GitRepository::stash_branch(self, id, branch)
    }

    fn recent_branches(&self, limit: usize) -> Vec<String> {
        let out = self.git(&["reflog", "show", "--format=%gs", "-n", "300", "HEAD"]);
        let current = version_control::Repository::branch(self);
        let locals = version_control::Repository::branches(self);
        version_control::recent_checkouts(&out.unwrap_or_default(), current.as_deref(), limit * 2)
            .into_iter()
            .filter(|b| locals.contains(b))
            .take(limit)
            .collect()
    }

    fn stashes_staged(&self) -> bool {
        GitRepository::stashes_staged(self)
    }

    fn proceed(&self, step: OperationStep) -> Result<Proceeded, VcsError> {
        let operation =
            self.operation().ok_or_else(|| VcsError::Failed("nothing in progress".into()))?;
        let args = operation_args(operation.kind, step)
            .ok_or_else(|| VcsError::Failed(format!("{step:?} not possible")))?;
        let ran = self.git(args);
        let stopped = self.operation().is_some();
        match ran {
            Ok(_) if stopped => Ok(Proceeded::Stopped),
            Ok(_) => Ok(Proceeded::Finished),
            // It stopped on the next commit's conflicts: still in progress, as expected.
            Err(_)
                if stopped && !version_control::application::conflicted_files(self)?.is_empty() =>
            {
                Ok(Proceeded::Stopped)
            }
            Err(error) => Err(error),
        }
    }

    fn refs(&self) -> Vec<RefLabel> {
        let mut refs: Vec<RefLabel> = self
            .labels(&self.local())
            .into_values()
            .flatten()
            .filter(|r| r.kind != RefKind::Head)
            .collect();
        refs.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
        refs.dedup();
        refs
    }

    fn branches(&self) -> Vec<String> {
        let repo = self.local();
        let Ok(references) = repo.references() else { return Vec::new() };
        let Ok(branches) = references.local_branches() else { return Vec::new() };
        let mut names: Vec<String> =
            branches.flatten().map(|r| r.name().shorten().to_str_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
