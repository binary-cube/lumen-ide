//! A remote project's session, while it's open: its server (a pool of connections kept and
//! shared, [`crate::pool`]), its local copy, what was last transferred (the manifest), the
//! transfers' log, what's still to upload and what the user must hear about. Uploads and
//! downloads go through the project's queues ([`crate::transfers`]); listings and the user's
//! conflict decisions through its own thread (`remote-sync`). [`RemoteService`] attaches one
//! to a project whose connection is saved, none otherwise — following the project's events.
//!
//! Saving a file uploads it — after checking the server's copy didn't change since the last
//! transfer: if it did, nothing is overwritten, the user is told and the two are compared.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, Weak};

use kernel::{ProjectId, SharedQueue, Subscription};
use remote::application::{
    Action, Change, Connector, LocalFiles, RemoteFs, Run, Standing, Transfer, apply, standing,
};
use remote::manifest::Manifest;
use remote::outbox::Outbox;
use remote::{RemoteEntry, Retry};
use workbench_model::{ProjectContext, ProjectKind, ProjectService, WorkbenchHost};

use crate::config::{self, Saved};
use crate::transfers::Uploaded;

mod permissions;
mod reconnect;
mod resources;
mod service;
pub(crate) use reconnect::reconnect_later;
pub(crate) use service::is_waiting;
use service::*;

/// Transfers kept in the log (the newest).
const LOG: usize = 500;
/// How long a queue waits for saves to calm down (a file saved again meanwhile: once).
const QUIET: std::time::Duration = std::time::Duration::from_millis(300);
/// At most this long after the first request.
const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
/// A batch of changes never ended (a paste that died) stops holding after this.
const HOLD_LIMIT: std::time::Duration = std::time::Duration::from_secs(600);
/// Local files up to this size are read to tell a touched one from a changed one (the
/// tree's marks); larger ones count as changed when their date moved.
const HASHED_FOR_MARKS: u64 = 256 * 1024;

/// The connector for `saved`, with its secret (from the keychain / the vault) and the
/// server key or certificate the user just accepted.
pub fn connector(
    saved: &Saved,
    secret: Option<remote::Secret>,
    accepted: Option<String>,
) -> Arc<dyn Connector> {
    use remote::{Auth, Protocol};
    let c = &saved.connection;
    match c.protocol {
        Protocol::Sftp => {
            let credentials = match &c.auth {
                Auth::Password => remote_sftp::Credentials::Password(secret.unwrap_or_default()),
                Auth::Key { path } => {
                    remote_sftp::Credentials::Key { path: path.clone(), passphrase: secret }
                }
                Auth::Agent => remote_sftp::Credentials::Agent,
            };
            let ours = config::known_hosts().unwrap_or_default();
            let mut known: Vec<PathBuf> =
                dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts")).into_iter().collect();
            known.push(ours.clone());
            Arc::new(remote_sftp::SftpConnector {
                host: c.host.clone(),
                port: c.port,
                user: c.user.clone(),
                credentials,
                root: c.root.clone(),
                host_keys: remote_sftp::HostKeys { known, learn_into: ours, accepted },
            })
        }
        Protocol::Ftp | Protocol::Ftps { .. } => {
            let security = match c.protocol {
                Protocol::Ftps { implicit: true } => remote_ftp::Security::Implicit,
                Protocol::Ftps { implicit: false } => remote_ftp::Security::Explicit,
                _ => remote_ftp::Security::Plain,
            };
            let mut trusted = saved.trusted.clone();
            trusted.extend(accepted);
            Arc::new(remote_ftp::FtpConnector {
                host: c.host.clone(),
                port: c.port,
                user: c.user.clone(),
                password: secret.unwrap_or_default(),
                security,
                root: c.root.clone(),
                trusted,
                tls12: Default::default(),
                chmod: Default::default(),
                hashes: Default::default(),
            })
        }
    }
}

/// The pool of `saved`'s connections through `connector`, `limit` at most: cautious for FTP
/// (servers limit the connections of an address: more only while they're accepted).
pub fn pool_for(
    saved: &Saved,
    connector: Arc<dyn Connector>,
    limit: usize,
) -> Arc<crate::pool::Pool> {
    let c = &saved.connection;
    match c.protocol {
        remote::Protocol::Sftp => crate::pool::Pool::new(connector, limit),
        _ => crate::pool::Pool::cautious(connector, limit, format!("{}:{}", c.host, c.port)),
    }
}

/// The local copy (the project's data folder left out).
pub fn local_copy(root: &Path) -> fs_local::remote_copy::LocalCopy {
    fs_local::remote_copy::LocalCopy {
        root: root.to_path_buf(),
        skip: vec![kernel::brand::PROJECT_DIR.to_owned()],
    }
}

fn manifest_file(root: &Path) -> PathBuf {
    config::data_dir(root).join("remote-manifest")
}

fn outbox_file(root: &Path) -> PathBuf {
    config::data_dir(root).join("remote-outbox")
}

/// The moves and deletions waiting to go (kept with the project: they outlive a restart).
pub fn load_outbox(root: &Path) -> Outbox {
    Outbox::from_text(&std::fs::read_to_string(outbox_file(root)).unwrap_or_default())
}

pub fn save_outbox(root: &Path, outbox: &Outbox) {
    let file = outbox_file(root);
    let written = if outbox.is_empty() && !file.exists() {
        Ok(())
    } else {
        files::write_atomic(&file, outbox.to_text().as_bytes())
    };
    if let Err(e) = written {
        tracing::warn!("can't keep the remote moves and deletions waiting: {e}");
    }
}

pub fn load_manifest(root: &Path) -> Manifest {
    Manifest::from_text(&std::fs::read_to_string(manifest_file(root)).unwrap_or_default())
}

pub fn save_manifest(root: &Path, manifest: &Manifest) {
    // Atomic: a manifest cut short would make every file look new or in conflict.
    if let Err(e) = files::write_atomic(&manifest_file(root), manifest.to_text().as_bytes()) {
        tracing::warn!("can't keep the remote manifest: {e}");
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Runs a transfer worker on `n` threads at once (named: the Task Manager counts their
/// CPU for the remote work); with no thread to be had, here.
// Scoped threads, bounded by the transfer's own limit and gone when it ends: a pool's would
// be held by one slow server for the whole batch.
#[allow(clippy::disallowed_methods)]
pub fn threads(n: usize, work: &(dyn Fn() + Sync)) {
    std::thread::scope(|scope| {
        for _ in 0..n {
            let named = std::thread::Builder::new().name("remote-transfer".into());
            if named.spawn_scoped(scope, work).is_err() {
                work();
            }
        }
    });
}

/// What the user must hear about (handled on the UI thread).
#[derive(Debug)]
pub enum Notice {
    /// A message for the user (a toast).
    Message(workbench_model::Notification),
    /// Saved here while the server's copy changed: not uploaded; compared.
    Conflict { path: PathBuf, relative: String, local: String, server: String },
    /// Entries deleted here that the server has (moves and deletions follow): delete them
    /// there too? (relative paths).
    ConfirmRemove(Vec<String>),
    /// A file compared with the server's copy (the user asked): `local` here, `server` there
    /// (empty where one side has none).
    Compare { path: PathBuf, relative: String, local: String, server: String },
    /// The listed rows' changes here given up (the server's copies taken back): asked first.
    ConfirmDiscard(Vec<(String, crate::outgoing::Why)>),
    /// Paths (files, folders, `*.ext`) never to upload any more: kept in the connection.
    Exclude(Vec<String>),
    /// An upload of several found files changed on the server too (`conflicts`): nothing
    /// went; resolve them first, or send the `rest` now?
    BulkConflicts { conflicts: Vec<String>, rest: Vec<(String, crate::outgoing::Why)> },
}

/// What the session's own thread does, one at a time (uploads and downloads have their
/// queues).
enum Job {
    /// After a conflict: ours goes up anyway (the server's copy replaced).
    UploadMine(String),
    /// After a conflict: the server's copy comes down (ours replaced).
    TakeServer(String),
    /// On demand: a folder of the server listed (the tree shows it).
    List(String),
    /// Moved here (moves follow): moved on the server too.
    Rename(String, String),
    /// A move waiting in the outbox sent (the user asked): `.0` here, `.1` on the server.
    SendMove(String, String),
    /// File `.0` here compared with `.1` on the server (its name there).
    Compare(String, String),
    /// The rows' changes here given up, confirmed: the server's copies taken back.
    Discard(Vec<(String, crate::outgoing::Why)>),
    /// Deleted here, and confirmed: deleted on the server too.
    Remove(Vec<String>),
}

/// Told when a file brought on demand is here (or why not).
type Fetched = Sender<Result<(), String>>;

/// A session's state, shared with its thread and its views.
pub struct State {
    pub saved: Saved,
    pub connector: Arc<dyn Connector>,
    pub manifest: Mutex<Manifest>,
    /// Moves and deletions made here while uploads wait for the user (kept on disk).
    pub(crate) outbox: Mutex<Outbox>,
    /// A save uploads at once (the connection's switch, changed without reconnecting).
    auto_upload: std::sync::atomic::AtomicBool,
    /// The newest transfers last.
    pub log: Mutex<VecDeque<Transfer>>,
    inbox: Mutex<Vec<Notice>>,
    /// Transfers going on.
    pub busy: std::sync::atomic::AtomicUsize,
    /// The bytes uploaded and downloaded since the project opened, as they pass (the Task
    /// Manager's network rate) — and so read here (uploads) and written here (downloads).
    pub traffic: workbench_model::resources::Traffic,
    pub disk: workbench_model::DiskIo,
    /// The project (its identity: its operations go by it), and its transfers as
    /// operations for the Task Manager.
    pub project: ProjectId,
    pub activities: workbench_model::Activities,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The session's thread (its work, one at a time).
    jobs: Mutex<Option<Sender<Job>>>,
    /// On demand: the server's folders listed so far (by relative folder, `""`: the root),
    /// those asked for and not listed yet, and those listed since the tree last looked.
    listings: Mutex<HashMap<String, Vec<RemoteEntry>>>,
    asked: Mutex<HashSet<String>>,
    relisted: Mutex<Vec<String>>,
    /// Files whose conflict the user is resolving by comparing: their next save is the
    /// resolution (uploaded as it is).
    resolving: Mutex<HashSet<String>>,
    /// Files changed on the server too since their last transfer: not uploaded until the
    /// user decides (the list marks them).
    pub(crate) conflicts: Mutex<std::collections::BTreeSet<String>>,
    /// The queues: files to upload, files to bring (on demand).
    pub(crate) uploads: SharedQueue<String>,
    pub(crate) downloads: SharedQueue<String>,
    /// Who waits for a file being brought (asked twice: brought once, both told).
    fetching: Mutex<HashMap<String, Vec<Fetched>>>,
    /// What's still to upload (it couldn't go: offline, an error), and why.
    pending: Mutex<BTreeMap<String, String>>,
    /// A retry is waiting; how many in a row failed (the waits grow).
    pub(crate) retrying: std::sync::atomic::AtomicBool,
    pub(crate) failures: std::sync::atomic::AtomicUsize,
    /// The server was out of reach (told once until it's back).
    pub(crate) offline: std::sync::atomic::AtomicBool,
    /// The project's events (its own bus): what's transferred, what runs and waits.
    pub events: kernel::EventBus,
    /// What the status bar shows now (running, to upload): set again only when it changes.
    shown: Mutex<Option<(usize, usize)>>,
    /// What's to upload (the button's count, the "To upload" tab), and the count the button
    /// shows now (how many, one couldn't go).
    pub(crate) outgoing: crate::outgoing::Outgoing,
    badge: Mutex<Option<(usize, workbench_model::BadgeKind)>>,
    /// The pool its connector is (`None`: a bare connector, the tests'): what the server
    /// accepts at once.
    pub(crate) pool: Option<Arc<crate::pool::Pool>>,
    /// The server's permissions shown on the tree's rows (a setting): by folder, its
    /// entries' notes; the folders asked for and not read yet.
    pub(crate) permissions: Mutex<HashMap<String, HashMap<String, workspace::Note>>>,
    pub(crate) permissions_asked: Mutex<HashSet<String>>,
    /// The uploaded files already warned about (the web server can't read them); those
    /// uploaded since the last look (checked together once the uploads end), and whether a
    /// look waits for them.
    pub(crate) warned: Mutex<HashSet<String>>,
    pub(crate) to_check: Mutex<Vec<String>>,
    pub(crate) checking: std::sync::atomic::AtomicBool,
    /// What the user sent and hasn't ended yet (the buttons' loaders): from the press until
    /// each goes up, fails or waits for a decision (a conflict), by when it was sent (the
    /// generation then: an upload already running when it was sent again doesn't end it);
    /// and how many times it changed (the views draw again only then).
    sending: Mutex<HashMap<String, u64>>,
    sending_generation: std::sync::atomic::AtomicU64,
}

impl Drop for State {
    fn drop(&mut self) {
        // The queues' threads end with it.
        self.uploads.close();
        self.downloads.close();
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("connection", &self.saved.connection.id)
            .finish_non_exhaustive()
    }
}

impl State {
    pub fn retry(&self) -> Retry {
        self.saved.connection.retries(crate::settings::defaults().retry)
    }

    /// Its transfers at once (its connection's, else the global settings).
    pub fn limits(&self) -> remote::Limits {
        self.saved.connection.limits(crate::settings::defaults().limits)
    }

    /// Wakes the app (a view's background work done).
    pub fn wake(&self) -> Arc<dyn Fn() + Send + Sync> {
        Arc::clone(&self.wake)
    }

    /// Bytes moved one way, as they pass: on the network, and on the disk here (an upload
    /// reads them, a download writes them).
    pub fn moved(&self, action: Action, bytes: u64) {
        count(&self.traffic, action, bytes);
        match action {
            Action::Upload => self.disk.read(bytes),
            Action::Download => self.disk.written(bytes),
            _ => {}
        }
    }

    /// `changes` as one operation (for the Task Manager): a single file counted in bytes,
    /// several in files. Follow it with [`Operation`] (from any thread: `apply`'s workers).
    pub(crate) fn begin(&self, changes: &[Change]) -> Operation {
        use workbench_model::{ActivityKind, ActivityUnit};
        let kind = |a: &Action| match a {
            Action::Upload => ActivityKind::Upload,
            Action::Download => ActivityKind::Download,
            Action::DeleteLocal | Action::DeleteRemote => ActivityKind::Delete,
        };
        let first = changes.first().map(|c| kind(&c.action)).unwrap_or(ActivityKind::Sync);
        let kind = if changes.iter().all(|c| kind(&c.action) == first) {
            first
        } else {
            ActivityKind::Sync
        };
        let (subject, unit, total) = match changes {
            [one] => {
                let size = match one.action {
                    Action::Upload => one.local.as_ref().map(|l| l.size),
                    _ => one.remote.as_ref().map(|r| r.size),
                };
                (one.path.clone(), ActivityUnit::Bytes, size)
            }
            many => (
                i18n::tr!("tasks.operation_files", count = many.len()),
                ActivityUnit::Files,
                Some(many.len() as u64),
            ),
        };
        let id = self.activities.start(Some(self.project), kind, subject, unit, total);
        Operation { id, unit }
    }

    /// `operation` stops once `flag` is set: the Task Manager can stop it.
    pub(crate) fn stoppable(&self, operation: Operation, flag: Arc<std::sync::atomic::AtomicBool>) {
        self.activities.stoppable(operation.id, flag);
    }

    /// Bytes of `operation` passed (it counts them when it's a single file's).
    pub(crate) fn bytes_of(&self, operation: Operation, bytes: u64) {
        if operation.unit == workbench_model::ActivityUnit::Bytes {
            self.activities.advance(operation.id, bytes);
        }
    }

    /// A file of `operation` done (it counts them when it's several files').
    pub(crate) fn file_of(&self, operation: Operation) {
        if operation.unit == workbench_model::ActivityUnit::Files {
            self.activities.advance(operation.id, 1);
        }
    }

    /// `operation` ended, with what `done` says of it.
    pub(crate) fn finish(&self, operation: Operation, done: &[Transfer], stopped: bool) {
        let outcome = match done.iter().find_map(|t| t.error.clone()) {
            _ if stopped => workbench_model::Outcome::Stopped,
            Some(why) => workbench_model::Outcome::Failed(why),
            None => workbench_model::Outcome::Done,
        };
        self.activities.end(operation.id, outcome);
    }

    pub fn record(&self, transfer: &Transfer) {
        // Uploaded: its permissions read again (and checked, the setting on).
        if transfer.action == Action::Upload && transfer.error.is_none() {
            self.uploaded_check(&transfer.path);
        }
        if let Ok(mut log) = self.log.lock() {
            log.push_back(transfer.clone());
            while log.len() > LOG {
                log.pop_front();
            }
        }
        self.events.publish(&remote::events::TransferFinished {
            path: transfer.path.clone(),
            action: transfer.action,
            bytes: transfer.bytes,
            error: transfer.error.clone(),
        });
        (self.wake)();
    }

    /// A transfer starts / ends (`started`): announced with what waits.
    pub fn running(&self, started: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        if started {
            self.busy.fetch_add(1, Relaxed);
        } else {
            self.busy.fetch_sub(1, Relaxed);
        }
        self.announce();
    }

    /// What runs and what waits, announced.
    fn announce(&self) {
        self.events.publish(&self.now());
        (self.wake)();
    }

    /// What runs and what waits to upload now.
    pub fn now(&self) -> remote::events::TransfersChanged {
        remote::events::TransfersChanged {
            running: self.busy.load(std::sync::atomic::Ordering::Relaxed),
            to_upload: self.pending.lock().map_or(0, |p| p.len()),
        }
    }

    pub(crate) fn tell(&self, notice: Notice) {
        if let Ok(mut inbox) = self.inbox.lock() {
            inbox.push(notice);
        }
        (self.wake)();
    }

    fn send_job(&self, job: Job) {
        if let Some(jobs) = self.jobs.lock().ok().and_then(|j| j.clone()) {
            let _ = jobs.send(job);
        }
    }

    /// Transfers `relative` one way (a conflict decided, a file brought on demand), the
    /// manifest updated. `Err`: why it failed.
    pub(crate) fn transfer(
        &self,
        relative: &str,
        action: Action,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) -> Result<(), String> {
        let local = local_copy(&self.saved.local);
        if session.is_none() {
            *session = self.connector.connect().ok();
        }
        let server = session.as_mut().and_then(|fs| fs.stat(relative).ok().flatten());
        // Back to the pool before the transfer takes one (never two held at once).
        *session = None;
        let change = Change {
            path: relative.to_owned(),
            action,
            conflict: false,
            chosen: true,
            remote: server,
            local: local.stat(relative),
        };
        self.run(&change, &local)
    }

    /// Applies `change` (one transfer, logged), keeping the manifest. `Err`: why it failed.
    fn run(&self, change: &Change, local: &fs_local::remote_copy::LocalCopy) -> Result<(), String> {
        let done = self.run_all(std::slice::from_ref(change), local);
        match done.first().and_then(|t| t.error.clone()) {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }

    /// Applies `changes` one after the other (logged), keeping the manifest.
    fn run_all(
        &self,
        changes: &[Change],
        local: &fs_local::remote_copy::LocalCopy,
    ) -> Vec<Transfer> {
        self.running(true);
        let operation = self.begin(changes);
        // Stopped from the Task Manager: what's done stays, the rest isn't transferred.
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stoppable(operation, Arc::clone(&stop));
        let stopped = || stop.load(std::sync::atomic::Ordering::Relaxed);
        let run = Run {
            workers: 1,
            retry: self.retry(),
            now: &now,
            on_transfer: &|t| {
                self.record(t);
                self.file_of(operation);
            },
            on_bytes: &|action, n| {
                self.moved(action, n);
                self.bytes_of(operation, n);
            },
            cancelled: &stopped,
            parallel: &threads,
            pause: &std::thread::sleep,
        };
        let mut manifest = self.manifest.lock().map(|m| m.clone()).unwrap_or_default();
        let done = apply(changes, self.connector.as_ref(), local, &mut manifest, &run);
        self.finish(operation, &done, stopped());
        self.running(false);
        self.keep(manifest, &done);
        done
    }

    /// The files the server got from here at `relative` (one) or under it (a folder's).
    pub(crate) fn sent_under(&self, relative: &str) -> Vec<String> {
        let folder = format!("{relative}/");
        let Ok(manifest) = self.manifest.lock() else { return Vec::new() };
        manifest
            .paths()
            .filter(|p| *p == relative || p.starts_with(&folder))
            .map(str::to_owned)
            .collect()
    }

    /// `from` moved here to `to` (a file or a folder): moved on the server, what was sent
    /// under it following in the manifest. `true` if it was.
    fn rename_on_server(
        &self,
        from: &str,
        to: &str,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) -> bool {
        if session.is_none() {
            *session = self.connector.connect().ok();
        }
        let Some(fs) = session.as_mut() else {
            self.tell(Notice::Message(workbench_model::Notification::warning(i18n::t(
                "remote.offline",
            ))));
            return false;
        };
        if let Err(e) = fs.rename(from, to) {
            if e.is_transient() {
                *session = None;
            }
            self.failed(&crate::connect_view::explain(&e));
            return false;
        }
        if let Ok(mut manifest) = self.manifest.lock() {
            let folder = format!("{from}/");
            let moved: Vec<String> = manifest
                .paths()
                .filter(|p| *p == from || p.starts_with(&folder))
                .map(str::to_owned)
                .collect();
            for old in moved {
                let Some(synced) = manifest.get(&old).copied() else { continue };
                let new = format!("{to}{}", &old[from.len()..]);
                manifest.remove(&old);
                manifest.set(&new, synced);
            }
            save_manifest(&self.saved.local, &manifest);
        }
        self.relist_around([from, to].into_iter());
        true
    }

    /// A move waiting in the outbox (`there` on the server is `here` here) made on the
    /// server — the folders it left removed if they're empty now (a folder's files moved
    /// one by one) — then its content sent if it changed here too.
    fn send_move(
        self: &Arc<Self>,
        here: &str,
        there: &str,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        if !self.rename_on_server(there, here, session) {
            self.end_sending(std::iter::once(here));
            return;
        }
        self.change_outbox(|o| o.rename_done(here));
        if let Some(fs) = session.as_mut() {
            let mut dir = parent(there);
            while !dir.is_empty() && !self.manifest.lock().is_ok_and(|m| m.has_under(dir)) {
                // Not empty on the server (what it has that never came here): it stays.
                if fs.remove(dir).is_err() {
                    break;
                }
                dir = parent(dir);
            }
        }
        // Its content follows (the upload ends what was sent), or it's done.
        if self.local_changes_of(here).is_some() {
            self.uploads.push_forced([here.to_owned()]);
        } else {
            self.end_sending(std::iter::once(here));
        }
        crate::outgoing::refresh(self);
    }

    /// The outbox changed by `change`, kept on disk if it did.
    pub(crate) fn change_outbox(&self, change: impl FnOnce(&mut Outbox)) {
        let Ok(mut outbox) = self.outbox.lock() else { return };
        let before = outbox.clone();
        change(&mut outbox);
        if *outbox != before {
            save_outbox(&self.saved.local, &outbox);
        }
    }

    /// A save uploads at once (else it waits for the user).
    pub fn auto_upload(&self) -> bool {
        self.auto_upload.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Saves upload at once (`on`) or wait for the user: kept in the connection (wherever
    /// it's saved), the session going on as it is. `Err`: why it couldn't be kept.
    pub fn set_auto_upload(self: &Arc<Self>, on: bool) -> Result<(), String> {
        let mut saved = config::find(&self.saved.local).unwrap_or_else(|| self.saved.clone());
        saved.connection.auto_upload = on;
        config::save(&saved).map_err(|e| e.to_string())?;
        self.auto_upload.store(on, std::sync::atomic::Ordering::Relaxed);
        crate::outgoing::refresh(self);
        (self.wake)();
        Ok(())
    }

    /// Moved here while uploads wait: kept to do on the server (what it has of `from`).
    fn note_moved(&self, from: &str, to: &str) {
        let sent = self.sent_under(from);
        self.change_outbox(|o| o.moved(from, to, &sent));
    }

    /// Deleted here while uploads wait: kept to delete on the server (asked first then).
    fn note_removed(&self, paths: &[String]) {
        let sent: Vec<(String, Vec<String>)> =
            paths.iter().map(|p| (p.clone(), self.sent_under(p))).collect();
        self.change_outbox(|o| {
            for (path, sent) in &sent {
                o.removed(path, sent);
            }
        });
    }

    /// `paths` deleted here (confirmed): what the server got of them deleted there (logged),
    /// then their folders left empty, the deepest first.
    fn remove_on_server(&self, paths: &[String], session: &mut Option<Box<dyn RemoteFs>>) {
        let files: Vec<String> = paths.iter().flat_map(|p| self.sent_under(p)).collect();
        let changes: Vec<Change> = files
            .iter()
            .map(|path| Change {
                path: path.clone(),
                action: Action::DeleteRemote,
                conflict: false,
                chosen: true,
                remote: None,
                local: None,
            })
            .collect();
        // Never two sessions held at once: this one back to the pool before the removals.
        *session = None;
        let local = local_copy(&self.saved.local);
        let done = self.run_all(&changes, &local);
        if let Some(why) = done.iter().find_map(|t| t.error.clone()) {
            self.failed(&why);
        }
        // Deleted there: no longer waiting (one that failed stays).
        self.change_outbox(|o| {
            for t in done.iter().filter(|t| t.error.is_none()) {
                o.delete_done(&t.path);
            }
        });
        // The folders of the files deleted that aren't here any more (a folder deleted here,
        // its files sent one by one from the list): gone there too, the deepest first. A
        // folder still holding something on the server stays (it's never emptied).
        let folders = gone_folders(&self.saved.local, done.iter().filter(|t| t.error.is_none()));
        if folders.is_empty() {
            return;
        }
        *session = self.connector.connect().ok();
        if let Some(fs) = session.as_mut() {
            for dir in &folders {
                let _ = fs.remove(dir);
            }
        }
        self.relist_around(folders.iter().map(String::as_str));
    }

    /// What transfers `done` changed in `manifest` (a copy they worked on) put into the
    /// session's manifest — only their entries, so transfers running at once never undo each
    /// other's — and kept on disk; what's known of the server follows them, and their folders
    /// are listed again (the tree's marks).
    pub fn keep(&self, manifest: Manifest, done: &[Transfer]) {
        if let Ok(mut kept) = self.manifest.lock() {
            for transfer in done.iter().filter(|t| t.error.is_none()) {
                match manifest.get(&transfer.path) {
                    Some(synced) => kept.set(&transfer.path, *synced),
                    None => kept.remove(&transfer.path),
                }
            }
            // Written while held: two sessions' writes never interleave on disk.
            save_manifest(&self.saved.local, &kept);
        }
        if let Ok(mut listings) = self.listings.lock() {
            for t in done.iter().filter(|t| t.error.is_none()) {
                match manifest.get(&t.path) {
                    Some(s) => learn(
                        &mut listings,
                        RemoteEntry {
                            path: t.path.clone(),
                            is_dir: false,
                            size: s.remote_size,
                            modified: s.remote_modified,
                        },
                    ),
                    None => {
                        if let Some(entries) = listings.get_mut(parent(&t.path)) {
                            entries.retain(|e| e.path != t.path);
                        }
                    }
                }
            }
        }
        self.relist_around(done.iter().map(|t| t.path.as_str()));
    }

    /// The folders holding `paths` (and those above them) listed again in the tree.
    pub(crate) fn relist_around<'a>(&self, paths: impl Iterator<Item = &'a str>) {
        let Ok(mut relisted) = self.relisted.lock() else { return };
        for path in paths {
            let mut dir = path;
            loop {
                dir = parent(dir);
                if !relisted.iter().any(|d| d == dir) {
                    relisted.push(dir.to_owned());
                }
                if dir.is_empty() {
                    break;
                }
            }
        }
        drop(relisted);
        (self.wake)();
    }

    /// A failure told with message `key` (its `{reason}`: `why`).
    pub(crate) fn failed_with(&self, key: &str, why: &str) {
        self.tell(Notice::Message(workbench_model::Notification::error(i18n::tr!(
            key,
            reason = why
        ))));
    }

    pub(crate) fn failed(&self, why: &str) {
        self.tell(Notice::Message(workbench_model::Notification::error(i18n::tr!(
            "remote.failed",
            reason = why
        ))));
    }

    /// `relative` can go to the server: not excluded, not the project's own data.
    pub(crate) fn travels(&self, relative: &str) -> bool {
        !self.saved.connection.is_excluded(relative)
            && relative.split('/').next() != Some(kernel::brand::PROJECT_DIR)
    }

    /// The project is worked on on the server (files brought when opened).
    pub fn on_demand(&self) -> bool {
        self.saved.connection.mode == remote::Mode::OnDemand
    }

    /// On demand: folder `dir` (relative; `""`: the root) and those under it are read from
    /// the server again (the tree shows them when they come).
    pub fn forget(&self, dir: &str) {
        if let Ok(mut listings) = self.listings.lock() {
            listings.retain(|known, _| !remote::application::in_scope(known, dir));
        }
        self.ask_list(dir);
    }

    /// On demand: asks for folder `dir` (relative) to be listed, once.
    fn ask_list(&self, dir: &str) {
        let first = self.asked.lock().is_ok_and(|mut asked| asked.insert(dir.to_owned()));
        if first {
            self.send_job(Job::List(dir.to_owned()));
        }
    }

    /// Lists folder `dir` of the server (tried again on a new session while the network
    /// fails); the tree then shows it.
    fn list(&self, dir: &str, session: &mut Option<Box<dyn RemoteFs>>) {
        let mut listed = None;
        for attempt in 0..self.retry().attempts.max(1) {
            if attempt > 0 {
                std::thread::sleep(self.retry().wait_before(attempt));
            }
            if session.is_none() {
                *session = self.connector.connect().ok();
            }
            let Some(fs) = session.as_mut() else { continue };
            match fs.list(dir) {
                Ok(entries) => {
                    listed = Some(entries);
                    break;
                }
                Err(e) if e.is_transient() => *session = None,
                // A folder made here, not on the server yet: nothing there (no error).
                Err(remote::RemoteError::NotFound(_)) => {
                    listed = Some(Vec::new());
                    break;
                }
                Err(e) => {
                    let e = e.during(&format!("listing /{dir}"));
                    self.failed(&crate::connect_view::explain(&e));
                    break;
                }
            }
        }
        if let Ok(mut asked) = self.asked.lock() {
            asked.remove(dir);
        }
        let Some(entries) = listed else { return };
        self.learn_listing(dir, &entries);
    }

    /// On demand: folder `dir` of the server as just listed (by anyone: the tree, a review
    /// comparing) — the tree shows it (exclusions and the project's data left out).
    pub(crate) fn learn_listing(&self, dir: &str, entries: &[RemoteEntry]) {
        if !self.on_demand() {
            return;
        }
        let connection = &self.saved.connection;
        let entries = entries
            .iter()
            .filter(|e| !connection.is_excluded(&e.path) && e.path != kernel::brand::PROJECT_DIR)
            .cloned()
            .collect();
        if let Ok(mut listings) = self.listings.lock() {
            listings.insert(dir.to_owned(), entries);
        }
        if let Ok(mut relisted) = self.relisted.lock() {
            relisted.push(dir.to_owned());
        }
        (self.wake)();
    }

    /// What a failure adds when the server limited the connections: how many it takes.
    pub fn hint(&self) -> Option<String> {
        let pool = self.pool.as_ref().filter(|p| p.was_lowered())?;
        Some(i18n::tr!("remote.connections_lowered", count = pool.limit()))
    }

    /// What the tree shows of folder `dir` (relative; `path` on disk): on demand, the
    /// server's entries (those not on disk are absent, brought when opened) — asked for, and
    /// none meanwhile, if it wasn't listed yet; and each one on disk marked by where it
    /// stands against its last transfer (only here, changed here, changed there, both) — a
    /// folder by what's to upload under it (a conflict first).
    fn known(&self, dir: &str, path: &Path) -> Vec<workspace::DirEntry> {
        use workspace::{DirEntry, Mark, NodeKind};
        let listed = if self.on_demand() {
            let listed = self.listings.lock().ok().and_then(|l| l.get(dir).cloned());
            if listed.is_none() {
                self.ask_list(dir);
            }
            listed
        } else {
            None
        };
        let server: Option<HashMap<&str, &RemoteEntry>> =
            listed.as_ref().map(|l| l.iter().map(|e| (e.path.as_str(), e)).collect());
        let kind = |is_dir| if is_dir { NodeKind::Directory } else { NodeKind::File };
        // The server's permissions on the rows (the setting on; asked for when not known).
        let notes = self.permission_notes(dir).unwrap_or_default();
        let note = |name: &str| notes.get(name).cloned();
        let mut entries: Vec<DirEntry> = listed
            .iter()
            .flatten()
            .filter_map(|e| {
                let name = e.path.rsplit('/').next()?;
                Some(DirEntry::new(name, kind(e.is_dir)).noted(note(name)))
            })
            .collect();
        let Ok(read) = std::fs::read_dir(path) else { return entries };
        let Ok(manifest) = self.manifest.lock() else { return entries };
        let local = local_copy(&self.saved.local);
        let mut by_name: HashMap<String, usize> =
            entries.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
            if relative == kernel::brand::PROJECT_DIR
                || self.saved.connection.is_excluded(&relative)
            {
                continue;
            }
            let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
            let mark = if meta.is_dir() {
                let there = match &server {
                    Some(server) => server.contains_key(relative.as_str()),
                    // Mirrored: a folder nothing was transferred from is only here.
                    None => self.on_demand() || manifest.has_under(&relative),
                };
                if there { self.folder_mark(&relative) } else { Mark::Local }
            } else {
                let Some(here) = local.stat(&relative) else { continue };
                let there = server.as_ref().map(|s| s.get(relative.as_str()).copied());
                let hash = |p: &str| {
                    (here.size <= HASHED_FOR_MARKS)
                        .then(|| local.read(p).ok().map(|b| remote::content_hash(&b)))
                        .flatten()
                };
                match standing(&here, &manifest, there.map(|r| r.filter(|r| !r.is_dir)), &hash) {
                    // Found changed on the server too when it was to go.
                    _ if self.conflicts.lock().is_ok_and(|c| c.contains(&relative)) => {
                        Mark::Conflict
                    }
                    Standing::Same => Mark::None,
                    Standing::OnlyHere => Mark::Local,
                    Standing::Ahead => Mark::Changed,
                    Standing::Behind => Mark::Behind,
                    Standing::Diverged => Mark::Conflict,
                }
            };
            let noted = note(&name);
            if mark == Mark::None && noted.is_none() {
                continue;
            }
            match by_name.get(&name) {
                Some(&i) => entries[i].mark = mark,
                None => {
                    by_name.insert(name.clone(), entries.len());
                    let entry = DirEntry::new(name, kind(meta.is_dir())).marked(mark);
                    entries.push(entry.noted(noted));
                }
            }
        }
        entries
    }

    /// A folder's mark by what's to upload under it (the list as last read): a conflict, a
    /// change, or nothing.
    fn folder_mark(&self, relative: &str) -> workspace::Mark {
        let under = self.outgoing.under(relative);
        if under.iter().any(|(_, why)| matches!(why, crate::outgoing::Why::Conflict)) {
            workspace::Mark::Conflict
        } else if under.is_empty() {
            workspace::Mark::None
        } else {
            workspace::Mark::Changed
        }
    }

    /// Uploads `relative` (a save), unless the server's copy changed since the last transfer
    /// (the user is asked then).
    pub(crate) fn upload(
        &self,
        relative: &str,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) -> Uploaded {
        // Compared and saved: that's the user's resolution — it goes up as it is.
        if self.resolving.lock().is_ok_and(|mut r| r.remove(relative)) {
            return match self.transfer(relative, Action::Upload, session) {
                Ok(()) => Uploaded::Done,
                Err(why) => Uploaded::Failed(why),
            };
        }
        let local = local_copy(&self.saved.local);
        let Some(here) = local.stat(relative) else { return Uploaded::Gone };
        // The server's copy now (a fresh session if the old one dropped).
        let mut server = None;
        for _ in 0..self.retry().attempts.max(1) {
            if session.is_none() {
                *session = self.connector.connect().ok();
            }
            let Some(fs) = session.as_mut() else { continue };
            match fs.stat(relative) {
                Ok(found) => {
                    server = Some(found);
                    break;
                }
                Err(e) if e.is_transient() => *session = None,
                Err(e) => return Uploaded::Failed(crate::connect_view::explain(&e)),
            }
        }
        let Some(server) = server else { return Uploaded::Offline };
        let changed_there = self.changed_there(relative, server.as_ref());
        if changed_there && let (Some(there), Some(fs)) = (&server, session.as_mut()) {
            // The tree marks it as changed on both sides.
            if let Ok(mut listings) = self.listings.lock() {
                learn(&mut listings, there.clone());
            }
            self.relist_around(std::iter::once(relative));
            if let Ok(mut conflicts) = self.conflicts.lock() {
                conflicts.insert(relative.to_owned());
            }
            let server_text = fs.read(relative).map(|b| String::from_utf8_lossy(&b).into_owned());
            let local_text = local.read(relative).map(|b| String::from_utf8_lossy(&b).into_owned());
            if let (Ok(server_text), Ok(local_text)) = (server_text, local_text) {
                self.tell(Notice::Conflict {
                    path: self.saved.local.join(relative),
                    relative: relative.to_owned(),
                    local: local_text,
                    server: server_text,
                });
            }
            return Uploaded::Conflict;
        }
        // Back to the pool before the transfer takes one (never two held at once).
        *session = None;
        let change = Change {
            path: relative.to_owned(),
            action: Action::Upload,
            conflict: false,
            chosen: true,
            remote: server.map(|s: RemoteEntry| s),
            local: Some(here),
        };
        match self.run(&change, &local) {
            Ok(()) => Uploaded::Done,
            Err(why) => Uploaded::Failed(why),
        }
    }

    /// Uploaded: not pending any more; the server is reachable again.
    pub(crate) fn uploaded(&self, relative: &str) {
        self.settled(relative);
        self.forget_pending(relative);
        self.offline.store(false, std::sync::atomic::Ordering::Relaxed);
        self.failures.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// `relative` still to upload, and why.
    pub(crate) fn keep_pending(&self, relative: &str, why: &str) {
        let added = self
            .pending
            .lock()
            .is_ok_and(|mut p| p.insert(relative.to_owned(), why.to_owned()).is_none());
        if added {
            self.announce();
        }
    }

    pub(crate) fn forget_pending(&self, relative: &str) {
        if self.pending.lock().is_ok_and(|mut p| p.remove(relative).is_some()) {
            self.announce();
        }
    }

    /// What's still to upload, and why it couldn't go.
    pub fn to_upload(&self) -> BTreeMap<String, String> {
        self.pending.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// The files here the server hasn't as they are (changed here, or never sent), but
    /// those excluded: what a Push would send. Reads the disk (a background thread's work).
    pub fn local_changes(&self) -> Vec<(String, Standing)> {
        let local = local_copy(&self.saved.local);
        let Ok(manifest) = self.manifest.lock().map(|m| m.clone()) else { return Vec::new() };
        let connection = &self.saved.connection;
        let mut changes: Vec<(String, Standing)> = local
            .list()
            .into_iter()
            .filter(|e| {
                !connection.is_excluded(&e.path) && !e.path.starts_with(kernel::brand::PROJECT_DIR)
            })
            .filter_map(|entry| {
                Some((entry.path.clone(), changed_here(&local, &manifest, &entry)?))
            })
            .collect();
        changes.sort_by(|a, b| a.0.cmp(&b.0));
        changes
    }

    /// The files here (each once) changed on disk at `from` or after, and before `to`
    /// (`None`: up to now) — but those never sent (excluded, the project's own data) —
    /// newest first, each with whether the server has it as it is. One walk of the local
    /// copy; reads the disk (a background thread's work).
    pub fn changed_between(&self, from: i64, to: Option<i64>) -> Vec<RecentFile> {
        let local = local_copy(&self.saved.local);
        let manifest = self.manifest.lock().map(|m| m.clone()).unwrap_or_default();
        let mut files: Vec<RecentFile> = local
            .list()
            .into_iter()
            .filter(|e| e.modified >= from && to.is_none_or(|to| e.modified < to))
            .filter(|e| self.travels(&e.path))
            .map(|e| RecentFile {
                sent: changed_here(&local, &manifest, &e).is_none(),
                path: e.path,
                modified: e.modified,
                size: e.size,
            })
            .collect();
        files.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
        files
    }

    /// How file `relative` stands if the server hasn't it as it is here (changed here, or
    /// never sent); `None`: it has, or it isn't here.
    pub(crate) fn local_changes_of(&self, relative: &str) -> Option<Standing> {
        let local = local_copy(&self.saved.local);
        let entry = local.stat(relative)?;
        let manifest = self.manifest.lock().ok()?.clone();
        changed_here(&local, &manifest, &entry)
    }

    /// `paths` up now (the user asked: no waiting for quiet, nor for a batch of changes
    /// going on — a paste; the server checked first, as for any save).
    pub fn upload_now(&self, paths: Vec<String>) {
        self.uploads.push_forced(paths);
    }

    /// A conflict decided: `relative` transferred `action`'s way (ours up, or the
    /// server's down), no longer in conflict.
    fn decide(
        self: &Arc<Self>,
        relative: &str,
        action: Action,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        match self.transfer(relative, action, session) {
            Ok(()) => {
                self.settled(relative);
                crate::outgoing::refresh(self);
            }
            Err(why) => self.failed(&why),
        }
    }

    /// `relative` is no longer in conflict (uploaded, or the server's taken).
    fn settled(&self, relative: &str) {
        if let Ok(mut conflicts) = self.conflicts.lock() {
            conflicts.remove(relative);
        }
    }

    /// The server's copy of `relative` (`server`, as it is now) changed since its last
    /// transfer: it appeared there, or its size or date moved.
    fn changed_there(&self, relative: &str, server: Option<&RemoteEntry>) -> bool {
        let known = self.manifest.lock().ok().and_then(|m| m.get(relative).copied());
        match (server, known) {
            (Some(s), Some(k)) => s.size != k.remote_size || s.modified != k.remote_modified,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// `paths` sent by the user: going now (their buttons' loaders turn).
    fn begin_sending<'a>(&self, paths: impl Iterator<Item = &'a str>) {
        use std::sync::atomic::Ordering::Relaxed;
        let Ok(mut sending) = self.sending.lock() else { return };
        let now = self.sending_generation.fetch_add(1, Relaxed) + 1;
        for path in paths {
            sending.insert(path.to_owned(), now);
        }
        drop(sending);
        (self.wake)();
    }

    /// `paths` sent ended (up, failed, or waiting for the user): their loaders stop.
    pub(crate) fn end_sending<'a>(&self, paths: impl Iterator<Item = &'a str>) {
        self.end_sending_since(paths, u64::MAX);
    }

    /// `paths`' loaders stop if they were sent by `started` (the generation when the work
    /// ending began): one sent again since waits for its own.
    pub(crate) fn end_sending_since<'a>(&self, paths: impl Iterator<Item = &'a str>, started: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        let Ok(mut sending) = self.sending.lock() else { return };
        let before = sending.len();
        for path in paths {
            if sending.get(path).is_some_and(|&sent| sent <= started) {
                sending.remove(path);
            }
        }
        if sending.len() != before {
            drop(sending);
            self.sending_generation.fetch_add(1, Relaxed);
            (self.wake)();
        }
    }

    /// `relative`, sent by the user, hasn't ended yet.
    pub fn is_sending(&self, relative: &str) -> bool {
        self.sending.lock().is_ok_and(|s| s.contains_key(relative))
    }

    /// One of `paths`, sent by the user, hasn't ended yet (one look at what's going).
    pub fn sending_among<'a>(&self, mut paths: impl Iterator<Item = &'a str>) -> bool {
        self.sending.lock().is_ok_and(|s| !s.is_empty() && paths.any(|p| s.contains_key(p)))
    }

    /// Something sent by the user hasn't ended yet.
    pub fn sending_any(&self) -> bool {
        self.sending.lock().is_ok_and(|s| !s.is_empty())
    }

    /// How many times what's being sent changed (a view draws again when it moves).
    pub fn sending_generation(&self) -> u64 {
        self.sending_generation.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The listed `rows` sent (the user asked). One alone goes now (in conflict: the user
    /// is asked what to do with it). Several: the server is checked for all of them first —
    /// nothing goes before; files changed there too are marked, and the user chooses to
    /// resolve them first or to upload the others now.
    ///
    /// The check runs on a worker of its own (never behind the session's other jobs: a
    /// listing, a comparison), the user told at once that it started.
    pub fn send(self: &Arc<Self>, rows: &[(String, crate::outgoing::Why)]) {
        if rows.len() > 1 {
            // Going from the check on (its loaders turn while it runs).
            self.begin_sending(sent_paths(rows));
            self.tell(Notice::Message(workbench_model::Notification::info(i18n::tr!(
                "remote.checking",
                count = rows.len()
            ))));
            let (state, rows) = (Arc::clone(self), rows.to_vec());
            kernel::work::run("remote-check", move || {
                let mut session = None;
                state.check_then_send(rows, &mut session);
                (state.wake)();
            });
        } else {
            self.send_now(rows);
        }
    }

    /// The rows several to send, checked first (see [`Self::send`]): none in conflict, they
    /// go; else the user decides. Out of reach, they go (and wait, as any upload).
    fn check_then_send(
        &self,
        rows: Vec<(String, crate::outgoing::Why)>,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        use crate::outgoing::Why;
        let mut conflicts = Vec::new();
        for (path, why) in &rows {
            if matches!(why, Why::Renamed { .. } | Why::Deleted) {
                continue;
            }
            if session.is_none() {
                *session = self.connector.connect().ok();
            }
            let Some(fs) = session.as_mut() else { break };
            match fs.stat(path) {
                Ok(server) if self.changed_there(path, server.as_ref()) => {
                    conflicts.push(path.clone());
                }
                Ok(_) => {}
                Err(e) if e.is_transient() => *session = None,
                Err(_) => {}
            }
        }
        // Back to the pool before the uploads take theirs.
        *session = None;
        if conflicts.is_empty() {
            self.send_now(&rows);
            return;
        }
        if let Ok(mut known) = self.conflicts.lock() {
            known.extend(conflicts.iter().cloned());
        }
        self.relist_around(conflicts.iter().map(String::as_str));
        // The user decides now: nothing goes until then.
        self.end_sending(sent_paths(&rows));
        let rest = rows.into_iter().filter(|(p, _)| !conflicts.contains(p)).collect();
        self.tell(Notice::BulkConflicts { conflicts, rest });
    }

    /// `rows` sent now: files uploaded (the server checked first, as for any save), moves
    /// made on the server (their content following if it changed), deletions asked about
    /// first (all in one question).
    pub(crate) fn send_now(&self, rows: &[(String, crate::outgoing::Why)]) {
        use crate::outgoing::Why;
        self.begin_sending(sent_paths(rows));
        let mut files = Vec::new();
        let mut deleted = Vec::new();
        for (path, why) in rows {
            match why {
                Why::Renamed { from } => self.send_job(Job::SendMove(path.clone(), from.clone())),
                Why::Deleted => deleted.push(path.clone()),
                Why::Waiting(_) | Why::Changed | Why::New | Why::Conflict => {
                    files.push(path.clone());
                }
            }
        }
        if !files.is_empty() {
            self.upload_now(files);
        }
        if !deleted.is_empty() {
            self.tell(Notice::ConfirmRemove(deleted));
        }
    }

    /// File `here` (its name on the server: `there`) compared with the server's copy, read
    /// in the background (the comparison opens when it's here).
    pub fn compare_with_server(&self, here: &str, there: &str) {
        self.send_job(Job::Compare(here.to_owned(), there.to_owned()));
    }

    /// The rows' changes here given up — the user is asked first.
    pub fn discard(&self, rows: Vec<(String, crate::outgoing::Why)>) {
        if !rows.is_empty() {
            self.tell(Notice::ConfirmDiscard(rows));
        }
    }

    /// `paths` (files, folders, `*.ext`) never uploaded any more: added to the connection's
    /// exclusions (the session starts again with them).
    pub fn exclude(&self, paths: Vec<String>) {
        if !paths.is_empty() {
            self.tell(Notice::Exclude(paths));
        }
    }

    /// Reads both sides of `here` (`there` on the server) and has them compared.
    fn compare(&self, here: &str, there: &str, session: &mut Option<Box<dyn RemoteFs>>) {
        if session.is_none() {
            *session = self.connector.connect().ok();
        }
        let Some(fs) = session.as_mut() else {
            self.tell(Notice::Message(workbench_model::Notification::warning(i18n::t(
                "remote.offline",
            ))));
            return;
        };
        let server = match fs.read(there) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(remote::RemoteError::NotFound(_)) => String::new(),
            Err(e) => {
                if e.is_transient() {
                    *session = None;
                }
                self.failed(&crate::connect_view::explain(&e));
                return;
            }
        };
        let local = local_copy(&self.saved.local)
            .read(here)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        self.tell(Notice::Compare {
            path: self.saved.local.join(here),
            relative: here.to_owned(),
            local,
            server,
        });
    }

    /// The rows' changes here given up (confirmed): a file changed here takes the server's
    /// copy back, one made here goes to the trash, a deleted one comes back, a moved one
    /// comes back where it was (the moved copy to the trash).
    fn discard_now(
        self: &Arc<Self>,
        rows: &[(String, crate::outgoing::Why)],
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        use crate::outgoing::Why;
        for (path, why) in rows {
            let done = match why {
                Why::New => to_trash(&self.saved.local.join(path)),
                Why::Renamed { from } => self
                    .transfer(from, Action::Download, session)
                    .and_then(|()| to_trash(&self.saved.local.join(path)))
                    .map(|()| self.change_outbox(|o| o.rename_done(path))),
                Why::Deleted => self.transfer(path, Action::Download, session).map(|()| {
                    self.change_outbox(|o| {
                        o.revived(path);
                    });
                }),
                Why::Waiting(_) | Why::Changed | Why::Conflict => {
                    self.transfer(path, Action::Download, session)
                }
            };
            match done {
                Ok(()) => {
                    self.settled(path);
                    self.forget_pending(path);
                }
                Err(why) => self.failed(&why),
            }
        }
        self.relist_around(rows.iter().map(|(p, _)| p.as_str()));
        crate::outgoing::refresh(self);
    }

    /// Brings `relative` (on demand: a file opened); `done` told when it's here (asked again
    /// meanwhile: brought once, both told).
    pub(crate) fn fetch(&self, relative: String, done: Sender<Result<(), String>>) {
        let first = self.fetching.lock().is_ok_and(|mut waiting| {
            let waiters = waiting.entry(relative.clone()).or_default();
            waiters.push(done);
            waiters.len() == 1
        });
        if first {
            // Someone waits for it: no waiting for quiet.
            self.downloads.push_urgent([relative]);
        }
    }

    /// A file brought (or why not): whoever waited is told.
    pub(crate) fn fetched(&self, relative: &str, outcome: Result<(), String>) {
        let waiting = self.fetching.lock().ok().and_then(|mut w| w.remove(relative));
        for done in waiting.into_iter().flatten() {
            let _ = done.send(outcome.clone());
        }
        (self.wake)();
    }

    /// What the user must hear about now.
    fn take_notices(&self) -> Vec<Notice> {
        self.inbox.lock().map(|mut i| std::mem::take(&mut *i)).unwrap_or_default()
    }
}

impl State {
    /// A session without a project's service (the tests).
    #[cfg(test)]
    pub(crate) fn detached(saved: Saved, connector: Arc<dyn Connector>) -> Self {
        Self::new(ProjectId::new(), saved, connector, Arc::new(|| {}), kernel::EventBus::new())
    }

    #[cfg(test)]
    fn new(
        project: ProjectId,
        saved: Saved,
        connector: Arc<dyn Connector>,
        wake: Arc<dyn Fn() + Send + Sync>,
        events: kernel::EventBus,
    ) -> Self {
        Self::pooled(project, saved, connector, None, wake, events)
    }

    fn pooled(
        project: ProjectId,
        saved: Saved,
        connector: Arc<dyn Connector>,
        pool: Option<Arc<crate::pool::Pool>>,
        wake: Arc<dyn Fn() + Send + Sync>,
        events: kernel::EventBus,
    ) -> Self {
        let queue = || SharedQueue::new(QUIET, MAX_WAIT, HOLD_LIMIT);
        Self {
            project,
            disk: workbench_model::DiskIo::new(),
            activities: workbench_model::Activities::new(std::time::Instant::now),
            manifest: Mutex::new(load_manifest(&saved.local)),
            outbox: Mutex::new(load_outbox(&saved.local)),
            auto_upload: std::sync::atomic::AtomicBool::new(saved.connection.auto_upload),
            saved,
            connector,
            log: Mutex::new(VecDeque::new()),
            inbox: Mutex::new(Vec::new()),
            busy: Default::default(),
            traffic: Default::default(),
            wake,
            jobs: Mutex::new(None),
            listings: Mutex::default(),
            asked: Mutex::default(),
            relisted: Mutex::default(),
            resolving: Mutex::default(),
            conflicts: Mutex::default(),
            uploads: queue(),
            downloads: queue(),
            fetching: Mutex::default(),
            pending: Mutex::default(),
            retrying: Default::default(),
            failures: Default::default(),
            offline: Default::default(),
            events,
            shown: Mutex::new(None),
            outgoing: crate::outgoing::Outgoing::default(),
            badge: Mutex::new(None),
            pool,
            permissions: Mutex::default(),
            permissions_asked: Mutex::default(),
            warned: Mutex::default(),
            to_check: Mutex::default(),
            checking: std::sync::atomic::AtomicBool::new(false),
            sending: Mutex::default(),
            sending_generation: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

/// What of `rows` goes when sent (files and moves; a deletion is asked about first).
fn sent_paths(rows: &[(String, crate::outgoing::Why)]) -> impl Iterator<Item = &str> {
    rows.iter().filter(|(_, why)| *why != crate::outgoing::Why::Deleted).map(|(p, _)| p.as_str())
}

/// The folders (relative) of the files `deleted` on the server that aren't in the local copy
/// at `root` any more, the deepest first: up from each file while its folder is gone here
/// (one still here stops the walk — and everything above it is still here too).
fn gone_folders<'a>(root: &Path, deleted: impl Iterator<Item = &'a Transfer>) -> Vec<String> {
    let here = |dir: &str| dir.split('/').fold(root.to_path_buf(), |p, part| p.join(part)).exists();
    let mut folders = std::collections::BTreeSet::new();
    for transfer in deleted {
        let mut dir = parent(&transfer.path);
        while !dir.is_empty() && !folders.contains(dir) && !here(dir) {
            folders.insert(dir.to_owned());
            dir = parent(dir);
        }
    }
    let mut folders: Vec<String> = folders.into_iter().collect();
    folders.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    folders
}

/// `path` to the system's trash (the tests' scratch files: removed, never in the user's
/// trash). `Err`: why it couldn't go.
fn to_trash(path: &Path) -> Result<(), String> {
    if cfg!(test) {
        return std::fs::remove_file(path).map_err(|e| e.to_string());
    }
    workspace::entries::trash(&fs_local::entries::LocalEntries, &[path.to_path_buf()])
}

/// A file of the local copy changed in a span of time ([`State::changed_between`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentFile {
    /// Relative to the project's root, with `/`.
    pub path: String,
    /// When it last changed here (seconds since the epoch).
    pub modified: i64,
    pub size: u64,
    /// The server has it as it is here.
    pub sent: bool,
}

/// How `entry` of the local copy stands if the server hasn't it as it is here (changed
/// here, or never sent); `None`: it has. Small files are read to tell a touched one from a
/// changed one.
fn changed_here(
    local: &fs_local::remote_copy::LocalCopy,
    manifest: &Manifest,
    entry: &remote::LocalEntry,
) -> Option<Standing> {
    let hash = |p: &str| {
        (entry.size <= HASHED_FOR_MARKS)
            .then(|| local.read(p).ok().map(|b| remote::content_hash(&b)))
            .flatten()
    };
    match standing(entry, manifest, None, &hash) {
        s @ (Standing::OnlyHere | Standing::Ahead) => Some(s),
        _ => None,
    }
}

/// A transfer (or several) followed as an operation: its identity and how it counts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Operation {
    id: u128,
    unit: workbench_model::ActivityUnit,
}

/// The open remote projects' sessions — by the project's identity (never its name or
/// path: the same folder opened again is another project), with its local root (where its
/// files are) — for the commands and views; held weakly: a session ends with its project.
static SESSIONS: Mutex<Vec<(ProjectId, PathBuf, Weak<State>)>> = Mutex::new(Vec::new());

/// The session of project `id`, if it's a remote one.
pub fn session(id: ProjectId) -> Option<Arc<State>> {
    let sessions = SESSIONS.lock().ok()?;
    sessions.iter().find(|(i, ..)| *i == id).and_then(|(.., s)| s.upgrade())
}

/// What the open remote projects' sessions hold, for the Task Manager: the transfers'
/// log and the server's folders read (estimated from their text), their threads' CPU.
#[derive(Debug, Default)]
pub struct RemoteResources;

/// Bytes moved one way, counted on `traffic`.
pub fn count(traffic: &workbench_model::resources::Traffic, action: Action, bytes: u64) {
    match action {
        Action::Upload => traffic.sent(bytes),
        Action::Download => traffic.received(bytes),
        Action::DeleteLocal | Action::DeleteRemote => {}
    }
}

/// Remote projects being opened (the wizard's first download): how many, and what they
/// moved — before any project's session exists.
pub static OPENING: (std::sync::atomic::AtomicUsize, workbench_model::resources::Traffic) =
    (std::sync::atomic::AtomicUsize::new(0), workbench_model::resources::Traffic::new());

/// Attaches a session to each project with a saved connection (none for the others).
#[derive(Debug, Default)]
pub struct RemoteService;

/// The project's data folder is ignored by git — or there's no repository.
pub fn data_ignored(root: &Path) -> bool {
    if !root.join(".git").exists() {
        return true;
    }
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    let dir = kernel::brand::PROJECT_DIR;
    ignore.lines().map(str::trim).any(|line| {
        let line = line.trim_start_matches('/').trim_end_matches('/');
        line == dir || line == format!("{dir}/*") || line == format!("{dir}/**")
    })
}

#[cfg(test)]
#[path = "../tests/unit/session.rs"]
mod tests;
