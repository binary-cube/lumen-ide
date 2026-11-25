//! Where the engine's install stands, for the whole app (one for every window): looked for
//! on a worker the first time it's asked, downloaded when the user chooses, kept up to date
//! in the background. The app reads it to know whether its pages can be drawn by the
//! engine and says when a page wanted it ([`want`]); whoever offers the download (a module)
//! follows it ([`follow`], [`generation`]) and starts it ([`start`]). Memory only, but for
//! the workers it starts. Nothing is looked for until the app says where its data is
//! ([`place_under`]): a program that never says so (a test's) never touches the user's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};

use pinned_downloads::fetch::Fetch;

use super::{Ask, Going, InstallError, Newest, layout};

/// The worker the install's work runs on.
pub const THREAD: &str = "web-engine-install";

type Wake = Arc<dyn Fn() + Send + Sync>;

/// The project's public key (Ed25519, hex): the engine's list is trusted only when signed
/// by its secret half, as the app's own releases are.
pub use pinned_downloads::signature::PUBLIC_KEY;

/// Why the last try failed, as the user is told it (each kind its own text, in the module
/// that shows it): never a path, an address or a message of the system's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// No connection, or the host couldn't be reached.
    Offline,
    /// The app has no data folder to put it in.
    NoDataFolder,
    /// No engine is made for this system (or for this app's protocol).
    NoPackage,
    /// The list isn't the project's, or can't be read.
    Untrusted,
    /// What came isn't what the list pinned, or isn't whole.
    Damaged,
    /// The server refused it, or sent more than it should.
    Server,
    /// It couldn't be unpacked or put in its place (the disk).
    Disk,
}

impl Failure {
    /// The kind of `error`. A stop isn't a failure ([`start`] says it apart: missing again,
    /// or no update to tell), so it's never shown; it's said as the disk's only to be one.
    pub fn of(error: &InstallError) -> Self {
        use pinned_downloads::fetch::FetchError;
        match error {
            e if e.stopped() => Self::Disk,
            InstallError::Fetch(e) if e.offline() => Self::Offline,
            InstallError::Fetch(FetchError::Mismatch(_)) => Self::Damaged,
            InstallError::Fetch(FetchError::Failed(_)) => Self::Disk,
            InstallError::Fetch(_) => Self::Server,
            InstallError::Manifest(_) => Self::Untrusted,
            InstallError::NoPackage => Self::NoPackage,
            InstallError::Incomplete(_) => Self::Damaged,
            InstallError::Unpack(_) | InstallError::Stopped => Self::Disk,
        }
    }
}

/// How a start of an installed version went, as its history counts it: only a failure of
/// the engine itself (its process ended, or never listened) counts against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// It started and greeted.
    Ran,
    /// Its process ended before it listened or greeted, or it didn't say what it is.
    EngineFailed,
    /// It didn't start for another reason (another GPU, another user's program on the
    /// channel, an old host ending, a slow first start): nothing said of the version.
    NotTheEngine,
}

impl StartOutcome {
    /// How the connection the start was made for went ([`crate::shared`]'s result).
    pub fn of<T>(made: &std::io::Result<T>) -> Self {
        match made {
            Ok(_) => Self::Ran,
            Err(e) if crate::connection::engine_failed(e) => Self::EngineFailed,
            Err(_) => Self::NotTheEngine,
        }
    }
}

/// Where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    /// Not looked for yet, or being looked for.
    #[default]
    Looking,
    /// Not there (or this build can't install it: no data folder, no engine for this
    /// system).
    Missing,
    /// Being downloaded: what came of how much.
    Downloading { received: u64, total: u64 },
    /// Downloaded, being unpacked and checked.
    Unpacking,
    /// The last try failed: why, as its kind (said to the user in their language).
    Failed(Failure),
    /// Installed: its version and its folder.
    Installed { version: String, dir: Arc<Path> },
}

impl Status {
    /// Work under way (a download, an unpacking).
    pub fn busy(&self) -> bool {
        matches!(self, Self::Downloading { .. } | Self::Unpacking)
    }

    /// The version installed (`None`: none is).
    fn version(&self) -> Option<&str> {
        match self {
            Self::Installed { version, .. } => Some(version),
            _ => None,
        }
    }
}

/// How the last update ended (a newer engine looked for while one is installed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Updated {
    /// A newer one installed: its version (pages use it from the engine's next start).
    Newer(String),
    /// The one installed is the newest there is for it.
    UpToDate,
    /// None could be looked for or had: why.
    Failed(Failure),
}

#[derive(Default)]
struct State {
    status: Status,
    looked: bool,
    /// A page wanted the engine while it wasn't there (the download is offered).
    wanted: bool,
    /// An update downloads in the background (the installed one stays in use meanwhile).
    updating: bool,
    /// How the last update ended (`None`: one runs, or none ran this run).
    updated: Option<Updated>,
    stop: Option<Arc<AtomicBool>>,
    wakers: Vec<Weak<dyn Fn() + Send + Sync>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The engine's place: said by the app ([`place_under`]) or a test ([`isolate`]).
static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);
/// Tests: another source, another key.
static FETCH: RwLock<Option<Arc<dyn Fetch>>> = RwLock::new(None);
static KEY: RwLock<Option<String>> = RwLock::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(State::default))
}

/// The engine's place: `<data>/web-engine` once the app said where its data is
/// ([`place_under`]; a test's, [`isolate`]). `None` until then: nothing is looked for.
pub fn root() -> Option<PathBuf> {
    ROOT.read().ok().and_then(|r| r.clone())
}

/// The engine goes under the app's data folder `data` (said once at the app's start; a
/// place a test chose stays).
pub fn place_under(data: &std::path::Path) {
    if let Ok(mut root) = ROOT.write()
        && root.is_none()
    {
        *root = Some(layout::root(data));
    }
}

/// Tests: the engine goes under `dir`, downloads come from `fetch`, the list is trusted
/// when signed by `public`'s key; what was known is forgotten.
pub fn isolate(dir: PathBuf, fetch: Arc<dyn Fetch>, public: &str) {
    if let Ok(mut r) = ROOT.write() {
        *r = Some(dir);
    }
    if let Ok(mut f) = FETCH.write() {
        *f = Some(fetch);
    }
    if let Ok(mut k) = KEY.write() {
        *k = Some(public.to_owned());
    }
    with(|s| *s = State::default());
    changed();
}

fn fetcher() -> Arc<dyn Fetch> {
    if let Some(fetch) = FETCH.read().ok().and_then(|f| f.clone()) {
        return fetch;
    }
    static HTTP: std::sync::OnceLock<Arc<dyn Fetch>> = std::sync::OnceLock::new();
    Arc::clone(HTTP.get_or_init(|| Arc::new(pinned_downloads::fetch::Http::new(&super::HOSTS))))
}

/// The key the list must be signed with: the project's (a test's own, isolated).
fn public_key() -> String {
    KEY.read().ok().and_then(|k| k.clone()).unwrap_or_else(|| PUBLIC_KEY.to_owned())
}

/// What changed since: a number bumped at every change.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// `wake` is called at every change from now on (while it lives).
pub fn follow(wake: &Wake) {
    let weak = Arc::downgrade(wake);
    with(|s| {
        s.wakers.retain(|w| w.strong_count() > 0);
        if !s.wakers.iter().any(|w| w.ptr_eq(&weak)) {
            s.wakers.push(weak);
        }
    });
}

/// Bumps the number and wakes the followers (never under the lock).
fn changed() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
    let wakers: Vec<Wake> = with(|s| s.wakers.iter().filter_map(Weak::upgrade).collect());
    for wake in wakers {
        wake();
    }
}

fn set(status: Status) {
    with(|s| s.status = status);
    changed();
}

/// `status` set unless work is under way (a download started meanwhile stands), decided
/// and written at once.
fn settle(status: Status) {
    let settled = with(|s| {
        let free = !s.status.busy();
        if free {
            s.status = status;
        }
        free
    });
    if settled {
        changed();
    }
}

/// Where it stands now; looked for on the worker the first time ([`Status::Looking`]
/// meanwhile), what's left of old versions and broken downloads cleaned then. Memory only.
pub fn status() -> Status {
    let (status, look) = with(|s| {
        let look = !s.looked;
        s.looked = true;
        (s.status.clone(), look)
    });
    if look {
        kernel::work::run(THREAD, || {
            let found = root().map(|root| {
                layout::clean(&root);
                installed(&root)
            });
            settle(found.flatten().unwrap_or(Status::Missing));
        });
    }
    status
}

/// The chosen engine under `root`, as a status.
fn installed(root: &Path) -> Option<Status> {
    let (dir, engine) = layout::chosen(root, web_engine_protocol::PROTOCOL)?;
    Some(Status::Installed { version: engine.version, dir: Arc::from(dir) })
}

/// A page wanted the engine and it isn't there, or isn't known to be yet (a page made at
/// the app's start asks before the first look ended): whoever offers it is told (once a run),
/// the offer made once it's found missing.
pub fn want() {
    let told = with(|s| {
        let new = !s.wanted && matches!(s.status, Status::Missing | Status::Looking);
        s.wanted |= new;
        new
    });
    if told {
        changed();
    }
}

/// A page wanted the engine while it wasn't there ([`want`]).
pub fn wanted() -> bool {
    with(|s| s.wanted)
}

/// Downloads and installs the newest engine on the worker. With one installed (an update),
/// it stays in use and is said installed until the new one is whole; a failure then leaves
/// it as it was, and how the update ended is kept ([`updated`]). `false`: nothing was done,
/// work is under way already (with no data folder, the failure is said: `true`).
pub fn start() -> bool {
    let Some(root) = root() else {
        set(Status::Failed(Failure::NoDataFolder));
        return true;
    };
    let stop = Arc::new(AtomicBool::new(false));
    let before = with(|s| {
        if s.status.busy() || s.updating {
            return None;
        }
        let before = s.status.version().map(str::to_owned);
        if before.is_none() {
            s.status = Status::Downloading { received: 0, total: 0 };
        }
        s.updating = before.is_some();
        s.updated = None;
        s.looked = true;
        s.stop = Some(Arc::clone(&stop));
        Some(before)
    });
    let Some(before) = before else { return false };
    changed();
    let fetch = fetcher();
    kernel::work::run(THREAD, move || {
        let update = before.is_some();
        let done = run(Run { fetch: &*fetch, root: &root, stop: &stop, update });
        if let Err(e) = &done {
            tracing::info!(error = %e, update, "the web engine wasn't installed");
        }
        // How it ended, written at once (one change told).
        with(|s| {
            s.stop = None;
            s.updating = false;
            match (done, before) {
                (Ok(status), Some(before)) => {
                    let newer = status.version().filter(|now| *now != before).map(str::to_owned);
                    s.updated = Some(newer.map_or(Updated::UpToDate, Updated::Newer));
                    s.status = status;
                }
                (Ok(status), None) => s.status = status,
                // An update stopped: nothing to tell of it (the one installed stands).
                (Err(e), Some(_)) if e.stopped() => s.updated = None,
                (Err(e), Some(_)) => s.updated = Some(Updated::Failed(Failure::of(&e))),
                (Err(e), None) if e.stopped() => s.status = Status::Missing,
                (Err(e), None) => s.status = Status::Failed(Failure::of(&e)),
            }
        });
        changed();
    });
    true
}

/// An update looks for a newer engine in the background now (the one installed in use).
pub fn updating() -> bool {
    with(|s| s.updating)
}

/// How the last update ended (`None`: one runs, or none ran this run).
pub fn updated() -> Option<Updated> {
    with(|s| s.updated.clone())
}

/// What a download is made with.
struct Run<'a> {
    /// Where the list and the package come from.
    fetch: &'a dyn Fetch,
    /// The engine's place.
    root: &'a Path,
    /// Set by the user: the download, the unpacking and the warm-up end.
    stop: &'a AtomicBool,
    /// One is installed: the progress isn't told, it stands until the new one is chosen.
    update: bool,
}

/// The newest package found and installed. What it is now.
fn run(Run { fetch, root, stop, update }: Run<'_>) -> Result<Status, InstallError> {
    let public = public_key();
    let ask = Ask {
        repository: kernel::brand::REPOSITORY,
        public: &public,
        root,
        protocol: web_engine_protocol::PROTOCOL,
        app: kernel::brand::version(),
    };
    let package = match super::newest(fetch, ask)? {
        Newest::Install(package) => package,
        Newest::UpToDate { below_minimum } => {
            if below_minimum {
                tracing::warn!(
                    "the web engine installed is older than the safe minimum, and nothing replaces it yet"
                );
            }
            return installed(root).ok_or(InstallError::NoPackage);
        }
    };
    // Told at most every 1% (each tell wakes every follower).
    let last = AtomicU64::new(0);
    let progress = |received: u64, total: Option<u64>| {
        let total = total.unwrap_or(package.size).max(received);
        let step = (total / 100).max(1);
        if !update && (received / step != last.load(Ordering::Relaxed) / step || received == total)
        {
            last.store(received, Ordering::Relaxed);
            set(Status::Downloading { received, total });
        }
    };
    let unpacking = || {
        if !update {
            set(Status::Unpacking);
        }
    };
    let going = Going { fetch, progress: &progress, unpacking: &unpacking, stop };
    let dir = super::install(going, root, &package)?;
    warm(&layout::program(&dir), stop);
    // What the new one replaced goes once nothing runs from it.
    layout::clean(root);
    installed(root).ok_or(InstallError::NoPackage)
}

/// The engine just installed started once, only to load its files and end (`--warm`): the
/// system checks a new program's files at its first start (on macOS ~2 s for the engine's
/// framework), paid here in the background rather than by the first page that needs it.
/// `stop` set meanwhile ends it (the engine stays installed: the first page pays the check).
///
/// Looked at every [`WARM_POLL`] rather than waited on: a child waited for on another thread
/// can't be killed from this one, and only for the seconds of one warm-up an install.
fn warm(program: &Path, stop: &AtomicBool) {
    warm_within(program, stop, WARM_WAIT);
}

/// [`warm`], given up (its child ended) past `limit`.
fn warm_within(program: &Path, stop: &AtomicBool, limit: std::time::Duration) {
    let started = std::time::Instant::now();
    let spawned = std::process::Command::new(program)
        .arg("--warm")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            tracing::warn!(error = %e, "the web engine didn't warm up");
            return;
        }
    };
    // A start that hangs is let go: the install goes on (the first page pays the check).
    while started.elapsed() < limit {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            tracing::info!(elapsed = ?started.elapsed(), "the web engine's warm-up stopped");
            return;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                tracing::info!(elapsed = ?started.elapsed(), "the web engine warmed up");
                return;
            }
            Ok(Some(status)) => {
                tracing::warn!(%status, "the web engine didn't warm up");
                return;
            }
            Ok(None) => std::thread::sleep(WARM_POLL),
            Err(e) => {
                tracing::warn!(error = %e, "the web engine didn't warm up");
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    tracing::warn!(elapsed = ?started.elapsed(), "the web engine's warm-up was given up");
}

/// The longest the warm-up may take (it takes ~2 s on macOS the first time).
const WARM_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
/// How often the warm-up is looked at.
const WARM_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Stops the download under way (what came is kept, to be resumed).
pub fn stop() {
    if let Some(stop) = with(|s| s.stop.clone()) {
        stop.store(true, Ordering::Relaxed);
    }
}

/// The engine removed (Settings): every version no host runs from, and the pages' storage
/// once none is left, on the worker; it's downloaded again when wanted. Nothing while a
/// download (an update's too) is under way, nor with no place for it: `false`.
pub fn remove() -> bool {
    let Some(root) = root() else { return false };
    if with(|s| s.status.busy() || s.updating) {
        return false;
    }
    kernel::work::run(THREAD, move || {
        let forgotten = layout::update_state(&root, |state| {
            *state = layout::State {
                bad: std::mem::take(&mut state.bad),
                judged_by: state.judged_by.take(),
                ..Default::default()
            };
        });
        if let Err(e) = forgotten {
            tracing::warn!(error = %e, "the web engine's choice wasn't forgotten");
        }
        layout::clean(&root);
        if layout::version_dirs(&root).is_empty() {
            let _ = std::fs::remove_dir_all(layout::profile(&root));
        }
        with(|s| s.wanted = false);
        settle(Status::Missing);
    });
    true
}

/// A start of `version` went as `outcome`: told to its history (only the engine's own
/// failures count), and a version given up for the one before is said as such (the app
/// then starts that one).
pub fn started(version: &str, outcome: StartOutcome) {
    let Some(root) = root() else { return };
    let version = version.to_owned();
    kernel::work::run(THREAD, move || match outcome {
        StartOutcome::Ran => {
            // It worked: the one kept in case it didn't goes (once nothing runs from it).
            if let Err(e) = layout::worked(&root, &version) {
                tracing::warn!(error = %e, "the web engine's start wasn't kept");
            }
            layout::clean(&root);
        }
        StartOutcome::EngineFailed => {
            match layout::failed(&root, &version, kernel::brand::version()) {
                Ok(true) => {
                    tracing::warn!(version = %version, "a web engine that didn't start given up for the one before");
                    settle(installed(&root).unwrap_or(Status::Missing));
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "the web engine's failed start wasn't kept"),
            }
        }
        StartOutcome::NotTheEngine => {}
    });
}

#[cfg(test)]
#[path = "../../tests/unit/install/service.rs"]
mod tests;
