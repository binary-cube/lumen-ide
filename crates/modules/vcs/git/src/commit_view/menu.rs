//! What a file's menu does: opening it, moving it, throwing its changes away, back to a commit.

use super::*;

/// The rows of a file's menu (the path after the prefix: a menu outlives the list it was
/// opened on, and what it does must reach that file).
pub(super) const OPEN_FILE: &str = "file.open:";
pub(super) const FILE_DIFF: &str = "file.diff:";
pub(super) const COPY_PATH: &str = "file.copy-path:";
pub(super) const SHOW_IN_PROJECT: &str = "file.show-in-project:";
pub(super) const REVEAL_FILE: &str = "file.reveal:";
pub(super) const STAGE_FILE: &str = "file.stage:";
pub(super) const UNSTAGE_FILE: &str = "file.unstage:";
pub(super) const DISCARD_FILE: &str = "file.discard:";
pub(super) const RESET_FILE: &str = "file.reset:";
pub(super) const RESET_TO_COMMIT: &str = "file.reset-to:";
pub(super) const ANNOTATE_FILE: &str = "file.annotate:";

/// How many of a file's commits "Reset to Commit" offers.
pub(super) const COMMITS_OFFERED: usize = 100;

impl CommitView {
    /// File `i` back as it was at the last commit, staged or not.
    pub(super) fn reset_file(&mut self, i: usize) -> Option<ViewerAction> {
        let file = self.files.get(i)?;
        let (path, staged) = (file.path.clone(), file.staged.is_some());
        let question = Question {
            title: tr!("git.reset_title", name = path.as_str()),
            message: t("git.reset_message"),
            icon: "trash",
            yes: t("git.reset_yes"),
            danger: true,
            list: Vec::new(),
        };
        self.asked(question, move |repo| reset(repo, &[path], staged))
    }

    /// `path`'s commits read in the background, offered; the one chosen asked about, then
    /// the file taken back to it.
    pub(super) fn reset_to_commit(&mut self, path: String) {
        let Some(repo) = self.repo.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        self.act_rx = Some(rx);
        self.asking = true;
        self.outcome = None;
        self.opening.start("git-output", move || {
            let commits = repo.history(&path, COMMITS_OFFERED);
            if commits.is_empty() {
                return None;
            }
            let work = Reset { repo, path, tx, waker };
            Some(ViewerAction::WithHost(WithHost::once(move |host: &mut dyn WorkbenchHost| {
                work.offer(host, commits);
            })))
        });
    }

    /// `path` opened with who last changed each line beside it.
    pub(super) fn annotate(&self, path: &str) -> Option<ViewerAction> {
        let file = self.file_path(path)?;
        Some(ViewerAction::WithHost(WithHost::once(move |host: &mut dyn WorkbenchHost| {
            if host.open_file(&file) {
                crate::decorations::show_annotations(host);
            }
        })))
    }

    pub(super) fn file_path(&self, path: &str) -> Option<std::path::PathBuf> {
        Some(self.repo.as_ref()?.root().join(path))
    }

    /// `path` beside its last commit, read in the background.
    pub(super) fn compare(&mut self, path: &str) -> Option<ViewerAction> {
        let (repo, file) = (self.repo.clone()?, self.file_path(path)?);
        self.opening.start("git-output", move || {
            let current = std::fs::read_to_string(&file).unwrap_or_default();
            crate::DISK.read(current.len() as u64);
            let input = compare_with(repo.as_ref(), &file, &current, &Revision::Head, "HEAD")?;
            let name = file.file_name()?.to_string_lossy().into_owned();
            Some(ViewerAction::Compare(Box::new(input), format!("{name} ↔ HEAD")))
        });
        None
    }

    /// The menu of file `i`, on one of its lists.
    pub(super) fn file_menu(&self, i: usize, staged: bool) -> Option<Vec<ViewMenuEntry>> {
        if self.among_marked(i, staged) {
            return Some(self.marked_menu(staged));
        }
        let file = self.files.get(i)?;
        let path = &file.path;
        let state = if staged { file.staged } else { file.unstaged };
        let row = |prefix: &str, label: String, icon: &'static str| {
            ViewMenuEntry::item(format!("{prefix}{path}"), label).with_icon(icon)
        };
        let deleted = state == Some(FileState::Deleted);
        let conflicted = state == Some(FileState::Conflicted);
        let mut rows = Vec::new();
        if !deleted {
            rows.push(row(OPEN_FILE, t("git.open_file"), "file"));
        }
        if workbench_model::presence::compare() {
            rows.push(row(FILE_DIFF, t("git.show_diff_working"), "compare"));
        }
        let new = matches!(state, Some(FileState::Untracked | FileState::Added));
        if !new && !deleted {
            rows.push(row(ANNOTATE_FILE, t("git.annotate_file"), "file-pen"));
        }
        rows.push(ViewMenuEntry::Separator);
        rows.push(row(COPY_PATH, t("git.copy_path"), "link"));
        if !deleted {
            rows.push(row(SHOW_IN_PROJECT, t("git.show_in_project"), "crosshair"));
            rows.push(row(REVEAL_FILE, t("git.reveal_file"), "folder-open"));
        }
        rows.push(ViewMenuEntry::Separator);
        rows.push(if staged {
            row(UNSTAGE_FILE, t("git.unstage_file"), "x")
        } else {
            row(STAGE_FILE, t("git.stage_file"), "plus")
        });
        if !conflicted {
            if !staged {
                rows.push(row(DISCARD_FILE, t("git.discard_file"), "trash"));
            }
            rows.push(row(RESET_FILE, t("git.reset_file"), "trash"));
            if !new {
                rows.push(row(RESET_TO_COMMIT, t("git.reset_to_commit"), "git-commit"));
            }
        }
        Some(rows)
    }

    /// The menu of several files chosen on a list: what can reach them all.
    fn marked_menu(&self, staged: bool) -> Vec<ViewMenuEntry> {
        let paths =
            self.marked.iter().map(String::as_str).collect::<Vec<_>>().join(PATHS_SEPARATOR);
        let row = |prefix: &str, label: String, icon: &'static str| {
            ViewMenuEntry::item(format!("{prefix}{paths}"), label).with_icon(icon)
        };
        let conflicted = self.files.iter().any(|f| {
            self.marked.contains(&f.path)
                && (if staged { f.staged } else { f.unstaged }) == Some(FileState::Conflicted)
        });
        let mut rows = vec![
            row(COPY_PATH, t("git.copy_paths"), "link"),
            ViewMenuEntry::Separator,
            if staged {
                row(UNSTAGE_FILE, t("git.unstage_file"), "x")
            } else {
                row(STAGE_FILE, t("git.stage_file"), "plus")
            },
        ];
        if !conflicted {
            if !staged {
                rows.push(row(DISCARD_FILE, t("git.discard_file"), "trash"));
            }
            rows.push(row(RESET_FILE, t("git.reset_file"), "trash"));
        }
        rows
    }

    /// Several files' changes thrown away, once answered.
    fn discard_marked(&mut self, paths: Vec<String>) -> Option<ViewerAction> {
        let question = Question {
            title: tr!("git.discard_many_title", count = paths.len() as i64),
            message: t("git.discard_message"),
            icon: "trash",
            yes: t("git.discard_yes"),
            danger: true,
            list: Vec::new(),
        };
        self.asked(question, move |repo| repo.discard(&paths))
    }

    /// Several files back as they were at the last commit, once answered.
    fn reset_marked(&mut self, paths: Vec<String>) -> Option<ViewerAction> {
        let staged = self.files.iter().any(|f| f.staged.is_some() && paths.contains(&f.path));
        let question = Question {
            title: tr!("git.reset_many_title", count = paths.len() as i64),
            message: t("git.reset_message"),
            icon: "trash",
            yes: t("git.reset_yes"),
            danger: true,
            list: Vec::new(),
        };
        self.asked(question, move |repo| reset(repo, &paths, staged))
    }

    /// A row of the file's menu done. `false`: `id` isn't one.
    pub(super) fn file_command(&mut self, id: &str) -> bool {
        let of = |prefix: &str| id.strip_prefix(prefix).map(str::to_owned);
        let at = |view: &Self, path: &str| view.files.iter().position(|f| f.path == path);
        self.pending = if let Some(path) = of(OPEN_FILE) {
            self.file_path(&path).map(|path| ViewerAction::OpenFile {
                path,
                line: None,
                column: None,
            })
        } else if let Some(path) = of(FILE_DIFF) {
            self.compare(&path)
        } else if let Some(path) = of(COPY_PATH) {
            // Several: one per line.
            let text = split_paths(&path).join("\n");
            Some(ViewerAction::Request(AppRequest::CopyText(text)))
        } else if let Some(path) = of(SHOW_IN_PROJECT) {
            self.file_path(&path).map(|p| ViewerAction::Request(AppRequest::ShowFolder(p)))
        } else if let Some(path) = of(REVEAL_FILE) {
            self.file_path(&path).map(|p| ViewerAction::Request(AppRequest::Reveal(p)))
        } else if let Some(path) = of(STAGE_FILE) {
            let paths = split_paths(&path);
            self.act(move |repo| repo.stage(&paths));
            None
        } else if let Some(path) = of(UNSTAGE_FILE) {
            let paths = split_paths(&path);
            self.act(move |repo| repo.unstage(&paths));
            None
        } else if let Some(path) = of(DISCARD_FILE) {
            let paths = split_paths(&path);
            if paths.len() > 1 {
                self.discard_marked(paths)
            } else {
                at(self, &path).and_then(|i| self.discard_file(i))
            }
        } else if let Some(path) = of(RESET_FILE) {
            let paths = split_paths(&path);
            if paths.len() > 1 {
                self.reset_marked(paths)
            } else {
                at(self, &path).and_then(|i| self.reset_file(i))
            }
        } else if let Some(path) = of(RESET_TO_COMMIT) {
            self.reset_to_commit(path);
            None
        } else if let Some(path) = of(ANNOTATE_FILE) {
            self.annotate(&path)
        } else {
            return false;
        };
        true
    }

    pub(super) fn discard_file(&mut self, i: usize) -> Option<ViewerAction> {
        let path = self.files.get(i)?.path.clone();
        let question = Question {
            title: tr!("git.discard_title", name = path.as_str()),
            message: t("git.discard_message"),
            icon: "trash",
            yes: t("git.discard_yes"),
            danger: true,
            list: Vec::new(),
        };
        self.asked(question, move |repo| repo.discard(&[path]))
    }

    pub(super) fn discard_hunk(&mut self, i: usize) -> Option<ViewerAction> {
        let patch = self.hunk_patch(i, true)?;
        let diff = self.diff.as_ref()?;
        let question = Question {
            title: tr!("git.discard_hunk_title", name = diff.path.as_str()),
            message: t("git.discard_message"),
            icon: "trash",
            yes: t("git.discard_yes"),
            danger: true,
            list: Vec::new(),
        };
        self.asked(question, move |repo| repo.apply_patch(&patch, PatchTarget::WorkingTree, true))
    }
}

/// Between the paths of a menu row reaching several files (no path can hold it).
const PATHS_SEPARATOR: &str = "\0";

fn split_paths(paths: &str) -> Vec<String> {
    paths.split(PATHS_SEPARATOR).map(str::to_owned).collect()
}

/// A file on its way back to one of its commits.
struct Reset {
    repo: Arc<dyn Repository>,
    path: String,
    tx: Sender<Done>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Reset {
    /// `commits` offered; dropping out at any step tells the view nothing was done.
    fn offer(self, host: &mut dyn WorkbenchHost, commits: Vec<Commit>) {
        let list = ChoiceList {
            title: t("git.reset_to_commit_title"),
            subject: self.path.clone(),
            icon: "git-commit",
            action: t("git.reset_yes"),
            items: commits
                .iter()
                .map(|commit| ChoiceItem {
                    icon: "git-commit",
                    label: commit.summary.clone(),
                    detail: format!("{} · {}", commit.short(), commit.author),
                })
                .collect(),
        };
        host.choose(
            list,
            Box::new(move |chosen, host| {
                if let Some(commit) = commits.get(chosen) {
                    self.ask(host, commit);
                }
            }),
        );
    }

    fn ask(self, host: &mut dyn WorkbenchHost, commit: &Commit) {
        let question = Question {
            title: tr!("git.reset_title", name = self.path.as_str()),
            message: tr!("git.reset_to_commit_message", commit = commit.short()),
            icon: "trash",
            yes: t("git.reset_yes"),
            danger: true,
            list: Vec::new(),
        };
        let id = commit.id.clone();
        host.confirm(
            question,
            Box::new(move |_: &mut dyn WorkbenchHost| {
                kernel::work::run("git-commit", move || {
                    let Self { repo, path, tx, waker } = self;
                    finish(&*repo, move |r| r.reset_to(&[path], &id), &tx, waker.as_ref());
                });
            }),
        );
    }
}

/// `paths` back as they were at the last commit (`staged`: some of it is on the staging area).
/// A file the last commit doesn't have is deleted.
pub(super) fn reset(repo: &dyn Repository, paths: &[String], staged: bool) -> Done {
    if staged {
        repo.unstage(paths)?;
    }
    repo.discard(paths)
}
