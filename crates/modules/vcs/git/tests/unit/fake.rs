//! A repository in memory for the module's tests: a status, a log, commit details, a
//! branch state, and remote operations answering what they're told to.

use std::path::Path;
use std::sync::Mutex;

use version_control::{
    BlameHunk, Commit, CommitDetails, FileState, FileStatus, LogEntry, LogFilter, RefLabel,
    RemoteOutcome, Repository, Revision, SyncState, TextMatcher, VcsError,
};

#[derive(Debug, Default)]
pub struct FakeRepository {
    pub committed: Mutex<Vec<(Vec<String>, String)>>,
    pub log: Vec<LogEntry>,
    pub details: Vec<CommitDetails>,
    pub texts: Vec<((String, String), String)>,
    pub sync: Option<SyncState>,
    pub refs: Vec<RefLabel>,
    /// What push / pull / fetch return (`None`: up to date).
    pub remote_error: Option<VcsError>,
    /// `HEAD`'s id (`None`: no commit yet) and its branch (`main` when `None`).
    pub head: Mutex<Option<String>>,
    pub head_branch: Mutex<Option<String>>,
    /// The files' texts at `HEAD`: (path, text).
    pub committed_texts: Vec<(String, String)>,
    /// What blame says of every file.
    pub blame: Vec<BlameHunk>,
    /// The authors, who commits here, the files tracked, the branches against their
    /// upstreams.
    pub authors: Vec<String>,
    pub me: Option<String>,
    pub files: Vec<String>,
    pub branch_states: Vec<SyncState>,
    /// The last commit's message, whether it was pushed, what was amended (paths, message).
    pub last_message: Option<String>,
    pub pushed: bool,
    pub amended: Mutex<Vec<(Vec<String>, String)>>,
    /// The conflicted files (with the two changed ones) and the operation in progress.
    pub conflicted: Mutex<Vec<String>>,
    pub operation: Option<version_control::Operation>,
    /// The sides taken: (paths, side).
    pub taken: Mutex<Vec<(Vec<String>, version_control::Side)>>,
    /// The files marked resolved, and the working tree's root (`/p` when `None`).
    pub resolved: Mutex<Vec<Vec<String>>>,
    pub root: Option<std::path::PathBuf>,
    /// The staging area: the files on it, their texts there, the patches applied.
    pub staged: Mutex<Vec<String>>,
    pub discarded: Mutex<Vec<String>>,
    /// The commits of every file.
    pub file_history: Vec<Commit>,
    pub index_texts: Vec<(String, String)>,
    pub patches: Mutex<Vec<(String, version_control::PatchTarget, bool)>>,
}

pub fn commit(id: &str, summary: &str) -> Commit {
    Commit { id: id.into(), author: "Ana".into(), time: 1_700_000_000, summary: summary.into() }
}

pub fn entry(id: &str, parents: &[&str]) -> LogEntry {
    LogEntry {
        commit: commit(id, id),
        parents: parents.iter().map(|p| (*p).to_owned()).collect(),
        refs: Vec::new(),
    }
}

impl FakeRepository {
    fn remote(&self) -> Result<RemoteOutcome, VcsError> {
        match &self.remote_error {
            Some(error) => Err(error.clone()),
            None => Ok(RemoteOutcome { up_to_date: true, message: String::new() }),
        }
    }
}

impl Repository for FakeRepository {
    fn root(&self) -> &Path {
        self.root.as_deref().unwrap_or(Path::new("/p"))
    }
    fn branch(&self) -> Option<String> {
        Some(self.head_branch.lock().unwrap().clone().unwrap_or_else(|| "main".into()))
    }
    fn head_id(&self) -> Option<String> {
        self.head.lock().unwrap().clone()
    }
    fn text_at(&self, revision: &Revision, path: &str) -> Option<String> {
        if *revision == Revision::Head {
            return self.committed_texts.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone());
        }
        if *revision == Revision::Index {
            return self.index_texts.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone());
        }
        let Revision::Named(rev) = revision else { return None };
        self.texts.iter().find(|((r, p), _)| r == rev && p == path).map(|(_, t)| t.clone())
    }
    fn history(&self, _: &str, _: usize) -> Vec<Commit> {
        self.file_history.clone()
    }
    fn blame(&self, _: &str) -> Vec<BlameHunk> {
        self.blame.clone()
    }
    fn branches(&self) -> Vec<String> {
        Vec::new()
    }
    fn staged_files(&self) -> Result<Vec<version_control::StagedFile>, VcsError> {
        let staged = self.staged.lock().unwrap().clone();
        let files = self.status()?;
        Ok(files
            .into_iter()
            .map(|f| {
                let on = staged.contains(&f.path);
                version_control::StagedFile {
                    path: f.path,
                    staged: on.then_some(f.state),
                    unstaged: (!on).then_some(f.state),
                }
            })
            .collect())
    }
    fn stage(&self, paths: &[String]) -> Result<(), VcsError> {
        self.staged.lock().unwrap().extend_from_slice(paths);
        Ok(())
    }
    fn unstage(&self, paths: &[String]) -> Result<(), VcsError> {
        self.staged.lock().unwrap().retain(|p| !paths.contains(p));
        Ok(())
    }
    fn discard(&self, paths: &[String]) -> Result<(), VcsError> {
        self.discarded.lock().unwrap().extend_from_slice(paths);
        Ok(())
    }
    fn apply_patch(
        &self,
        patch: &str,
        target: version_control::PatchTarget,
        reverse: bool,
    ) -> Result<(), VcsError> {
        self.patches.lock().unwrap().push((patch.to_owned(), target, reverse));
        Ok(())
    }
    fn commit_staged(&self, message: &str) -> Result<String, VcsError> {
        let paths = std::mem::take(&mut *self.staged.lock().unwrap());
        self.committed.lock().unwrap().push((paths, message.to_owned()));
        Ok("new".into())
    }
    fn status(&self) -> Result<Vec<FileStatus>, VcsError> {
        let mut files = vec![
            FileStatus { path: "a.php".into(), state: FileState::Modified },
            FileStatus { path: "b.php".into(), state: FileState::Untracked },
        ];
        files.extend(
            self.conflicted
                .lock()
                .unwrap()
                .iter()
                .map(|path| FileStatus { path: path.clone(), state: FileState::Conflicted }),
        );
        Ok(files)
    }
    fn operation(&self) -> Option<version_control::Operation> {
        self.operation.clone()
    }
    fn mark_resolved(&self, paths: &[String]) -> Result<(), VcsError> {
        self.conflicted.lock().unwrap().retain(|p| !paths.contains(p));
        self.resolved.lock().unwrap().push(paths.to_vec());
        Ok(())
    }
    fn take_side(&self, paths: &[String], side: version_control::Side) -> Result<(), VcsError> {
        self.conflicted.lock().unwrap().retain(|p| !paths.contains(p));
        self.taken.lock().unwrap().push((paths.to_vec(), side));
        Ok(())
    }
    fn commit(&self, paths: &[String], message: &str) -> Result<String, VcsError> {
        self.committed.lock().unwrap().push((paths.to_vec(), message.to_owned()));
        Ok("abcdef0123".into())
    }
    fn log(&self, filter: &LogFilter, skip: usize, limit: usize) -> Vec<LogEntry> {
        // A reference's log: from its commit on.
        let from = filter.references.first().map_or(0, |name| {
            self.log
                .iter()
                .position(|e| e.refs.iter().any(|r| r.name == *name))
                .unwrap_or(self.log.len())
        });
        let text = filter.text.as_ref().and_then(|t| TextMatcher::new(t).ok().flatten());
        self.log
            .iter()
            .skip(from)
            .filter(|e| filter.matches(text.as_ref(), &e.commit, "", &e.commit.summary))
            .skip(skip)
            .take(limit)
            .cloned()
            .collect()
    }
    fn refs(&self) -> Vec<RefLabel> {
        self.refs.clone()
    }
    fn authors(&self, _: usize) -> Vec<String> {
        self.authors.clone()
    }
    fn user_name(&self) -> Option<String> {
        self.me.clone()
    }
    fn tracked_files(&self) -> Vec<String> {
        self.files.clone()
    }
    fn branch_states(&self) -> Vec<SyncState> {
        self.branch_states.clone()
    }
    fn last_commit_message(&self) -> Option<String> {
        self.last_message.clone()
    }
    fn head_pushed(&self) -> bool {
        self.pushed
    }
    fn amend(&self, paths: &[String], message: &str) -> Result<String, VcsError> {
        self.amended.lock().unwrap().push((paths.to_vec(), message.to_owned()));
        Ok("amended".into())
    }
    fn commit_details(&self, id: &str) -> Option<CommitDetails> {
        self.details.iter().find(|d| d.commit.id == id).cloned()
    }
    fn sync_state(&self) -> Option<SyncState> {
        self.sync.clone()
    }
    fn fetch(&self) -> Result<RemoteOutcome, VcsError> {
        self.remote()
    }
    fn pull(&self) -> Result<RemoteOutcome, VcsError> {
        self.remote()
    }
    fn push(&self) -> Result<RemoteOutcome, VcsError> {
        self.remote()
    }
}

/// A project in `repo` (at `/p`) with `a.php` open as `text`, its decorations opened and
/// brought up to date (the committed text read, the changed lines found).
pub struct Decorated {
    pub decorations: Box<dyn workbench_model::ProjectDecorations>,
    pub project: kernel::ProjectId,
    pub doc: kernel::DocumentId,
    pub text: document::TextBuffer,
    pub version: u64,
    pub events: kernel::EventBus,
}

impl Decorated {
    pub fn new(repo: impl Into<std::sync::Arc<FakeRepository>>, text: &str) -> Self {
        use workbench_model::LineDecorator;
        let events = kernel::EventBus::default();
        let project = kernel::ProjectId::new();
        let context = workbench_model::DecoratorContext {
            id: project,
            root: Path::new("/p"),
            events: &events,
            wake: std::sync::Arc::new(|| {}),
            repository: Some(repo.into()),
        };
        let decorations = crate::decorations::GitDecorator.open(&context).expect("a repository");
        let mut decorated = Self {
            decorations,
            project,
            doc: kernel::DocumentId::next(),
            text: document::TextBuffer::new(text),
            version: 1,
            events,
        };
        decorated.settle();
        decorated
    }

    /// Asks for what's missing and waits for it.
    pub fn settle(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let shown = workbench_model::ShownDocument {
                id: self.doc,
                path: Path::new("/p/a.php"),
                version: self.version,
                text: &self.text,
            };
            self.decorations.sync(&shown);
            self.decorations.poll();
            self.decorations.sync(&shown);
            if self.decorations.settled(self.doc, self.version) {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "the decorations never settled");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
