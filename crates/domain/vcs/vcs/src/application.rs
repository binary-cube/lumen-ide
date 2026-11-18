//! The version control use cases, over the [`Repository`] port: the lines changed in the
//! editor (and reverting one change), a file compared with one of its versions, its history
//! as a list, the blame annotations of its current lines, the log with its graph (in
//! pages), a commit's file compared with its parent, and pushing / pulling. No I/O here
//! besides the port; tested with a fake one.

use std::ops::Range;
use std::path::Path;

use diff::CompareInput;

use crate::{
    BlameHunk, ChangeKind, Commit, CommitDetails, FileChange, FileState, GraphBuilder, GraphRow,
    LineChange, LogEntry, LogFilter, OperationStep, Proceeded, RemoteOutcome, Repository, Revision,
    Stage, VcsError, relative_path,
};

/// The changed lines of `current` against the committed text (`None`: the file isn't in the
/// last commit, so every line is new).
pub fn changed_lines(base: Option<&str>, current: &str, line_count: usize) -> Vec<LineChange> {
    match base {
        Some(base) => crate::line_changes(base, current),
        None => vec![LineChange { lines: 0..line_count, kind: ChangeKind::Added, base: 0..0 }],
    }
}

/// The file at `path` (as in the editor: `current`) compared with its version at
/// `revision`, called `label` (e.g. `HEAD`). The version's side is read-only; writing a
/// composed result goes to the file. `None` if the file isn't in the repository.
pub fn compare_with(
    repo: &dyn Repository,
    path: &Path,
    current: &str,
    revision: &Revision,
    label: &str,
) -> Option<CompareInput> {
    let relative = relative_path(repo.root(), path)?;
    let old = repo.text_at(revision, &relative).unwrap_or_default();
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(CompareInput {
        left: (path.to_path_buf(), old),
        right: (path.to_path_buf(), current.to_owned()),
        labels: [Some(format!("{name} · {label}")), Some(name)],
        read_only: [true, false],
    })
}

/// The commits that changed the file at `path`, newest first.
pub fn file_history(repo: &dyn Repository, path: &Path, limit: usize) -> Vec<Commit> {
    relative_path(repo.root(), path).map(|p| repo.history(&p, limit)).unwrap_or_default()
}

/// Why a commit can't be made as asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitError {
    /// No file is selected.
    NothingSelected,
    /// The message is empty.
    NoMessage,
    Vcs(crate::VcsError),
}

/// Commits the selected files with `message` (trimmed); returns the new commit's id.
pub fn commit_selected(
    repo: &dyn Repository,
    selected: &[String],
    message: &str,
) -> Result<String, CommitError> {
    let message = message.trim();
    if selected.is_empty() {
        return Err(CommitError::NothingSelected);
    }
    if message.is_empty() {
        return Err(CommitError::NoMessage);
    }
    repo.commit(selected, message).map_err(CommitError::Vcs)
}

/// The last commit made again with `message` (trimmed) and the changes of the selected
/// files (none: only its message changes); returns its new id.
pub fn amend_selected(
    repo: &dyn Repository,
    selected: &[String],
    message: &str,
) -> Result<String, CommitError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(CommitError::NoMessage);
    }
    repo.amend(selected, message).map_err(CommitError::Vcs)
}

/// A line's blame annotation: who / when, how old (0 = the newest commit, 1 = the oldest)
/// and whether it starts its commit's block.
#[derive(Debug, Clone, PartialEq)]
pub struct LineAnnotation {
    pub commit: usize,
    pub date: (i64, u32, u32),
    pub author: String,
    pub age: f32,
    pub first: bool,
}

/// The annotation of each current line (`None`: added or changed since the commit).
pub fn blame_annotations(
    changes: &[LineChange],
    blame: &[BlameHunk],
    line_count: usize,
) -> Vec<Option<LineAnnotation>> {
    let lines = crate::blame_lines(changes, blame, line_count);
    let mut times: Vec<i64> = blame.iter().map(|h| h.commit.time).collect();
    times.sort_unstable();
    times.dedup();
    let span = times.len().saturating_sub(1).max(1) as f32;
    let mut previous: Option<&str> = None;
    lines
        .into_iter()
        .map(|hunk| {
            let Some(hunk) = hunk else {
                previous = None;
                return None;
            };
            let commit = &blame[hunk].commit;
            let first = previous != Some(commit.id.as_str());
            previous = Some(commit.id.as_str());
            let index = times.partition_point(|t| *t < commit.time).min(times.len() - 1);
            Some(LineAnnotation {
                commit: hunk,
                date: crate::date(commit.time),
                author: commit.author.clone(),
                age: (times.len() - 1 - index) as f32 / span,
                first,
            })
        })
        .collect()
}

/// Reverting one change: the bytes of the current text to replace, and the committed lines
/// that go back there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revert {
    pub range: Range<usize>,
    pub text: String,
    pub change: LineChange,
}

/// The change at `line` of `current` (its lines, or the place lines were deleted from —
/// the line there or just above it), undone back to `base` (the committed text). `None` if
/// the line didn't change.
pub fn revert_change(base: &str, current: &str, line: usize) -> Option<Revert> {
    // Line ends as in the editor (the committed text may have `\r\n`).
    let base = base.replace("\r\n", "\n");
    let change = crate::line_changes(&base, current).into_iter().find(|c| {
        if c.lines.is_empty() {
            line == c.lines.start || line + 1 == c.lines.start
        } else {
            c.lines.contains(&line)
        }
    })?;
    // Lines with their terminators: `starts[i]` is where line `i` begins.
    let mut starts = vec![0];
    starts.extend(current.match_indices('\n').map(|(i, _)| i + 1));
    let at = |line: usize| starts.get(line).copied().unwrap_or(current.len()).min(current.len());
    let range = at(change.lines.start)..at(change.lines.end);
    let text: String =
        base.split_inclusive('\n').skip(change.base.start).take(change.base.len()).collect();
    // At the end of a file without a final newline: the lines keep that shape (replaced
    // last lines lose theirs; lines deleted after the last one come after a newline).
    let unterminated =
        !current.is_empty() && !current.ends_with('\n') && range.end == current.len();
    let text = match (unterminated, range.is_empty()) {
        (true, false) => text.strip_suffix('\n').map_or(text.clone(), str::to_owned),
        (true, true) => format!("\n{}", text.strip_suffix('\n').unwrap_or(&text)),
        _ => text,
    };
    Some(Revert { range, text, change })
}

/// A log row: the commit and its place in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRow {
    pub entry: LogEntry,
    pub graph: GraphRow,
}

/// The next page of the log as `filter` has it (`limit` commits after the `skip` already
/// shown), continuing the graph `builder` has drawn so far. A filter leaving commits out
/// (a text, an author, a time, paths) draws what it found as one line: each commit's parent
/// is the next one found, as a file's history does.
pub fn log_page(
    repo: &dyn Repository,
    filter: &LogFilter,
    builder: &mut GraphBuilder,
    skip: usize,
    limit: usize,
) -> Vec<LogRow> {
    if filter.keeps_every_commit() {
        return repo
            .log(filter, skip, limit)
            .into_iter()
            .map(|entry| {
                let graph = builder.push(&entry.commit.id, &entry.parents);
                LogRow { entry, graph }
            })
            .collect();
    }
    // One more: the last one's parent (the next page's first).
    let mut found = repo.log(filter, skip, limit + 1);
    let next = (found.len() > limit).then(|| found.pop().map(|e| e.commit.id)).flatten();
    let ids: Vec<String> = found.iter().map(|e| e.commit.id.clone()).skip(1).chain(next).collect();
    found
        .into_iter()
        .zip(ids.into_iter().map(Some).chain(std::iter::repeat(None)))
        .map(|(mut entry, parent)| {
            entry.parents = parent.into_iter().collect();
            let graph = builder.push(&entry.commit.id, &entry.parents);
            LogRow { entry, graph }
        })
        .collect()
}

/// The next page of one file's history (the commits that changed it), as a log: each
/// commit's parent is the next one that changed the file (one straight line).
pub fn file_log_page(
    repo: &dyn Repository,
    path: &str,
    builder: &mut GraphBuilder,
    skip: usize,
    limit: usize,
) -> Vec<LogRow> {
    let commits = repo.history(path, skip + limit + 1);
    let window: Vec<&Commit> = commits.iter().skip(skip).collect();
    window
        .iter()
        .take(limit)
        .enumerate()
        .map(|(i, commit)| {
            let parents: Vec<String> =
                window.get(i + 1).map(|c| c.id.clone()).into_iter().collect();
            let graph = builder.push(&commit.id, &parents);
            let entry = LogEntry { commit: (*commit).clone(), parents, refs: Vec::new() };
            LogRow { entry, graph }
        })
        .collect()
}

/// A file of a commit compared with its version before (in the first parent): both sides
/// read-only. Labels: `a.php · 1a2b3c4^` and `a.php · 1a2b3c4`.
pub fn compare_commit_file(
    repo: &dyn Repository,
    details: &CommitDetails,
    file: &FileChange,
) -> CompareInput {
    let id = &details.commit.id;
    let before_path = file.old_path.as_deref().unwrap_or(&file.path);
    let before = details
        .parents
        .first()
        .and_then(|p| repo.text_at(&Revision::Named(p.clone()), before_path))
        .unwrap_or_default();
    let after = repo.text_at(&Revision::Named(id.clone()), &file.path).unwrap_or_default();
    let name = file.path.rsplit('/').next().unwrap_or(&file.path).to_owned();
    let short = details.commit.short();
    CompareInput {
        left: (repo.root().join(before_path), before),
        right: (repo.root().join(&file.path), after),
        labels: [Some(format!("{name} · {short}^")), Some(format!("{name} · {short}"))],
        read_only: [true, true],
    }
}

/// An operation with the remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOp {
    Fetch,
    Pull,
    Push,
}

/// Runs `op`; a pull needs a branch tracking an upstream (a push publishes one that
/// doesn't).
pub fn sync(repo: &dyn Repository, op: SyncOp) -> Result<RemoteOutcome, VcsError> {
    match op {
        SyncOp::Fetch => repo.fetch(),
        SyncOp::Pull => {
            if repo.sync_state().is_some_and(|s| s.upstream.is_none()) {
                return Err(VcsError::NoRemote);
            }
            repo.pull()
        }
        SyncOp::Push => repo.push(),
    }
}

/// Why a step can't be done with the operation in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProceedError {
    /// Nothing is in progress.
    NothingInProgress,
    /// That operation has no such step (a merge has nothing to skip).
    NotAllowed,
    /// Conflicts are left (their paths): resolve them first.
    Unresolved(Vec<String>),
    Vcs(VcsError),
}

/// The conflicted files (paths relative to the root, with `/`), sorted.
pub fn conflicted_files(repo: &dyn Repository) -> Result<Vec<String>, VcsError> {
    let mut paths: Vec<String> = repo
        .status()?
        .into_iter()
        .filter(|f| f.state == FileState::Conflicted)
        .map(|f| f.path)
        .collect();
    paths.sort();
    Ok(paths)
}

/// `step` done with the operation in progress: allowed for it, and continued only once no
/// conflict is left.
pub fn proceed(repo: &dyn Repository, step: OperationStep) -> Result<Proceeded, ProceedError> {
    let operation = repo.operation().ok_or(ProceedError::NothingInProgress)?;
    if !operation.kind.allows(step) {
        return Err(ProceedError::NotAllowed);
    }
    if step == OperationStep::Continue {
        let left = conflicted_files(repo).map_err(ProceedError::Vcs)?;
        if !left.is_empty() {
            return Err(ProceedError::Unresolved(left));
        }
    }
    repo.proceed(step).map_err(ProceedError::Vcs)
}

/// The versions of a conflicted file: the base's, ours and theirs (`None`: that version
/// hasn't the file — added on both sides, deleted on one).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConflictVersions {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

/// The versions the repository keeps of the conflicted file at `path` (relative, with `/`).
pub fn conflict_versions(repo: &dyn Repository, path: &str) -> ConflictVersions {
    let at = |stage| repo.text_at(&Revision::Conflict(stage), path);
    ConflictVersions { base: at(Stage::Base), ours: at(Stage::Ours), theirs: at(Stage::Theirs) }
}

#[cfg(test)]
#[path = "../tests/unit/application.rs"]
mod tests;
