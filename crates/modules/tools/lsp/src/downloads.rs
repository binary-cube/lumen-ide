//! The language servers downloaded when they're first needed, instead of bundled: Node, PHP
//! and each server, as the app's manifest says (`data/downloads.toml`, made by `cargo xtask
//! lsp-manifest`), installed once into a folder shared by every project, window and version
//! of the app (`<data folder>/lsp/`: `lsp_client::store`) — on the user's click only (the
//! bar over a file, the settings), off the UI thread, each one an operation of the Task
//! Manager's "Language server installs" part (stoppable).
//!
//! Where a server's parts are: the app's own `lsp/` first (a full build, `target/lsp` while
//! developing: `crate::catalog`), then this folder — the version pinned, else an older one
//! left by an earlier version of the app (it still runs; brought up to date in the
//! background, the user having taken it once already).
//!
//! At the first project ([`maintain`], in the background): a 43h install of a server the user
//! installs moved here from the config folder, what's out of date updated, what nothing uses
//! removed (`lsp_client::store::collect`: this app's versions and those another version of
//! the app used in the last 30 days kept), this version's references written.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Instant;

use lsp_client::downloads::{Component, Manifest, Role};
use lsp_client::fetch::{Fetch, FetchError};
use lsp_client::spec::ServerSpec;
use lsp_client::store::{self, Found};
use workbench_model::resources::{ActivityKind, ActivityUnit, Outcome};

/// The app's manifest of what can be downloaded.
const EMBEDDED: &str = include_str!("../data/downloads.toml");

/// Where the shared folder is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    /// The data folder's `lsp/` (not for a test's binary: none).
    Default,
    /// Nowhere: nothing is downloaded (the snapshots, unless they ask).
    Off,
    At(PathBuf),
}

static PLACE: RwLock<Place> = RwLock::new(Place::Default);
/// The manifest, when set (tests).
static MANIFEST: RwLock<Option<Arc<Manifest>>> = RwLock::new(None);

/// A component's install as it's going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
    /// Another window or app is installing it: waited for.
    pub waiting: bool,
}

/// A component as the bar and the settings show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Not downloaded: what it would download.
    Missing {
        size: u64,
    },
    Installed(Found),
    Downloading(Progress),
    Removing,
}

/// Why a component's last install failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub reason: String,
    /// The network is the reason.
    pub offline: bool,
}

#[derive(Default)]
struct Shared {
    /// What was found, by component (looked for once, again after a change).
    found: HashMap<String, Option<Found>>,
    jobs: HashMap<String, Progress>,
    stops: HashMap<String, Arc<AtomicBool>>,
    errors: HashMap<String, Failure>,
    removing: HashSet<String>,
    /// Brought up to date this session already.
    updated: HashSet<String>,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    let mut shared = SHARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(shared.get_or_insert_with(Shared::default))
}

/// Something changed: the bars and the settings draw again (the installs' generation).
fn changed() {
    crate::install::bump();
}

/// The downloads go to `dir` (`None`: nowhere — nothing downloadable) — tests, snapshots:
/// never the user's folder; what was found forgotten.
pub fn use_downloads(dir: Option<PathBuf>) {
    if let Ok(mut place) = PLACE.write() {
        *place = dir.map_or(Place::Off, Place::At);
    }
    with(|s| *s = Shared::default());
    changed();
}

/// What can be downloaded is `manifest` (tests).
pub fn use_manifest(manifest: Manifest) {
    if let Ok(mut m) = MANIFEST.write() {
        *m = Some(Arc::new(manifest));
    }
    with(|s| s.found.clear());
    changed();
}

/// What can be downloaded.
pub fn manifest() -> Arc<Manifest> {
    if let Some(m) = MANIFEST.read().ok().and_then(|m| m.clone()) {
        return m;
    }
    static PARSED: OnceLock<Arc<Manifest>> = OnceLock::new();
    Arc::clone(PARSED.get_or_init(|| {
        Arc::new(Manifest::parse(EMBEDDED).unwrap_or_else(|e| {
            tracing::warn!("the language servers' download manifest can't be read: {e}");
            Manifest::default()
        }))
    }))
}

/// A test's binary (`target/debug/deps/<test>`): never the user's data folder.
fn test_binary() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.file_name().is_some_and(|n| n == "deps")))
        .unwrap_or(false)
}

/// The shared folder, if downloads are possible.
pub fn root() -> Option<PathBuf> {
    match PLACE.read().map(|p| p.clone()).unwrap_or(Place::Off) {
        Place::At(dir) => Some(dir),
        Place::Off => None,
        Place::Default if test_binary() => None,
        Place::Default => storage::data_dir().map(|d| d.join("lsp")),
    }
}

/// Whether the folder is the user's own (not one a test or a snapshot set).
fn default_place() -> bool {
    PLACE.read().is_ok_and(|p| *p == Place::Default)
}

/// The platform things are downloaded for.
pub fn platform() -> &'static str {
    lsp_client::bundle::platform()
}

/// Component `name` where it's installed in the shared folder, if it is (looked for once,
/// a few file reads; again after a change).
pub fn found(name: &str) -> Option<Found> {
    if let Some(known) = with(|s| s.found.get(name).cloned()) {
        return known;
    }
    let root = root()?;
    let manifest = manifest();
    let found = manifest.named(name).and_then(|c| c.find(&root, platform()));
    with(|s| {
        // Being removed: gone already.
        let found = found.clone().filter(|_| !s.removing.contains(name));
        s.found.insert(name.to_owned(), found);
    });
    found
}

/// A runtime's program in the shared folder (`Role::Node`, `Role::Php`), if it's there.
fn runtime(role: Role) -> Option<PathBuf> {
    let manifest = manifest();
    let c = manifest.of(role)?;
    let found = found(&c.name)?;
    let bin = c.bin(platform())?;
    Some(store::path_in(&found.dir, bin)).filter(|p| p.is_file())
}

/// Node downloaded, if it is.
pub fn node() -> Option<PathBuf> {
    runtime(Role::Node)
}

/// PHP downloaded, if it is.
pub fn php() -> Option<PathBuf> {
    runtime(Role::Php)
}

/// Server `name`'s folder and version, downloaded, if it is.
pub fn server(name: &str) -> Option<(PathBuf, String)> {
    found(name).map(|f| (f.dir, f.version))
}

/// What server `spec` needs downloaded to start (component names, Node and PHP first), none
/// when nothing is missing — or when what's missing can't be downloaded (a server the user
/// added, one not made for this platform). One the user installs: only its runtime (Node).
pub fn needed_for(spec: &ServerSpec) -> Vec<String> {
    if root().is_none() {
        return Vec::new();
    }
    let manifest = manifest();
    let bundle = crate::catalog::bundle();
    let mut out = Vec::new();
    let mut want = |role: Role, have: bool| -> bool {
        if have {
            return true;
        }
        match manifest.of(role).filter(|c| c.available(platform())) {
            Some(c) => {
                out.push(c.name.clone());
                true
            }
            None => false,
        }
    };
    let node =
        !spec.uses("node") || bundle.as_ref().and_then(|b| b.node()).is_some() || node().is_some();
    let php =
        !spec.uses("php") || bundle.as_ref().and_then(|b| b.php()).is_some() || php().is_some();
    if !want(Role::Node, node) || !want(Role::Php, php) {
        return Vec::new();
    }
    if spec.uses("server")
        && spec.install.is_none()
        && let Some(name) = spec.bundle.as_deref()
    {
        let bundled = bundle.as_ref().and_then(|b| b.server(name)).is_some();
        if !bundled && found(name).is_none() {
            match manifest.named(name).filter(|c| c.available(platform())) {
                Some(c) => out.push(c.name.clone()),
                None => return Vec::new(),
            }
        }
    }
    out
}

/// Component `name` as it is now.
pub fn status(name: &str) -> Status {
    if let Some(progress) = with(|s| s.jobs.get(name).copied()) {
        return Status::Downloading(progress);
    }
    if with(|s| s.removing.contains(name)) {
        return Status::Removing;
    }
    match found(name) {
        Some(found) => Status::Installed(found),
        None => Status::Missing { size: manifest().named(name).map_or(0, |c| c.size(platform())) },
    }
}

/// Why component `name`'s last install failed, if it did.
pub fn failure(name: &str) -> Option<Failure> {
    with(|s| s.errors.get(name).cloned())
}

/// What downloading components `names` would weigh (bytes), those there already left out.
pub fn size_of(names: &[String]) -> u64 {
    let manifest = manifest();
    names
        .iter()
        .filter(|n| found(n).is_none_or(|f| !f.current))
        .filter_map(|n| manifest.named(n))
        .map(|c| c.size(platform()))
        .sum()
}

/// The space the downloaded components take (bytes).
pub fn used_space() -> u64 {
    manifest().components.iter().filter_map(|c| found(&c.name)).map(|f| f.size).sum()
}

/// What stops component `name`'s install going on.
pub fn stop(name: &str) -> bool {
    with(|s| s.stops.get(name).map(|f| f.store(true, Ordering::Relaxed))).is_some()
}

/// Components `names` installed one after the other in the background; `wake` as they go.
pub fn start(names: Vec<String>, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
    if names.is_empty() {
        return;
    }
    // Shown as going on at once (not when its thread gets to it).
    let manifest = manifest();
    with(|s| {
        for name in &names {
            if !s.jobs.contains_key(name)
                && let Some(c) = manifest.named(name)
            {
                s.errors.remove(name);
                s.jobs.insert(
                    name.clone(),
                    Progress { total: c.size(platform()), ..Progress::default() },
                );
                s.stops.insert(name.clone(), Arc::new(AtomicBool::new(false)));
            }
        }
    });
    changed();
    // A thread of its own, not the pool's: a download lasts minutes (the pool's work is
    // short), one per click at most.
    #[allow(clippy::disallowed_methods)]
    let spawned = std::thread::Builder::new().name("lsp-download".into()).spawn(move || {
        let _ = ensure(&names, wake.as_deref());
    });
    if let Err(e) = spawned {
        tracing::warn!("a download's thread didn't start: {e}");
    }
}

/// Components `names` installed (on this thread: a background one), in order; the first
/// failure ends it.
pub fn ensure(names: &[String], wake: Option<&(dyn Fn() + Send + Sync)>) -> Result<(), FetchError> {
    let mut outcome = Ok(());
    for (i, name) in names.iter().enumerate() {
        if let Err(e) = install(name, wake) {
            // What came after it isn't waited for any more.
            with(|s| {
                for rest in &names[i + 1..] {
                    if s.jobs.get(rest).is_some_and(|p| p.received == 0) {
                        s.jobs.remove(rest);
                        s.stops.remove(rest);
                    }
                }
            });
            outcome = Err(e);
            break;
        }
    }
    changed();
    crate::projects::refresh_all();
    if let Some(wake) = wake {
        wake();
    }
    outcome
}

/// What downloads (set by the tests: never the network there).
fn fetcher() -> Arc<dyn Fetch> {
    crate::install::fetcher()
}

/// Component `name` installed (here: a background thread), or waited for if it's being
/// installed already.
pub fn install(name: &str, wake: Option<&(dyn Fn() + Send + Sync)>) -> Result<Found, FetchError> {
    let root = root().ok_or_else(|| FetchError::Failed("no folder for the downloads".into()))?;
    let manifest = manifest();
    let c = manifest
        .named(name)
        .cloned()
        .ok_or_else(|| FetchError::Failed(format!("{name} isn't downloadable")))?;
    if let Some(found) = found(name).filter(|f| f.current) {
        with(|s| {
            s.jobs.remove(name);
            s.stops.remove(name);
        });
        return Ok(found);
    }
    // One install per component in this process (another one waits for it).
    let claimed = RUNNING.lock().map(|mut r| r.insert(name.to_owned())).unwrap_or(false);
    if !claimed {
        return wait_for(name);
    }
    // Its progress and stop flag made by `start` already, or now.
    let stop = with(|s| {
        s.errors.remove(name);
        s.jobs
            .entry(name.to_owned())
            .or_insert(Progress { total: c.size(platform()), ..Progress::default() });
        Arc::clone(
            s.stops.entry(name.to_owned()).or_insert_with(|| Arc::new(AtomicBool::new(false))),
        )
    });
    changed();
    let total = c.size(platform());
    let activity = crate::install::ACTIVITIES.start(
        None,
        ActivityKind::Download,
        format!("{} {}", c.label(), c.version),
        ActivityUnit::Bytes,
        Some(total),
    );
    crate::install::ACTIVITIES.stoppable(activity, Arc::clone(&stop));
    let counted = AtomicU64::new(0);
    let last = AtomicU64::new(u64::MAX);
    let progress = |received: u64| {
        let before = counted.swap(received, Ordering::Relaxed);
        crate::install::ACTIVITIES.advance(activity, received.saturating_sub(before));
        with(|s| {
            if let Some(p) = s.jobs.get_mut(name) {
                p.received = received;
                p.waiting = false;
            }
        });
        // Drawn again every 256 KB, not at every chunk.
        if received >> 18 != last.swap(received >> 18, Ordering::Relaxed) {
            changed();
            if let Some(wake) = wake {
                wake();
            }
        }
    };
    let waiting = || {
        with(|s| {
            if let Some(p) = s.jobs.get_mut(name) {
                p.waiting = true;
            }
        });
        changed();
        if let Some(wake) = wake {
            wake();
        }
    };
    tracing::info!(component = %c.name, version = %c.version, "downloading a language server's component");
    let started = Instant::now();
    let fetch = fetcher();
    let outcome = lsp_client::downloads::install(
        &root,
        &c,
        platform(),
        fetch.as_ref(),
        &progress,
        &waiting,
        &stop,
    );
    with(|s| {
        s.jobs.remove(name);
        s.stops.remove(name);
        s.found.remove(name);
        match &outcome {
            Ok(_) => {
                s.errors.remove(name);
            }
            Err(FetchError::Stopped) => {}
            Err(e) => {
                s.errors.insert(
                    name.to_owned(),
                    Failure { reason: e.to_string(), offline: e.offline() },
                );
            }
        }
    });
    if let Ok(mut running) = RUNNING.lock() {
        running.remove(name);
    }
    match &outcome {
        Ok(found) => {
            crate::install::ACTIVITIES.end(activity, Outcome::Done);
            tracing::info!(component = %c.name, dir = %found.dir.display(), size = found.size, elapsed = ?started.elapsed(), "language server component downloaded");
            write_refs();
        }
        Err(FetchError::Stopped) => {
            crate::install::ACTIVITIES.end(activity, Outcome::Stopped);
            tracing::info!(component = %c.name, "a language server's download stopped");
        }
        Err(e) => {
            crate::install::ACTIVITIES.end(activity, Outcome::Failed(e.to_string()));
            tracing::warn!(component = %c.name, "a language server's download failed: {e}");
        }
    }
    changed();
    outcome
}

/// The components whose install runs on some thread of this process.
static RUNNING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Component `name`'s install going on elsewhere in this process, waited for.
fn wait_for(name: &str) -> Result<Found, FetchError> {
    loop {
        let running = RUNNING.lock().map(|r| r.contains(name)).unwrap_or(false);
        if !running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    match found(name).filter(|f| f.current) {
        Some(found) => Ok(found),
        None => Err(failure(name)
            .map(|f| {
                if f.offline { FetchError::Offline(f.reason) } else { FetchError::Failed(f.reason) }
            })
            .unwrap_or(FetchError::Stopped)),
    }
}

/// Every server that uses component `c`.
fn uses(c: &Component, spec: &ServerSpec) -> bool {
    match c.role {
        Role::Node => spec.uses("node"),
        Role::Php => spec.uses("php"),
        Role::Server => spec.bundle.as_deref() == Some(c.name.as_str()),
    }
}

/// Removes component `name` from the shared folder (every version of it): the servers
/// using it stop first (their files can't go under them on Windows) — here, a background
/// thread.
pub fn remove(name: &str) -> Result<(), String> {
    let root = root().ok_or("no folder for the downloads")?;
    let manifest = manifest();
    let c = manifest.named(name).cloned().ok_or_else(|| format!("{name} isn't downloadable"))?;
    with(|s| {
        s.removing.insert(name.to_owned());
        s.found.insert(name.to_owned(), None);
        s.errors.remove(name);
    });
    changed();
    // Its servers stop, in every project.
    let clients: Vec<_> =
        crate::projects::all().iter().flat_map(|p| p.let_go(&|spec| uses(&c, spec))).collect();
    let until = Instant::now() + std::time::Duration::from_secs(10);
    while clients.iter().any(|c| !c.ended()) && Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(clients);
    let never = AtomicBool::new(false);
    let outcome = store::Lock::acquire(&root, &c.name, &never, &|| {}).and_then(|_lock| {
        let versions = store::path_in(&root, &c.group());
        let mut result = Ok(());
        for entry in std::fs::read_dir(&versions).into_iter().flatten().flatten() {
            let named = store::marker(&entry.path()).is_some_and(|m| m.name == c.name);
            if named && let Err(e) = store::remove_dir(&entry.path()) {
                result = Err(e);
            }
        }
        // A server's own folder left empty.
        if c.role == Role::Server {
            let _ = std::fs::remove_dir(&versions);
        }
        result
    });
    with(|s| {
        s.removing.remove(name);
        s.found.remove(name);
        if let Err(e) = &outcome {
            s.errors.insert(name.to_owned(), Failure { reason: e.clone(), offline: false });
        }
    });
    write_refs();
    changed();
    crate::projects::refresh_all();
    match &outcome {
        Ok(()) => tracing::info!(component = %name, "a language server's component removed"),
        Err(e) => {
            tracing::warn!(component = %name, "a language server's component can't be removed: {e}")
        }
    }
    outcome
}

/// This app version's references' key: its version and its manifest's fingerprint.
fn refs_key() -> String {
    let digest = ring_free_hash(EMBEDDED.as_bytes());
    format!("{}-{digest:016x}", kernel::brand::version())
}

/// FNV-1a: a stable fingerprint of the manifest (no cryptography needed).
fn ring_free_hash(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// The folders this app version uses (relative to the shared folder): each component where
/// it's found (pinned or older), and the installs of the servers the user installs.
fn used_folders() -> BTreeSet<String> {
    let Some(root) = root() else { return BTreeSet::new() };
    let relative = |dir: &std::path::Path| {
        dir.strip_prefix(&root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
    };
    let mut out: BTreeSet<String> = manifest()
        .components
        .iter()
        .filter_map(|c| c.find(&root, platform()))
        .filter_map(|f| relative(&f.dir))
        .collect();
    let servers = root.join("servers");
    for spec in crate::catalog::catalog().servers.iter() {
        if let Some(install) = &spec.install
            && let Some(found) = lsp_client::install::installed_in(&servers, &spec.name, install)
            && let Some(r) = relative(&found.dir)
        {
            out.insert(r);
        }
    }
    out
}

/// This app version's references written (what it uses kept by every other's cleanup).
fn write_refs() {
    let Some(root) = root() else { return };
    if let Err(e) = store::write_refs(&root, &refs_key(), &used_folders()) {
        tracing::warn!("the language servers' references can't be written: {e}");
    }
}

/// Once per process, at the first project, in the background (see the module's doc).
pub fn maintain(wake: Option<Arc<dyn Fn() + Send + Sync>>) {
    static DONE: std::sync::Once = std::sync::Once::new();
    if root().is_none() {
        return;
    }
    DONE.call_once(|| {
        kernel::work::run("lsp-maintain", move || maintain_now(wake));
    });
}

/// What [`maintain`] does, here (a background thread; tests).
pub fn maintain_now(wake: Option<Arc<dyn Fn() + Send + Sync>>) {
    if default_place()
        && let (Some(root), Some(config)) = (root(), storage::config_dir())
    {
        migrate_from(&config.join("lsp"), &root);
    }
    let Some(root) = root() else { return };
    let keep = used_folders();
    let removed = store::collect(&root, &keep, &refs_key(), std::time::SystemTime::now());
    for path in &removed {
        tracing::info!(path = %path.display(), "unused language server files removed");
    }
    write_refs();
    // What the user took once, out of date: brought up to date.
    let stale: Vec<String> = manifest()
        .components
        .iter()
        .filter(|c| c.available(platform()))
        .filter(|c| found(&c.name).is_some_and(|f| !f.current))
        .filter(|c| with(|s| s.updated.insert(c.name.clone())))
        .map(|c| c.name.clone())
        .collect();
    if !stale.is_empty() {
        tracing::info!(components = ?stale, "updating downloaded language servers");
        start(stale, wake);
    }
}

/// The servers the user installed where 43h put them (`<old>/installed/<server>/`, their
/// data in `<old>/data/<server>`; `<old>`: `<config>/lsp`) moved to the shared folder
/// `root`.
pub fn migrate_from(old: &std::path::Path, root: &std::path::Path) {
    if !old.is_dir() {
        return;
    }
    for (from, to) in
        [(old.join("installed"), root.join("servers")), (old.join("data"), root.join("data"))]
    {
        for entry in std::fs::read_dir(&from).into_iter().flatten().flatten() {
            let target = to.join(entry.file_name());
            let moved = std::fs::create_dir_all(&to).and_then(|()| {
                if target.exists() {
                    // Versions one by one (the folder exists already).
                    for version in std::fs::read_dir(entry.path())?.flatten() {
                        let into = target.join(version.file_name());
                        if !into.exists() {
                            std::fs::rename(version.path(), into)?;
                        }
                    }
                    Ok(())
                } else {
                    std::fs::rename(entry.path(), &target)
                }
            });
            match moved {
                Ok(()) => {
                    tracing::info!(from = %entry.path().display(), to = %target.display(), "an installed language server moved")
                }
                Err(e) => {
                    tracing::warn!(from = %entry.path().display(), "an installed language server can't be moved: {e}")
                }
            }
        }
    }
    let _ = std::fs::remove_dir(old.join("installed"));
    let _ = std::fs::remove_dir(old.join("data"));
    let _ = std::fs::remove_dir(old);
    crate::install::forget();
}

#[cfg(test)]
#[path = "../tests/unit/downloads.rs"]
mod tests;
