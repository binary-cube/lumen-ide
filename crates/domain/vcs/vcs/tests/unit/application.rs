use std::collections::HashMap;
use std::path::PathBuf;

use super::*;

/// A repository in memory: texts per (revision, path) and a history.
#[derive(Debug, Default)]
struct FakeRepository {
    root: PathBuf,
    texts: HashMap<(String, String), String>,
    commits: Vec<Commit>,
    /// What was committed: (paths, message).
    committed: std::sync::Mutex<Vec<(Vec<String>, String)>>,
    log: Vec<LogEntry>,
    sync: Option<crate::SyncState>,
    status: Vec<crate::FileStatus>,
    operation: Option<crate::Operation>,
    /// The steps done with the operation.
    proceeded: std::sync::Mutex<Vec<OperationStep>>,
}

fn key(revision: &Revision) -> String {
    match revision {
        Revision::Head => "HEAD".into(),
        Revision::Index => "INDEX".into(),
        Revision::Named(name) => name.clone(),
        Revision::Conflict(stage) => format!("{stage:?}"),
    }
}

impl Repository for FakeRepository {
    fn root(&self) -> &Path {
        &self.root
    }
    fn branch(&self) -> Option<String> {
        Some("main".into())
    }
    fn head_id(&self) -> Option<String> {
        Some("head".into())
    }
    fn text_at(&self, revision: &Revision, path: &str) -> Option<String> {
        self.texts.get(&(key(revision), path.to_owned())).cloned()
    }
    fn history(&self, _path: &str, limit: usize) -> Vec<Commit> {
        self.commits.iter().take(limit).cloned().collect()
    }
    fn blame(&self, _path: &str) -> Vec<BlameHunk> {
        Vec::new()
    }
    fn branches(&self) -> Vec<String> {
        vec!["main".into()]
    }
    fn status(&self) -> Result<Vec<crate::FileStatus>, crate::VcsError> {
        Ok(self.status.clone())
    }
    fn operation(&self) -> Option<crate::Operation> {
        self.operation.clone()
    }
    fn proceed(&self, step: OperationStep) -> Result<Proceeded, VcsError> {
        self.proceeded.lock().unwrap().push(step);
        Ok(Proceeded::Finished)
    }
    fn commit(&self, paths: &[String], message: &str) -> Result<String, crate::VcsError> {
        self.committed.lock().unwrap().push((paths.to_vec(), message.to_owned()));
        Ok("new".into())
    }
    fn log(&self, filter: &crate::LogFilter, skip: usize, limit: usize) -> Vec<LogEntry> {
        let text = filter.text.as_ref().and_then(|t| crate::TextMatcher::new(t).ok().flatten());
        self.log
            .iter()
            .filter(|e| filter.matches(text.as_ref(), &e.commit, "", &e.commit.summary))
            .skip(skip)
            .take(limit)
            .cloned()
            .collect()
    }
    fn refs(&self) -> Vec<crate::RefLabel> {
        Vec::new()
    }
    fn commit_details(&self, _id: &str) -> Option<CommitDetails> {
        None
    }
    fn sync_state(&self) -> Option<crate::SyncState> {
        self.sync.clone()
    }
    fn fetch(&self) -> Result<RemoteOutcome, VcsError> {
        Ok(RemoteOutcome { up_to_date: true, message: "fetched".into() })
    }
    fn pull(&self) -> Result<RemoteOutcome, VcsError> {
        Ok(RemoteOutcome { up_to_date: false, message: "pulled".into() })
    }
    fn push(&self) -> Result<RemoteOutcome, VcsError> {
        Err(VcsError::ToolMissing("git".into()))
    }
}

fn commit(id: &str, time: i64) -> Commit {
    Commit { id: id.into(), author: format!("author {id}"), time, summary: id.into() }
}

#[test]
fn compares_a_file_with_a_read_only_version() {
    let mut repo = FakeRepository { root: "/p".into(), ..Default::default() };
    repo.texts.insert(("HEAD".into(), "src/a.php".into()), "old\n".into());
    let input =
        compare_with(&repo, Path::new("/p/src/a.php"), "new\n", &Revision::Head, "HEAD").unwrap();
    assert_eq!(input.left.1, "old\n");
    assert_eq!(input.right.1, "new\n");
    assert_eq!(input.labels[0].as_deref(), Some("a.php · HEAD"));
    assert_eq!(input.read_only, [true, false]);
    assert!(
        compare_with(&repo, Path::new("/elsewhere/a.php"), "", &Revision::Head, "HEAD").is_none()
    );
}

#[test]
fn new_files_are_all_added_and_history_is_limited() {
    assert_eq!(
        changed_lines(None, "a\nb\n", 2),
        [LineChange { lines: 0..2, kind: ChangeKind::Added, base: 0..0 }]
    );
    let repo = FakeRepository {
        root: "/p".into(),
        commits: vec![commit("c3", 3), commit("c2", 2), commit("c1", 1)],
        ..Default::default()
    };
    assert_eq!(file_history(&repo, Path::new("/p/a.php"), 2).len(), 2);
    assert!(file_history(&repo, Path::new("/x/a.php"), 2).is_empty());
}

#[test]
fn blame_annotations_rank_commits_by_age() {
    let blame = vec![
        BlameHunk { lines: 0..2, commit: commit("old", 100) },
        BlameHunk { lines: 2..3, commit: commit("new", 200) },
    ];
    let changes = crate::line_changes("a\nb\nc\n", "a\nb\nc\nd\n");
    let lines = blame_annotations(&changes, &blame, 4);
    let (a, b, c) =
        (lines[0].clone().unwrap(), lines[1].clone().unwrap(), lines[2].clone().unwrap());
    assert!(a.first && !b.first && c.first);
    assert_eq!((a.age, c.age), (1.0, 0.0));
    assert_eq!(a.author, "author old");
    assert!(lines[3].is_none(), "the added line has no blame");
}

#[test]
fn commits_need_files_and_a_message() {
    let repo = FakeRepository::default();
    assert_eq!(commit_selected(&repo, &[], "msg"), Err(CommitError::NothingSelected));
    assert_eq!(commit_selected(&repo, &["a".into()], "  \n"), Err(CommitError::NoMessage));
    assert_eq!(commit_selected(&repo, &["a".into()], " Fix it \n"), Ok("new".into()));
    assert_eq!(*repo.committed.lock().unwrap(), [(vec!["a".to_owned()], "Fix it".to_owned())]);
}

#[test]
fn reverting_a_change_brings_the_committed_lines_back() {
    let base = "a\r\nb\r\nc\r\nd\r\n";
    // Modified.
    let current = "a\nB\nc\nd\n";
    let r = revert_change(base, current, 1).unwrap();
    assert_eq!((r.range.clone(), r.text.as_str()), (2..4, "b\n"));
    assert_eq!(r.change.kind, ChangeKind::Modified);
    // Added: removed again.
    let current = "a\nb\nnew\nc\nd\n";
    let r = revert_change(base, current, 2).unwrap();
    assert_eq!((r.range.clone(), r.text.as_str()), (4..8, ""));
    // Deleted: put back, from the line there or the one above.
    let current = "a\nd\n";
    for line in [0, 1] {
        let r = revert_change(base, current, line).unwrap();
        assert_eq!((r.range.clone(), r.text.as_str()), (2..2, "b\nc\n"));
    }
    // An unchanged line has nothing to revert.
    assert!(revert_change(base, "a\nB\nc\nd\n", 3).is_none());
}

#[test]
fn reverting_at_the_end_of_a_file_without_a_final_newline() {
    let r = revert_change("a\nb\n", "a\nX", 1).unwrap();
    assert_eq!((r.range.clone(), r.text.as_str()), (2..3, "b"));
    let r = revert_change("a\nb\nc\n", "a", 0).unwrap();
    assert_eq!((r.range.clone(), r.text.as_str()), (1..1, "\nb\nc"));
}

fn entry(id: &str, parents: &[&str]) -> LogEntry {
    LogEntry {
        commit: commit(id, 0),
        parents: parents.iter().map(|p| (*p).to_owned()).collect(),
        refs: Vec::new(),
    }
}

#[test]
fn the_log_comes_in_pages_with_its_graph() {
    let repo = FakeRepository {
        log: vec![entry("m", &["a", "f"]), entry("f", &["a"]), entry("a", &[])],
        ..Default::default()
    };
    let mut graph = GraphBuilder::default();
    let all = crate::LogFilter::default();
    let first = log_page(&repo, &all, &mut graph, 0, 2);
    let rest = log_page(&repo, &all, &mut graph, 2, 2);
    assert_eq!(first.len(), 2);
    assert_eq!(first[1].graph.column, 1);
    assert_eq!(rest[0].graph.from_above, [0, 1]);
}

/// A filter leaving commits out draws what it found as one line, page after page.
#[test]
fn a_filtered_log_is_one_line() {
    let repo = FakeRepository {
        log: vec![entry("m", &["a", "f"]), entry("f", &["a"]), entry("x", &["a"]), entry("a", &[])],
        ..Default::default()
    };
    let text = crate::TextFilter { text: "^[mfa]$".into(), regex: true, match_case: false };
    let filter = crate::LogFilter { text: Some(text), ..Default::default() };
    let mut graph = GraphBuilder::default();
    let first = log_page(&repo, &filter, &mut graph, 0, 2);
    let rest = log_page(&repo, &filter, &mut graph, 2, 2);
    let ids: Vec<&str> = first.iter().chain(&rest).map(|r| r.entry.commit.id.as_str()).collect();
    assert_eq!(ids, ["m", "f", "a"]);
    assert_eq!(first[0].entry.parents, ["f"]);
    assert_eq!(first[1].entry.parents, ["a"], "the next page's first");
    assert!(rest[0].entry.parents.is_empty());
    assert!(first.iter().chain(&rest).all(|r| r.graph.column == 0), "one lane");
}

#[test]
fn a_commit_file_is_compared_with_its_parent() {
    let mut repo = FakeRepository { root: "/p".into(), ..Default::default() };
    repo.texts.insert(("p1".into(), "old/a.php".into()), "before".into());
    repo.texts.insert(("c1abcdef".into(), "src/a.php".into()), "after".into());
    let details = CommitDetails {
        commit: commit("c1abcdef", 0),
        email: String::new(),
        message: String::new(),
        parents: vec!["p1".into()],
        files: Vec::new(),
    };
    let file = FileChange {
        path: "src/a.php".into(),
        old_path: Some("old/a.php".into()),
        state: crate::FileState::Renamed,
    };
    let input = compare_commit_file(&repo, &details, &file);
    assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("before", "after"));
    assert_eq!(input.labels[1].as_deref(), Some("a.php · c1abcde"));
    assert_eq!(input.read_only, [true, true]);
}

#[test]
fn pulling_needs_an_upstream_and_errors_come_through() {
    let repo = FakeRepository {
        sync: Some(crate::SyncState { branch: "main".into(), upstream: None, ahead: 1, behind: 0 }),
        ..Default::default()
    };
    assert_eq!(sync(&repo, SyncOp::Pull), Err(VcsError::NoRemote));
    assert_eq!(sync(&repo, SyncOp::Push), Err(VcsError::ToolMissing("git".into())));
    assert!(sync(&repo, SyncOp::Fetch).unwrap().up_to_date);
}

#[test]
fn a_file_history_is_one_straight_line_in_pages() {
    let repo = FakeRepository {
        commits: vec![commit("c3", 3), commit("c2", 2), commit("c1", 1)],
        ..Default::default()
    };
    let mut graph = GraphBuilder::default();
    let first = file_log_page(&repo, "a.php", &mut graph, 0, 2);
    assert_eq!(first[0].entry.parents, ["c2"]);
    assert_eq!(first[1].entry.parents, ["c1"], "the next page's first commit");
    let rest = file_log_page(&repo, "a.php", &mut graph, 2, 2);
    assert_eq!(rest.len(), 1);
    assert!(rest[0].entry.parents.is_empty());
    assert!(first.iter().chain(&rest).all(|r| r.graph.column == 0));
}

fn conflicted(path: &str) -> crate::FileStatus {
    crate::FileStatus { path: path.into(), state: crate::FileState::Conflicted }
}

fn in_progress(kind: crate::OperationKind) -> Option<crate::Operation> {
    Some(crate::Operation { kind, branch: None, step: None })
}

#[test]
fn an_operation_continues_only_once_no_conflict_is_left() {
    let mut repo = FakeRepository {
        status: vec![
            conflicted("b.php"),
            crate::FileStatus { path: "c.php".into(), state: crate::FileState::Modified },
            conflicted("a.php"),
        ],
        operation: in_progress(crate::OperationKind::Rebase),
        ..Default::default()
    };
    assert_eq!(conflicted_files(&repo).unwrap(), ["a.php", "b.php"]);
    assert_eq!(
        proceed(&repo, OperationStep::Continue),
        Err(ProceedError::Unresolved(vec!["a.php".into(), "b.php".into()]))
    );
    // Skipping or giving up doesn't wait for them.
    assert_eq!(proceed(&repo, OperationStep::Skip), Ok(Proceeded::Finished));
    repo.status.retain(|f| f.state != crate::FileState::Conflicted);
    assert_eq!(proceed(&repo, OperationStep::Continue), Ok(Proceeded::Finished));
    assert_eq!(*repo.proceeded.lock().unwrap(), [OperationStep::Skip, OperationStep::Continue]);
}

#[test]
fn a_step_needs_an_operation_that_has_it() {
    let mut repo = FakeRepository::default();
    assert_eq!(proceed(&repo, OperationStep::Abort), Err(ProceedError::NothingInProgress));
    repo.operation = in_progress(crate::OperationKind::Merge);
    assert_eq!(proceed(&repo, OperationStep::Skip), Err(ProceedError::NotAllowed));
    assert_eq!(proceed(&repo, OperationStep::Abort), Ok(Proceeded::Finished));
}

#[test]
fn a_conflicted_file_has_its_three_versions() {
    let mut repo = FakeRepository::default();
    repo.texts.insert(("Ours".into(), "a.php".into()), "ours".into());
    repo.texts.insert(("Theirs".into(), "a.php".into()), "theirs".into());
    let versions = conflict_versions(&repo, "a.php");
    // Added on both sides: no base.
    assert_eq!(
        versions,
        ConflictVersions { base: None, ours: Some("ours".into()), theirs: Some("theirs".into()) }
    );
}
