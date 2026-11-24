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

mod listing;
mod operations;
mod permissions;
mod reconnect;
mod resources;
mod sending;
mod server_changes;
mod service;
mod uploads;
pub(crate) use reconnect::reconnect_later;
pub(crate) use service::is_waiting;
use service::*;

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
        tracing::warn!(error = %e, "can't keep the remote moves and deletions waiting");
    }
}

pub fn load_manifest(root: &Path) -> Manifest {
    Manifest::from_text(&std::fs::read_to_string(manifest_file(root)).unwrap_or_default())
}

pub fn save_manifest(root: &Path, manifest: &Manifest) {
    // Atomic: a manifest cut short would make every file look new or in conflict.
    if let Err(e) = files::write_atomic(&manifest_file(root), manifest.to_text().as_bytes()) {
        tracing::warn!(error = %e, "can't keep the remote manifest");
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
#[path = "../../tests/unit/session.rs"]
mod tests;
