//! The service keeping versions: subscribed to the project's saves, it hands each to the
//! project's worker (the UI never writes them; one at a time, in order; its thread ends when
//! idle, so a project nobody saves in holds none). Before a save, the version still on disk
//! (about to be replaced) is kept without reading it on the UI thread:
//! - the file as this history last kept it (its size and date the same): it's there
//!   already, nothing is read;
//! - else, on Unix, the file is only opened: the save puts a new file in its place and the
//!   one opened still holds the old version, read on the worker — dropped if it was written
//!   in place meanwhile (its date moved: it may not be the old version any more);
//! - else (Windows: an open file can't be replaced) it's read at once.
//!
//! Versions past the retention (the user's settings, read each time: a change applies at
//! once) are dropped before the first one is kept, and in each file as its versions are.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use history::application::{keep, prune};
use history::{Retention, RevisionKind};
use history_fs::DiskHistory;
use kernel::{EventBus, ProjectId, Subscription};
use workbench_model::{ProjectContext, ProjectService, ResourceReporter, ResourceUse};

/// What a project's history did, for the Task Manager: the versions written (its store
/// counts them), the saved files read, its thread's CPU (each project has its own: its
/// CPU is the project's).
#[derive(Debug)]
struct Usage {
    project: ProjectId,
    name: String,
    store: DiskHistory,
    read: AtomicU64,
    /// Nanoseconds.
    cpu: AtomicU64,
}

/// The open projects' histories (held weakly: one ends with its project's thread).
static OPEN: Mutex<Vec<Weak<Usage>>> = Mutex::new(Vec::new());

/// The open projects' histories in the Task Manager.
#[derive(Debug)]
pub(crate) struct Reporter;

impl ResourceReporter for Reporter {
    fn report(&self) -> Vec<ResourceUse> {
        let open: Vec<Arc<Usage>> =
            OPEN.lock().map(|o| o.iter().filter_map(Weak::upgrade).collect()).unwrap_or_default();
        open.iter()
            .map(|usage| {
                let name = i18n::tr!("tasks.local_history", project = usage.name.as_str());
                let mut part = ResourceUse::new(name, crate::MODULE_ID);
                part.project = Some(usage.project);
                part.threads = vec!["local-history"];
                part.read = Some(usage.read.load(Ordering::Relaxed));
                part.written = Some(usage.store.written());
                part.cpu = Some(std::time::Duration::from_nanos(usage.cpu.load(Ordering::Relaxed)));
                part
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct HistoryService {
    /// A fixed retention (tests); else the configured one ([`crate::settings::configured`]).
    retention: Option<Retention>,
}

impl HistoryService {
    fn retention(fixed: Option<Retention>) -> Retention {
        fixed.unwrap_or_else(crate::settings::configured)
    }
}

/// What the worker is asked.
enum Job {
    /// Keep these bytes (read before a save).
    Keep { path: String, text: Vec<u8>, kind: RevisionKind },
    /// Keep the file as it is now (just saved).
    KeepFile { path: String, file: PathBuf },
    /// Keep what `file` holds: opened before a save replaced it (still the old version),
    /// its size and date then `stamp`.
    KeepOpened { path: String, file: std::fs::File, stamp: Stamp },
}

/// A file's size and modification date: the same, the same content (as far as a save
/// tells).
type Stamp = (u64, SystemTime);

fn stamp_of(meta: &std::fs::Metadata) -> Option<Stamp> {
    Some((meta.len(), meta.modified().ok()?))
}

/// `file` relative to `root`, with `/`.
fn relative(root: &Path, file: &Path) -> Option<String> {
    version_control::relative_path(root, file)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl ProjectService for HistoryService {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        let (root, storage, events) = (project.root, project.storage, project.events);
        let store = DiskHistory::new(storage.join(crate::FOLDER));
        let usage = Arc::new(Usage {
            project: project.id,
            name: kernel::paths::file_name(root),
            store: store.clone(),
            read: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
        });
        if let Ok(mut open) = OPEN.lock() {
            open.retain(|w| w.strong_count() > 0);
            open.push(Arc::downgrade(&usage));
        }
        let keeper = Arc::new(Keeper {
            pool: kernel::work::Pool::new("local-history", 1, kernel::work::IDLE),
            store,
            usage,
            fixed: self.retention,
            pruned: AtomicBool::new(false),
            kept: Mutex::default(),
        });
        let project = project.id;
        vec![
            subscribe_before(events, root.to_path_buf(), Arc::clone(&keeper), self.retention),
            subscribe_after(events, root.to_path_buf(), keeper),
            Subscription::on_end(move || crate::follow::forget(project)),
        ]
    }

    /// The window's fixed tab, while it shows: the active file asked (else nothing).
    fn poll(&self, host: &mut dyn workbench_model::WorkbenchHost) -> bool {
        let Some(project) = host.project_id() else { return false };
        crate::follow::poll(project, || host.active_file().map(|f| f.path))
    }
}

/// A project's versions kept on its worker.
struct Keeper {
    /// One job at a time, in the order asked.
    pool: kernel::work::Pool,
    store: DiskHistory,
    usage: Arc<Usage>,
    fixed: Option<Retention>,
    /// The old versions were dropped (before the first one kept).
    pruned: AtomicBool,
    /// Each file's stamp when its version was last kept from the disk (after a save): a
    /// save of it as it still is needs nothing read.
    kept: Mutex<HashMap<String, Stamp>>,
}

impl Keeper {
    fn send(self: &Arc<Self>, job: Job) {
        if !self.pruned.swap(true, Ordering::Relaxed) {
            let keeper = Arc::clone(self);
            self.pool.spawn(kernel::work::Priority::Background, None, move || {
                keeper.timed(|k| {
                    let dropped = prune(&k.store, &HistoryService::retention(k.fixed), now_ms());
                    if dropped > 0 {
                        tracing::info!(dropped, "local history pruned");
                    }
                });
            });
        }
        let keeper = Arc::clone(self);
        self.pool.spawn(kernel::work::Priority::Background, None, move || {
            keeper.timed(|k| k.keep(job));
        });
    }

    fn keep(&self, job: Job) {
        let (path, text, kind) = match job {
            Job::Keep { path, text, kind } => (path, text, kind),
            Job::KeepFile { path, file } => {
                let before = std::fs::metadata(&file).ok().and_then(|m| stamp_of(&m));
                let Ok(text) = std::fs::read(&file) else { return };
                self.usage.read.fetch_add(text.len() as u64, Ordering::Relaxed);
                // Its stamp, if unchanged while read (else not known for this text).
                let after = std::fs::metadata(&file).ok().and_then(|m| stamp_of(&m));
                if let Ok(mut kept) = self.kept.lock() {
                    match before.filter(|b| Some(*b) == after) {
                        Some(stamp) => kept.insert(path.clone(), stamp),
                        None => kept.remove(&path),
                    };
                }
                (path, text, RevisionKind::Saved)
            }
            Job::KeepOpened { path, mut file, stamp } => {
                use std::io::Read as _;
                let mut text = Vec::new();
                if file.read_to_end(&mut text).is_err() {
                    return;
                }
                // Written in place since it was opened: maybe not the old version any more.
                if file.metadata().ok().and_then(|m| stamp_of(&m)) != Some(stamp) {
                    return;
                }
                self.usage.read.fetch_add(text.len() as u64, Ordering::Relaxed);
                (path, text, RevisionKind::BeforeSave)
            }
        };
        let retention = HistoryService::retention(self.fixed);
        if let Err(e) = keep(&self.store, &retention, &path, &text, kind, now_ms()) {
            tracing::warn!(%path, error = %e, "local history: can't keep");
        }
    }

    /// Runs `f`, its CPU counted as the project's.
    fn timed(&self, f: impl FnOnce(&Self)) {
        let before = kernel::cpu::thread_time();
        f(self);
        if let (Some(before), Some(after)) = (before, kernel::cpu::thread_time()) {
            let nanos = u64::try_from(after.saturating_sub(before).as_nanos()).unwrap_or(0);
            self.usage.cpu.fetch_add(nanos, Ordering::Relaxed);
        }
    }
}

/// Before a save: the version on disk kept (small files only), without reading it here when
/// it can be helped (see the module's doc).
fn subscribe_before(
    events: &EventBus,
    root: PathBuf,
    keeper: Arc<Keeper>,
    fixed: Option<Retention>,
) -> Subscription {
    events.subscribe(move |e: &document::events::FileWillSave| {
        let Some(path) = relative(&root, &e.path) else { return };
        let max = HistoryService::retention(fixed).max_file_size;
        let Some(stamp) = std::fs::metadata(&e.path).ok().and_then(|m| stamp_of(&m)) else {
            return;
        };
        if stamp.0 > max {
            return;
        }
        // As this history last kept it: there already.
        if keeper.kept.lock().is_ok_and(|kept| kept.get(&path) == Some(&stamp)) {
            return;
        }
        if cfg!(unix) {
            if let Ok(file) = std::fs::File::open(&e.path) {
                keeper.send(Job::KeepOpened { path, file, stamp });
            }
        } else if let Ok(text) = std::fs::read(&e.path) {
            keeper.send(Job::Keep { path, text, kind: RevisionKind::BeforeSave });
        }
    })
}

/// After a save: the file as written (read on the worker).
fn subscribe_after(events: &EventBus, root: PathBuf, keeper: Arc<Keeper>) -> Subscription {
    events.subscribe(move |e: &document::events::FileSaved| {
        if let Some(path) = relative(&root, &e.path) {
            keeper.send(Job::KeepFile { path, file: e.path.clone() });
        }
    })
}

#[cfg(test)]
#[path = "../tests/unit/service.rs"]
mod tests;
