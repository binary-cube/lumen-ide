//! Whether draw.io is installed, and its download, for the whole process: every view asks
//! here (memory only, never the disk), the looking and the download run on the
//! `diagram-web-install` worker, and each view is woken (by its paint's waker) whenever what
//! it would show changed — a number bumped at every change says so.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};

use pinned_downloads::fetch::Fetch;

use crate::package;

/// The worker the install's work runs on.
pub const THREAD: &str = "diagram-web-install";

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    /// Not looked for yet, or being looked for.
    #[default]
    Looking,
    /// Not there (or this build can't install it: no data folder).
    Missing,
    /// Being downloaded: what came of how much.
    Downloading { received: u64, total: u64 },
    /// Downloaded, being unpacked.
    Unpacking,
    /// The last try failed: why (said to the user), and whether the network was the reason.
    Failed { reason: String, offline: bool },
    /// Installed in this folder.
    Installed(Arc<Path>),
}

#[derive(Default)]
struct State {
    status: Status,
    looked: bool,
    stop: Option<Arc<AtomicBool>>,
    /// The views to wake (those gone are let go as they're met).
    wakers: Vec<Weak<dyn Fn() + Send + Sync>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Tests: another folder, another source.
static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);
static FETCH: RwLock<Option<Arc<dyn Fetch>>> = RwLock::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(State::default))
}

/// The folder installs go to: `<data>/web-apps/diagram` (or the one a test set; a test's
/// binary never uses the user's).
pub fn root() -> Option<PathBuf> {
    if let Some(dir) = ROOT.read().ok().and_then(|r| r.clone()) {
        return Some(dir);
    }
    if test_binary() {
        return None;
    }
    files::places::data_dir().map(|d| d.join("web-apps").join("diagram"))
}

/// A test's binary (`target/debug/deps/<test>`).
fn test_binary() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.file_name().is_some_and(|n| n == "deps")))
        .unwrap_or(false)
}

/// Tests: installs go under `dir`, downloads come from `fetch`; what was known is forgotten.
pub fn isolate(dir: PathBuf, fetch: Arc<dyn Fetch>) {
    if let Ok(mut r) = ROOT.write() {
        *r = Some(dir);
    }
    if let Ok(mut f) = FETCH.write() {
        *f = Some(fetch);
    }
    with(|s| *s = State::default());
    changed();
}

fn fetcher() -> Arc<dyn Fetch> {
    if let Some(fetch) = FETCH.read().ok().and_then(|f| f.clone()) {
        return fetch;
    }
    static HTTP: std::sync::OnceLock<Arc<dyn Fetch>> = std::sync::OnceLock::new();
    Arc::clone(HTTP.get_or_init(|| Arc::new(pinned_downloads::fetch::Http::new(&package::HOSTS))))
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

/// Bumps the number and wakes the views (never under the lock).
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

/// Where it stands now; looked for on the worker the first time ([`Status::Looking`]
/// meanwhile). Memory only.
pub fn status() -> Status {
    let (status, look) = with(|s| {
        let look = !s.looked;
        s.looked = true;
        (s.status.clone(), look)
    });
    if look {
        kernel::work::run(THREAD, || {
            let found = root().and_then(|r| package::installed(&r));
            let busy = with(|s| matches!(s.status, Status::Downloading { .. } | Status::Unpacking));
            if !busy {
                set(match found {
                    Some(dir) => Status::Installed(Arc::from(dir)),
                    None => Status::Missing,
                });
            }
        });
    }
    status
}

/// Downloads and installs it on the worker (nothing if it's under way or there already).
pub fn start() {
    let Some(root) = root() else {
        set(Status::Failed { reason: "no data folder".into(), offline: false });
        return;
    };
    let stop = Arc::new(AtomicBool::new(false));
    let started = with(|s| {
        if matches!(s.status, Status::Downloading { .. } | Status::Unpacking | Status::Installed(_))
        {
            return false;
        }
        s.status = Status::Downloading { received: 0, total: package::DOWNLOAD_SIZE };
        s.looked = true;
        s.stop = Some(Arc::clone(&stop));
        true
    });
    if !started {
        return;
    }
    changed();
    let fetch = fetcher();
    kernel::work::run(THREAD, move || {
        // Told at most every 1% (each tell wakes every view).
        let last = AtomicU64::new(0);
        let progress = |received: u64, total: Option<u64>| {
            let total = total.unwrap_or(package::DOWNLOAD_SIZE).max(received);
            let step = (total / 100).max(1);
            if received / step != last.load(Ordering::Relaxed) / step || received == total {
                last.store(received, Ordering::Relaxed);
                set(Status::Downloading { received, total });
            }
        };
        let unpacking = || set(Status::Unpacking);
        let done = package::install(&*fetch, &root, &progress, &unpacking, &stop);
        with(|s| s.stop = None);
        set(match done {
            Ok(dir) => Status::Installed(Arc::from(dir)),
            Err(e) if e.stopped() => Status::Missing,
            Err(e) => {
                tracing::info!(error = %e, "draw.io wasn't installed");
                Status::Failed { reason: e.to_string(), offline: e.offline() }
            }
        });
    });
}

/// Stops the download under way (what came is kept, to be resumed).
pub fn stop() {
    if let Some(stop) = with(|s| s.stop.clone()) {
        stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "../tests/unit/install.rs"]
mod tests;
