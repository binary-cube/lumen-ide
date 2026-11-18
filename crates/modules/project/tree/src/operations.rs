//! The tree's operations on the project's entries, as the module's commands acting on the
//! action's targets ([`workbench_model::ActionContext`]: the right-clicked entry, the
//! tree's selection): a new file / folder typed in the tree, Rename (the new name typed in
//! the entry's row, renamed by the app as a tab's file is), Cut / Copy / Paste (a clipboard
//! shared by every window; the paste on a thread of its own, its progress in the status
//! bar, announced as one batch of changes), Delete (asked first, to the system's trash), Copy
//! Path, Reveal in the file manager. They go through the `workspace` domain's use cases
//! (`entries`) on the local disk; what changed is announced (`FileRenamed`,
//! `EntriesDeleted`), the folders listed again — whoever shows them follows.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, Weak};

use command_model::CommandId;
use fs_local::entries::LocalEntries;
use kernel::ProjectId;
use workbench_model::{
    Activities, ActivityKind, ActivityUnit, CommandHandler, DiskIo, EntriesRequest, Notification,
    Outcome, Progress, Question, ResourceReporter, ResourceUse, WorkbenchHost,
};

/// A project's file operations, for the Task Manager: what its pastes read and wrote, its
/// pastes and deletions as operations; who wakes its window. Held by the project's
/// subscription (an operation may hold it longer: `open` tells whether the project is).
struct ProjectFiles {
    project: ProjectId,
    name: String,
    disk: DiskIo,
    activities: Activities,
    wake: Waker,
    open: Weak<()>,
}

impl std::fmt::Debug for ProjectFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectFiles").field("project", &self.project).finish_non_exhaustive()
    }
}

impl ProjectFiles {
    /// Its project is still open (its subscriptions held).
    fn is_open(&self) -> bool {
        self.open.strong_count() > 0
    }
}

/// The open projects' file operations (held weakly).
static PROJECTS: Mutex<Vec<Weak<ProjectFiles>>> = Mutex::new(Vec::new());

/// Project `id`'s file operations, if it's open.
fn files_of(id: Option<ProjectId>) -> Option<Arc<ProjectFiles>> {
    let id = id?;
    let projects = PROJECTS.lock().ok()?;
    projects.iter().filter_map(Weak::upgrade).find(|p| p.project == id && p.is_open())
}

/// An operation of a project's: its files' record, its identity.
type Followed = Option<(Arc<ProjectFiles>, u128)>;

/// The open projects' file operations in the Task Manager.
#[derive(Debug)]
pub struct Reporter;

impl ResourceReporter for Reporter {
    fn report(&self) -> Vec<ResourceUse> {
        let open: Vec<Arc<ProjectFiles>> = PROJECTS
            .lock()
            .map(|p| p.iter().filter_map(Weak::upgrade).filter(|f| f.is_open()).collect())
            .unwrap_or_default();
        open.iter()
            .map(|files| {
                let name = i18n::tr!("tasks.file_operations", project = files.name.as_str());
                let mut part = ResourceUse::new(name, crate::MANIFEST.id.as_str());
                part.project = Some(files.project);
                part.threads = vec!["paste", "trash"];
                files.disk.report(&mut part);
                files.activities.report(&mut part);
                part
            })
            .collect()
    }
}

pub const NEW_FILE: CommandId = CommandId("tree.new_file");
pub const NEW_FOLDER: CommandId = CommandId("tree.new_folder");
pub const CUT: CommandId = CommandId("tree.cut");
pub const COPY: CommandId = CommandId("tree.copy");
pub const PASTE: CommandId = CommandId("tree.paste");
pub const DELETE: CommandId = CommandId("tree.delete");
pub const RENAME: CommandId = CommandId("tree.rename");
pub const COPY_PATH: CommandId = CommandId("tree.copy_path");
pub const REVEAL: CommandId = CommandId("tree.reveal_in_file_manager");
pub const CANCEL_PASTE: CommandId = CommandId("files.cancel_paste");

/// The paste's work in the status bar.
const PASTING: &str = "paste";

/// The copied / cut entries (absolute paths), shared by every window.
static CLIPBOARD: Mutex<Option<(Vec<PathBuf>, bool)>> = Mutex::new(None);

/// Wakes the app (its polls run): from a background thread.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// A paste going on.
struct PasteJob {
    rx: Receiver<PasteEvent>,
    cancel: Arc<AtomicBool>,
    cut: bool,
    /// Files done / to do.
    progress: (u64, u64),
    /// Where the entries go.
    into: PathBuf,
    /// The batch of changes announced for it (`BulkChangeStarted` / `Ended`).
    bulk: u128,
    /// Its operation in the Task Manager.
    followed: Followed,
    /// The project it pastes into: only its window brings its progress and end back (its
    /// batch ends on that project's events; another's would leave it open) — unless the
    /// project closed meanwhile: then any window ends it, on the project's own events.
    project: Option<kernel::ProjectId>,
    events: kernel::EventBus,
}

enum PasteEvent {
    Progress(u64, u64),
    Done(workspace::entries::PasteOutcome),
}

/// The operations' state, shared by the module's commands and its project service (which
/// brings a paste's progress and end back to the UI thread).
#[derive(Default)]
pub struct Operations {
    wake: Mutex<Option<Waker>>,
    paste: Mutex<Option<PasteJob>>,
    deleting: AtomicBool,
}

impl std::fmt::Debug for Operations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Operations").finish_non_exhaustive()
    }
}

/// The status bar's text while a paste runs ("Copying 120 / 340 files"); a click stops it.
fn paste_progress(cut: bool, (done, total): (u64, u64)) -> Progress {
    let key = if cut { "files.moving" } else { "files.copying" };
    Progress {
        label: i18n::tr!(key, done = done, total = total.max(done)),
        icon: "copy",
        stop: Some(CANCEL_PASTE),
    }
}

/// The folder an entry's paste or new entry goes into (a file's own folder).
fn folder_of(path: &Path) -> PathBuf {
    if path.is_dir() { path.to_path_buf() } else { path.parent().unwrap_or(path).to_path_buf() }
}

impl Operations {
    /// Who to wake (known once a project is attached).
    pub fn set_waker(&self, wake: Waker) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(wake);
        }
    }

    fn waker(&self) -> Option<Waker> {
        self.wake.lock().ok().and_then(|w| w.clone())
    }

    /// `paths` copied (or cut) for a later paste: here and in the system's clipboard.
    pub fn copy(&self, host: &mut dyn WorkbenchHost, paths: Vec<PathBuf>, cut: bool) -> bool {
        if paths.is_empty() {
            return false;
        }
        host.set_clipboard_files(&paths);
        if let Ok(mut clipboard) = CLIPBOARD.lock() {
            *clipboard = Some((paths, cut));
        }
        host.notify(workbench_model::Notification::success(i18n::t(if cut {
            "files.cut"
        } else {
            "files.copied"
        })));
        true
    }

    /// Pastes into `into`'s folder: files copied in the system's file manager when they're
    /// not what was copied here, else what was copied / cut here, else paths in the
    /// clipboard's text — in the background. `false`: nothing to paste, or a paste running.
    pub fn paste(&self, host: &mut dyn WorkbenchHost, into: &Path) -> bool {
        let Ok(mut slot) = self.paste.lock() else { return false };
        if slot.as_ref().is_some_and(PasteJob::orphaned)
            && let Some(job) = slot.take()
        {
            abandon(&job);
        }
        if slot.is_some() {
            return false;
        }
        let dir = folder_of(into);
        let ours = CLIPBOARD.lock().ok().and_then(|c| c.clone());
        let os_files = host.clipboard_files();
        let text = host.clipboard_text();
        let (sources, cut) = workspace::entries::paste_sources(
            &LocalEntries,
            ours,
            &os_files,
            text.as_deref(),
            &dir,
        );
        if sources.is_empty() {
            host.notify(workbench_model::Notification::info(i18n::t("files.nothing_to_paste")));
            return false;
        }
        // Many entries change at once: announced as one batch (whoever follows them — the
        // index — waits for its end and takes the changes as one).
        let bulk = uuid::Uuid::new_v4().as_u128();
        host.events().publish(&workspace::events::BulkChangeStarted { id: bulk });
        // An operation of the project's (for the Task Manager): its files, its bytes.
        let files = files_of(host.project_id());
        let followed: Followed = files.map(|files| {
            let kind = if cut { ActivityKind::Move } else { ActivityKind::Copy };
            let subject = match sources.as_slice() {
                [one] => kernel::paths::file_name(one),
                many => i18n::tr!("tasks.operation_files", count = many.len()),
            };
            let id = files.activities.start(
                Some(files.project),
                kind,
                subject,
                ActivityUnit::Files,
                None,
            );
            (files, id)
        });
        let counting = followed.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        // The pasting window's own (the last attached may be another's).
        let waker = followed.as_ref().map(|(f, _)| Arc::clone(&f.wake)).or_else(|| self.waker());
        let stop = Arc::clone(&cancel);
        let target = dir.clone();
        kernel::work::run("paste", move || {
            let wake = || {
                if let Some(w) = &waker {
                    w();
                }
            };
            let mut last = std::time::Instant::now();
            let mut counted = 0;
            // A redraw a few times a second, not per file.
            let mut progress = |done: u64, total: u64| {
                if let Some((files, id)) = &counting {
                    files.activities.set_total(*id, total);
                    files.activities.advance(*id, done.saturating_sub(counted));
                    counted = done;
                }
                if last.elapsed() > std::time::Duration::from_millis(100) {
                    last = std::time::Instant::now();
                    let _ = tx.send(PasteEvent::Progress(done, total));
                    wake();
                }
            };
            let outcome = workspace::entries::paste(
                &LocalEntries,
                sources,
                cut,
                &target,
                &mut progress,
                // A copy reads the file and writes it.
                &mut |bytes| {
                    if let Some((files, _)) = &counting {
                        files.disk.read(bytes);
                        files.disk.written(bytes);
                    }
                },
                &|| stop.load(Ordering::Relaxed),
            );
            let _ = tx.send(PasteEvent::Done(outcome));
            wake();
        });
        host.set_progress(PASTING, Some(paste_progress(cut, (0, 0))));
        let project = host.project_id();
        *slot = Some(PasteJob {
            rx,
            cancel,
            cut,
            progress: (0, 0),
            into: dir,
            bulk,
            followed,
            project,
            events: host.events().clone(),
        });
        true
    }

    /// Stops the paste going on (what's done stays).
    pub fn cancel_paste(&self) {
        if let Some(job) = self.paste.lock().ok().as_ref().and_then(|j| j.as_ref()) {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The paste's progress / its end, on the UI thread. `true`: something changed.
    pub fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        let finished = {
            let Ok(mut slot) = self.paste.lock() else { return false };
            let Some(job) = slot.as_mut() else { return false };
            // Its project closed meanwhile: ended here (on its own events).
            if job.orphaned() {
                if let Some(job) = slot.take() {
                    abandon(&job);
                }
                return false;
            }
            // Another project's window: left for the paste's own.
            if job.project != host.project_id() {
                return false;
            }
            let mut changed = false;
            let mut done = None;
            while let Ok(event) = job.rx.try_recv() {
                changed = true;
                match event {
                    PasteEvent::Progress(d, total) => job.progress = (d, total),
                    PasteEvent::Done(outcome) => done = Some(outcome),
                }
            }
            match done {
                Some(outcome) => slot.take().map(|job| (job, outcome)),
                None => {
                    if changed {
                        host.set_progress(PASTING, Some(paste_progress(job.cut, job.progress)));
                    }
                    return changed;
                }
            }
        };
        if let Some((job, outcome)) = finished {
            pasted(host, &job, &outcome);
        }
        true
    }

    /// Asks before moving `paths` (inside the project, not twice) to the trash; then does it
    /// in the background. `false`: nothing to delete, or a deletion running.
    pub fn delete(self: &Arc<Self>, host: &mut dyn WorkbenchHost, paths: &[PathBuf]) -> bool {
        let Some(root) = host.project_root() else { return false };
        let paths = workspace::delete::deletion_roots(paths, &root);
        let Some(first) = paths.first() else { return false };
        if self.deleting.load(Ordering::Relaxed) {
            return false;
        }
        let name = format!("**{}**", kernel::paths::file_name(first));
        let message = match (paths.len(), first.is_dir()) {
            (1, true) => i18n::tr!("dialog.delete_folder", name = name),
            (1, false) => i18n::tr!("dialog.delete_file", name = name),
            (n, _) => i18n::tr!("dialog.delete_many", count = n),
        };
        // Many: each of them listed under the message (from the project's root).
        let list = if paths.len() > 1 { listed(&paths, &root) } else { Vec::new() };
        let question = Question {
            title: i18n::t("dialog.delete_title"),
            message,
            icon: "trash",
            yes: i18n::t("dialog.delete"),
            danger: true,
            list,
        };
        let operations = Arc::clone(self);
        host.confirm(
            question,
            Box::new(move |host| {
                operations.deleting.store(true, Ordering::Relaxed);
                let done = Arc::clone(&operations);
                // An operation of the project's (for the Task Manager).
                let followed: Followed = files_of(host.project_id()).map(|files| {
                    let subject = match paths.as_slice() {
                        [one] => kernel::paths::file_name(one),
                        many => i18n::tr!("tasks.operation_files", count = many.len()),
                    };
                    let total = Some(paths.len() as u64);
                    let id = files.activities.start(
                        Some(files.project),
                        ActivityKind::Delete,
                        subject,
                        ActivityUnit::Files,
                        total,
                    );
                    (files, id)
                });
                host.spawn(Box::new(move || {
                    let result = workspace::entries::trash(&LocalEntries, &paths);
                    Box::new(move |host: &mut dyn WorkbenchHost| {
                        done.deleting.store(false, Ordering::Relaxed);
                        if let Some((files, id)) = &followed {
                            if result.is_ok() {
                                files.activities.advance(*id, paths.len() as u64);
                            }
                            let outcome = match &result {
                                Ok(()) => Outcome::Done,
                                Err(why) => Outcome::Failed(why.clone()),
                            };
                            files.activities.end(*id, outcome);
                        }
                        deleted(host, &paths, result);
                    })
                }));
            }),
        )
    }
}

impl PasteJob {
    /// Its project closed while it ran (nobody would bring its end back).
    fn orphaned(&self) -> bool {
        self.followed.as_ref().is_some_and(|(files, _)| !files.is_open())
    }
}

/// A paste whose project closed: stopped, its batch ended on that project's events (the
/// queues waiting for it let go), its operation ended.
fn abandon(job: &PasteJob) {
    job.cancel.store(true, Ordering::Relaxed);
    job.events.publish(&workspace::events::BulkChangeEnded { id: job.bulk });
    if let Some((files, id)) = &job.followed {
        files.activities.end(*id, Outcome::Stopped);
    }
}

/// A paste's end: moved entries announced (tabs follow, the indexer reads them again), the
/// cut used up, the folders listed again, the last entry shown, one reindex for all.
fn pasted(
    host: &mut dyn WorkbenchHost,
    job: &PasteJob,
    outcome: &workspace::entries::PasteOutcome,
) {
    if job.cut {
        for (from, to) in &outcome.placed {
            let event = workspace::events::FileRenamed { from: from.clone(), to: to.clone() };
            host.events().publish(&event);
        }
        if let Ok(mut clipboard) = CLIPBOARD.lock() {
            *clipboard = None;
        }
    }
    let mut dirs: Vec<PathBuf> = outcome
        .placed
        .iter()
        .flat_map(|(from, to)| [from.parent(), to.parent()])
        .flatten()
        .map(Path::to_path_buf)
        .chain([job.into.clone()])
        .collect();
    dirs.sort();
    dirs.dedup();
    for dir in &dirs {
        host.relist(dir);
    }
    if let Some((_, last)) = outcome.placed.last() {
        let reveal = EntriesRequest::Reveal { path: last.clone(), expand: false };
        host.tell_panel(crate::TOOL, &reveal, false);
    }
    host.events().publish(&workspace::events::BulkChangeEnded { id: job.bulk });
    host.set_progress(PASTING, None);
    if let Some((files, id)) = &job.followed {
        let outcome = match outcome.failed.first() {
            _ if job.cancel.load(Ordering::Relaxed) => Outcome::Stopped,
            Some((_, why)) => Outcome::Failed(why.clone()),
            None => Outcome::Done,
        };
        files.activities.end(*id, outcome);
    }
    let message = match outcome.failed.first() {
        None => Notification::success(i18n::tr!("files.pasted", count = outcome.placed.len())),
        Some((path, why)) => Notification::error(i18n::tr!(
            "files.paste_failed",
            name = kernel::paths::file_name(path),
            reason = why.as_str()
        )),
    };
    host.notify(message);
}

/// A deletion's end: announced (saved tabs close), the folders listed again; or why not.
fn deleted(host: &mut dyn WorkbenchHost, paths: &[PathBuf], result: Result<(), String>) {
    if let Err(why) = result {
        tracing::warn!("delete failed: {why}");
        host.notify(Notification::error(i18n::tr!("files.delete_failed", reason = why.as_str())));
        return;
    }
    let mut dirs: Vec<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        host.relist(dir);
    }
    host.events().publish(&workspace::events::EntriesDeleted { paths: paths.to_vec() });
    let message = match paths {
        [one] => i18n::tr!("files.deleted_one", name = kernel::paths::file_name(one)),
        many => i18n::tr!("files.deleted", count = many.len()),
    };
    host.notify(Notification::success(message));
}

/// One of the tree's commands, on the action's targets.
#[derive(Debug)]
pub struct Command {
    pub id: CommandId,
    pub operations: Arc<Operations>,
}

impl CommandHandler for Command {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        let context = host.context();
        let on_disk = || !context.any(workbench_model::context::facts::ABSENT);
        match self.id {
            NEW_FILE | NEW_FOLDER => host.project_root().is_some(),
            CANCEL_PASTE => true,
            CUT | COPY | DELETE | REVEAL => context.path().is_some() && on_disk(),
            PASTE | COPY_PATH => context.path().is_some(),
            // One entry of the project (not a selection of many, not the root).
            RENAME => {
                let root = host.project_root();
                let inside = |p: &std::path::Path| {
                    root.as_deref().is_some_and(|r| p.starts_with(r) && p != r)
                };
                context.targets.len() == 1 && context.path().is_some_and(inside) && on_disk()
            }
            _ => false,
        }
    }

    /// Rename's keys (⇧F6) work while the tree has them; the editor's are its own (the
    /// symbol renamed).
    fn when(&self) -> command_model::When {
        match self.id {
            RENAME => command_model::When::View(crate::TOOL),
            _ => command_model::When::Everywhere,
        }
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let context = host.context();
        let paths = context.paths();
        match self.id {
            NEW_FILE | NEW_FOLDER => {
                let new = EntriesRequest::NewEntry { is_dir: self.id == NEW_FOLDER };
                host.tell_panel(crate::TOOL, &new, true)
            }
            RENAME => match context.path() {
                Some(path) => {
                    let rename = EntriesRequest::Rename { path: path.to_path_buf() };
                    host.tell_panel(crate::TOOL, &rename, true)
                }
                None => false,
            },
            CUT | COPY => self.operations.copy(host, paths, self.id == CUT),
            PASTE => match context.path() {
                Some(into) => self.operations.paste(host, into),
                None => false,
            },
            DELETE => self.operations.delete(host, &paths),
            COPY_PATH => {
                let text = paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>();
                host.set_clipboard_text(&text.join("\n"));
                false
            }
            REVEAL => {
                if let Some(path) = context.path() {
                    host.reveal_in_file_manager(path);
                }
                false
            }
            CANCEL_PASTE => {
                self.operations.cancel_paste();
                true
            }
            _ => false,
        }
    }
}

/// Brings the paste's progress and end to the UI thread (the app polls project services
/// after a wake).
#[derive(Debug)]
pub struct Service(pub Arc<Operations>);

impl workbench_model::ProjectService for Service {
    fn attach(&self, project: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        self.0.set_waker(Arc::clone(&project.wake));
        // The project's file operations' record, held by a subscription of its own: gone
        // when the project closes (with its subscriptions; `open` with them).
        let open = Arc::new(());
        let files = Arc::new(ProjectFiles {
            project: project.id,
            name: project
                .root
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            disk: DiskIo::new(),
            activities: Activities::new(std::time::Instant::now),
            wake: Arc::clone(&project.wake),
            open: Arc::downgrade(&open),
        });
        if let Ok(mut projects) = PROJECTS.lock() {
            projects.retain(|p| p.strong_count() > 0);
            projects.push(Arc::downgrade(&files));
        }
        let held = project.events.subscribe(move |_: &workspace::events::ProjectOpened| {
            let _ = (&files, &open);
        });
        vec![held]
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        self.0.poll(host)
    }
}

/// `paths` as a question lists them: from `root`, `/`-separated (on Windows too); one
/// outside it, whole.
fn listed(paths: &[PathBuf], root: &Path) -> Vec<String> {
    paths
        .iter()
        .map(|p| match p.strip_prefix(root) {
            Ok(relative) => {
                let parts: Vec<_> =
                    relative.components().map(|c| c.as_os_str().to_string_lossy()).collect();
                parts.join("/")
            }
            Err(_) => p.display().to_string(),
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/operations.rs"]
mod tests;
