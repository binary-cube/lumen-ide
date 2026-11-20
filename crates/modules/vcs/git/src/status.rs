//! The working tree's changed files, per project: read once for both the Git button's count
//! and the "Changes" tab. Read in the background (one read at a time on a pool's thread,
//! gone when idle), again when the project's files change (a save, the disk, a batch of
//! changes) or `HEAD` moves (a commit, a checkout); the requests close together are taken as
//! one, a read asked during another made after it. The operation in progress (a rebase, a
//! merge) is read with them; conflicts appearing are told once (a notification leading to
//! the Conflicts tab), the button turns to a warning while they last.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use kernel::work::{self, Pool, Priority};
use kernel::{EventBus, ProjectId, Subscription};
use version_control::{FileState, FileStatus, Operation, Repository};
use workbench_model::{ProjectContext, ProjectService, ToolBadge, WorkbenchHost};

/// How long a read waits for the requests to calm down (a save, a formatter after it).
const QUIET: Duration = Duration::from_millis(300);

/// Each open project's changes (held by its subscriptions: gone with the project).
static PROJECTS: workbench_model::PerProject<Changes> = workbench_model::PerProject::new();

/// A project's changed files, as last read.
#[derive(Default)]
pub struct Changes {
    state: Mutex<Read>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Its reads, one at a time (this project's: another's never waits for them).
    reads: std::sync::OnceLock<Pool>,
}

impl std::fmt::Debug for Changes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Changes").field("state", &self.state).finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct Read {
    files: Arc<Vec<FileStatus>>,
    /// The operation in progress, as last read.
    operation: Option<Operation>,
    /// The conflicts were told (a notification); again once they're gone and come back.
    told: bool,
    /// Bumped at each read (0: never read yet).
    generation: u64,
    reading: bool,
    /// Asked to read again (the files changed since the last read started).
    stale: bool,
    /// The count the button shows now.
    shown: Option<(usize, bool)>,
    /// git refused to run: the repository's own settings run programs (those lines), not
    /// trusted yet. Asked once per opening.
    untrusted: Option<Vec<String>>,
    asked: bool,
    /// The batches of changes running (their identities): no read until they end.
    held: Vec<u128>,
    /// The conflicted files open in a three-part merge (on disk): marked resolved once its
    /// result is saved, with the repository to do it in.
    resolving: Vec<(std::path::PathBuf, Arc<dyn Repository>)>,
}

impl Changes {
    fn lock(&self) -> std::sync::MutexGuard<'_, Read> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The files as last read, and their generation (0: not read yet).
    pub fn files(&self) -> (u64, Arc<Vec<FileStatus>>) {
        let read = self.lock();
        (read.generation, Arc::clone(&read.files))
    }

    /// The operation in progress, as last read.
    pub fn operation(&self) -> Option<Operation> {
        self.lock().operation.clone()
    }

    /// The conflicted files, as last read.
    pub fn conflicted(&self) -> Vec<String> {
        conflicted(&self.lock().files)
    }

    /// Conflicted file `path` (on disk) marked resolved in `repo` when it's next saved
    /// without conflict markers (its three-part merge's result).
    pub fn resolve_on_save(&self, path: std::path::PathBuf, repo: Arc<dyn Repository>) {
        let mut read = self.lock();
        read.resolving.retain(|(p, _)| *p != path);
        read.resolving.push((path, repo));
    }

    /// `path` was saved: if it was being resolved and has no conflict markers left, it's
    /// marked resolved (in the background), the changes read again after.
    fn saved(self: &Arc<Self>, path: &std::path::Path) {
        let found = {
            let mut read = self.lock();
            if read.resolving.is_empty() {
                return;
            }
            // The same file however its path is spelled (a link in the project's path).
            let real = std::fs::canonicalize(path).ok();
            let at = read.resolving.iter().position(|(p, _)| {
                p == path
                    || real
                        .as_ref()
                        .is_some_and(|r| std::fs::canonicalize(p).ok().as_ref() == Some(r))
            });
            at.map(|at| read.resolving.remove(at))
        };
        let Some((path, repo)) = found else { return };
        let weak = Arc::downgrade(self);
        kernel::work::run("git-conflicts", move || {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            crate::DISK.read(text.len() as u64);
            let relative = version_control::relative_path(repo.root(), &path);
            if let Some(relative) = relative
                && version_control::Merged::parse(&text).is_resolved()
                && let Err(e) = repo.mark_resolved(&[relative])
            {
                tracing::warn!(error = ?e, "can't mark the merged file resolved");
            }
            if let Some(changes) = weak.upgrade() {
                changes.stale();
            }
        });
    }

    /// Asks for a read (made at the next poll, `repo` in hand).
    pub fn stale(&self) {
        self.lock().stale = true;
        if let Some(wake) = &self.wake {
            wake();
        }
    }

    /// Starts a read if one is asked and none is going on.
    pub(crate) fn start(self: &Arc<Self>, repo: Arc<dyn Repository>) {
        {
            let mut read = self.lock();
            if !read.stale || read.reading || !read.held.is_empty() {
                return;
            }
            read.reading = true;
        }
        let weak = Arc::downgrade(self);
        let reads = self.reads.get_or_init(|| Pool::new("git-status", 1, work::IDLE));
        reads.spawn(Priority::Background, None, move || {
            std::thread::sleep(QUIET);
            let Some(changes) = weak.upgrade() else { return };
            changes.lock().stale = false;
            let files = repo.status();
            let operation = repo.operation();
            {
                let mut read = changes.lock();
                read.reading = false;
                match files {
                    Ok(files) => {
                        read.files = Arc::new(files);
                        read.operation = operation;
                        read.generation += 1;
                    }
                    Err(version_control::VcsError::Untrusted(lines)) => {
                        read.untrusted = Some(lines);
                    }
                    Err(e) => tracing::warn!(error = ?e, "can't read the working tree's status"),
                }
            }
            // Asked again meanwhile: the next poll starts it.
            if let Some(wake) = &changes.wake {
                wake();
            }
        });
    }
}

/// The conflicted paths among `files`, sorted.
fn conflicted(files: &[FileStatus]) -> Vec<String> {
    let mut paths: Vec<String> =
        files.iter().filter(|f| f.state == FileState::Conflicted).map(|f| f.path.clone()).collect();
    paths.sort();
    paths
}

/// Project `id`'s changes (while it's open).
pub fn changes(id: ProjectId) -> Option<Arc<Changes>> {
    PROJECTS.get(id)
}

/// Follows each project's changes: reads them when asked, puts their count on the button.
#[derive(Debug)]
pub(crate) struct ChangesService;

impl ProjectService for ChangesService {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        let changes =
            Arc::new(Changes { wake: Some(Arc::clone(&project.wake)), ..Changes::default() });
        PROJECTS.insert(project.id, &changes);
        changes.stale();
        let events = project.events;
        let id = project.id;
        vec![
            // Its entry in the table goes with the project.
            Subscription::on_end(move || PROJECTS.remove(id)),
            {
                let changes = Arc::clone(&changes);
                events.subscribe(move |e: &document::events::FileSaved| {
                    changes.saved(&e.path);
                    changes.stale();
                })
            },
            // Found after the service started: its files are read then.
            on::<version_control::RepositoryOpened>(&changes, events),
            // Written by others: read again unless all of it is ignored (a log, a cache).
            {
                let changes = Arc::clone(&changes);
                let ignored = crate::ignored::Ignored::default();
                events.subscribe(move |e: &workspace::events::FilesWritten| {
                    if !ignored.all(&e.paths) {
                        changes.stale();
                    }
                })
            },
            on::<workspace::events::EntriesChanged>(&changes, events),
            on::<workspace::events::EntriesDeleted>(&changes, events),
            on::<workspace::events::FileRenamed>(&changes, events),
            on::<workspace::events::EntriesRescan>(&changes, events),
            // A batch of changes (a refactoring, a paste) is read once, when it ends.
            {
                let changes = Arc::clone(&changes);
                events.subscribe(move |e: &workspace::events::BulkChangeStarted| {
                    changes.lock().held.push(e.id);
                })
            },
            {
                let changes = Arc::clone(&changes);
                events.subscribe(move |e: &workspace::events::BulkChangeEnded| {
                    changes.lock().held.retain(|h| *h != e.id);
                    changes.stale();
                })
            },
            on::<version_control::HeadChanged>(&changes, events),
        ]
    }

    fn busy(&self, id: ProjectId) -> bool {
        changes(id).is_some_and(|changes| {
            let read = changes.lock();
            read.stale || read.reading
        })
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(changes) = host.project_id().and_then(changes) else { return false };
        let Some(repo) = host.repository() else {
            // Not a repository (opened before the services start): nothing to read, no count.
            let shown = {
                let mut read = changes.lock();
                read.stale = false;
                read.shown.take()
            };
            if shown.is_some() {
                host.set_tool_badge(crate::log_view::TOOL, None);
            }
            return shown.is_some();
        };
        if ask_trust(&changes, host) {
            return true;
        }
        changes.start(repo);
        let (count, conflicted, generation, tell) = {
            let mut read = changes.lock();
            let conflicted = conflicted(&read.files).len();
            // Told once each time they appear (by us or another program: SourceTree).
            let tell = read.generation > 0 && conflicted > 0 && !read.told;
            read.told = conflicted > 0;
            (read.files.len(), conflicted > 0, read.generation, tell.then_some(conflicted))
        };
        if let Some(count) = tell {
            tell_conflicts(host, count, changes.operation().as_ref());
        }
        let now = Some((count, conflicted));
        let moved = generation > 0 && std::mem::replace(&mut changes.lock().shown, now) != now;
        if moved {
            let label = i18n::tr!("git.changed_files", count = count);
            let kind = if conflicted {
                workbench_model::BadgeKind::Warning
            } else {
                workbench_model::BadgeKind::Count
            };
            let badge = ToolBadge { count, kind, label };
            host.set_tool_badge(crate::log_view::TOOL, Some(badge));
        }
        moved
    }
}

/// Conflicts appeared: a warning (staying until closed) leading to the Conflicts tab.
fn tell_conflicts(host: &mut dyn WorkbenchHost, count: usize, operation: Option<&Operation>) {
    let title = i18n::tr!("git.conflicts_found", count = count);
    let mut notification = workbench_model::Notification::warning(title)
        .with_action(i18n::t("git.conflicts_resolve"), crate::SHOW_CONFLICTS)
        .with_key("git.conflicts");
    if let Some(operation) = operation {
        notification = notification.with_detail(crate::conflicts_view::operation_text(operation));
    }
    host.notify(notification);
}

/// The repository's own settings run programs: asked once whether git may run in it ("yes"
/// trusts it as it is, kept with the user's settings, and reads it). `true` if it asked.
fn ask_trust(changes: &Arc<Changes>, host: &mut dyn WorkbenchHost) -> bool {
    let lines = {
        let mut read = changes.lock();
        if read.asked {
            return false;
        }
        let Some(lines) = read.untrusted.take() else { return false };
        read.asked = true;
        lines
    };
    let Some(root) = host.project_root() else { return false };
    let shown: Vec<&str> = lines.iter().take(6).map(String::as_str).collect();
    let question = workbench_model::Question {
        title: i18n::t("git.trust_title"),
        message: i18n::tr!("git.trust", lines = shown.join("; ").as_str()),
        icon: "git-branch",
        yes: i18n::t("git.trust_yes"),
        danger: true,
        list: Vec::new(),
    };
    let changes = Arc::clone(changes);
    host.confirm(
        question,
        Box::new(move |_host| match git::trust::trust(&root) {
            Ok(()) => changes.stale(),
            Err(e) => tracing::warn!(error = %e, "can't keep the trusted repository"),
        }),
    )
}

/// The changes read again when `E` happens.
fn on<E: kernel::DomainEvent>(changes: &Arc<Changes>, events: &EventBus) -> Subscription {
    let changes = Arc::clone(changes);
    events.subscribe(move |_: &E| changes.stale())
}

#[cfg(test)]
#[path = "../tests/unit/status.rs"]
mod tests;
