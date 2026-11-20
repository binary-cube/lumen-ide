//! The servers the user installs (described with `install`, not bundled: intelephense): where
//! they are — the app's own install, in the shared folder of the downloaded servers
//! (`<data>/lsp/servers/<server>/<version>/`, see `crate::downloads`; one left by 43h in
//! `<config>/lsp/installed/` is moved there, and found there meanwhile), else one the user
//! made (the program in PATH, npm's global folder) — looked for once, off the UI thread, and
//! kept; their install from the settings (downloaded only on the user's click,
//! from registry.npmjs.org, checked against the pinned SHA-512: `lsp_client::install`), with
//! its progress for the settings and as an operation in the Task Manager; their uninstall.
//!
//! Once a server is found or installed, every project's documents go to the engines chosen
//! then ([`crate::projects::refresh_all`]): a language whose default is that server (PHP on
//! Windows) starts it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use lsp_client::install::{self as installer, Fetch, Installed};
use lsp_client::spec::ServerSpec;
use workbench_model::resources::{Activities, ActivityKind, ActivityUnit, Outcome};

/// Where a server that isn't bundled is, as last looked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Found {
    /// Not looked for yet.
    #[default]
    Unknown,
    /// Being looked for.
    Looking,
    /// Neither installed by the app nor found elsewhere.
    Missing,
    Installed(Installed),
}

/// A server's install as the settings show it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    pub found: Found,
    /// Being installed: received, total bytes.
    pub installing: Option<(u64, Option<u64>)>,
    /// Being removed.
    pub removing: bool,
    /// Why the last install or removal failed.
    pub error: Option<String>,
    /// npm's global folder was looked in too.
    pub asked_npm: bool,
}

#[derive(Default)]
struct Shared {
    servers: HashMap<String, State>,
    /// What stops the install going on (by server).
    stops: HashMap<String, Arc<AtomicBool>>,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
/// Bumped at every change (the settings' block is drawn again).
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Where the installs are, when set (tests, snapshots: never the user's; nothing looked for
/// in PATH nor with npm then).
static INSTALLS: RwLock<Option<PathBuf>> = RwLock::new(None);
/// What downloads (tests: a fake registry).
static FETCH: RwLock<Option<Arc<dyn Fetch>>> = RwLock::new(None);

/// The installs' operations (the Task Manager's).
pub static ACTIVITIES: Activities = Activities::new(Instant::now);

fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    let mut shared = SHARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(shared.get_or_insert_with(Shared::default))
}

fn changed() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Something the settings show of the installs changed (a licence key saved).
pub fn bump() {
    changed();
}

/// Said where a server's description names no licence page.
pub const REGISTRY_NOTE: &str = "registry.npmjs.org";

/// The installs are under `dir` (tests, snapshots), what was found forgotten; nothing is
/// looked for outside it.
pub fn use_install_dir(dir: PathBuf) {
    if let Ok(mut installs) = INSTALLS.write() {
        *installs = Some(dir);
    }
    with(|s| *s = Shared::default());
    changed();
}

/// Downloads go through `fetch` (tests: a fake registry, never the network).
pub fn use_fetch(fetch: Arc<dyn Fetch>) {
    if let Ok(mut f) = FETCH.write() {
        *f = Some(fetch);
    }
}

fn isolated() -> bool {
    INSTALLS.read().is_ok_and(|i| i.is_some())
}

/// The folder of the app's installs: `<data>/lsp/servers` (or the one set).
pub fn installs_dir() -> Option<PathBuf> {
    if let Some(dir) = INSTALLS.read().ok().and_then(|i| i.clone()) {
        return Some(dir);
    }
    crate::downloads::root().map(|r| r.join("servers"))
}

/// Where 43h installed them (`<config>/lsp/installed`): moved from there at the first project,
/// looked in meanwhile.
fn legacy_installs_dir() -> Option<PathBuf> {
    storage::config_dir().map(|d| d.join("lsp").join("installed"))
}

/// What downloads: the one set (tests: files in memory), else the network.
pub fn fetcher() -> Arc<dyn Fetch> {
    if let Some(fetch) = FETCH.read().ok().and_then(|f| f.clone()) {
        return fetch;
    }
    static HTTP: std::sync::OnceLock<Arc<dyn Fetch>> = std::sync::OnceLock::new();
    Arc::clone(HTTP.get_or_init(|| Arc::new(lsp_client::fetch::Http::default())))
}

/// What was found forgotten (looked for again: an install moved).
pub fn forget() {
    with(|s| {
        for state in s.servers.values_mut() {
            if state.installing.is_none() && !state.removing {
                state.found = Found::Unknown;
                state.asked_npm = false;
            }
        }
    });
    changed();
    crate::projects::refresh_all();
}

/// Server `name`'s folder in the app's data folder, for every project (`${data}`):
/// `<data>/lsp/data/<name>` (beside the installs set, when they are).
pub fn data_dir(name: &str) -> Option<PathBuf> {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    installs_dir().and_then(|i| i.parent().map(|p| p.join("data").join(safe)))
}

/// What changed since: a number bumped at every change.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Server `spec`'s install as it is now (looked for in the background the first time:
/// [`Found::Looking`] meanwhile).
pub fn state(spec: &ServerSpec) -> State {
    let state = with(|s| s.servers.get(&spec.name).cloned().unwrap_or_default());
    if state.found == Found::Unknown {
        look(spec, false, None);
        return State { found: Found::Looking, ..state };
    }
    state
}

/// Where server `spec` is installed, if it is (looked for in the background the first time:
/// `None` meanwhile). Cheap: never waits.
pub fn installed(spec: &ServerSpec) -> Option<Installed> {
    spec.install.as_ref()?;
    match state(spec).found {
        Found::Installed(found) => Some(found),
        _ => None,
    }
}

/// Whether server `spec` can be offered: bundled ones always, installable ones once found.
pub fn available(spec: &ServerSpec) -> bool {
    spec.install.is_none() || installed(spec).is_some()
}

/// Looks for server `spec` in the background: the app's folder, then (not isolated) the
/// program in PATH, then — when `npm` — npm's global folder. `wake` once it's known.
pub fn look(spec: &ServerSpec, npm: bool, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
    let Some(install) = spec.install.clone() else { return };
    let name = spec.name.clone();
    let start = with(|s| {
        let state = s.servers.entry(name.clone()).or_default();
        if state.found == Found::Looking || state.installing.is_some() || state.removing {
            return false;
        }
        if npm && state.asked_npm || !npm && state.found != Found::Unknown {
            return false;
        }
        let before = std::mem::replace(&mut state.found, Found::Looking);
        if npm {
            state.asked_npm = true;
            // Shown as it was while npm is asked.
            state.found = before;
        }
        true
    });
    if !start {
        return;
    }
    kernel::work::run("lsp-install", move || {
        let isolated = isolated();
        let ours = installs_dir().and_then(|dir| installer::installed_in(&dir, &name, &install));
        let found = ours.or_else(|| {
            if isolated {
                return None;
            }
            // Where 43h put it, not moved yet.
            if let Some(legacy) =
                legacy_installs_dir().and_then(|dir| installer::installed_in(&dir, &name, &install))
            {
                return Some(legacy);
            }
            let root = if npm { installer::npm_global_root() } else { None };
            installer::find_existing(&install, root.as_deref())
        });
        let found = found.map_or(Found::Missing, Found::Installed);
        tracing::info!(server = %name, found = ?found, "an installable language server looked for");
        let new = with(|s| {
            let state = s.servers.entry(name.clone()).or_default();
            let new = state.found != found;
            // An install or removal meanwhile knows better.
            if state.installing.is_none() && !state.removing {
                state.found = found;
            }
            new
        });
        changed();
        if new {
            crate::projects::refresh_all();
        }
        if let Some(wake) = wake {
            wake();
        }
    });
}

/// Installs server `spec` (downloads it): on this thread (a background one), `wake` as it
/// goes. `Ok` with where it went; the error said otherwise.
pub fn install(
    spec: &ServerSpec,
    wake: Option<&(dyn Fn() + Send + Sync)>,
) -> Result<Installed, String> {
    let install = spec.install.clone().ok_or("not installable")?;
    let dir = installs_dir().ok_or("no data folder")?;
    let name = spec.name.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let busy = with(|s| {
        let state = s.servers.entry(name.clone()).or_default();
        if state.installing.is_some() || state.removing {
            return true;
        }
        state.installing = Some((0, install.size));
        state.error = None;
        s.stops.insert(name.clone(), Arc::clone(&stop));
        false
    });
    if busy {
        return Err("busy".into());
    }
    changed();
    let activity = ACTIVITIES.start(
        None,
        ActivityKind::Download,
        format!("{} {}", install.npm, install.version),
        ActivityUnit::Bytes,
        install.size,
    );
    ACTIVITIES.stoppable(activity, Arc::clone(&stop));
    let fetch = fetcher();
    let last = AtomicU64::new(0);
    let counted = AtomicU64::new(0);
    let progress = |received: u64, total: Option<u64>| {
        let before = counted.swap(received, Ordering::Relaxed);
        ACTIVITIES.advance(activity, received.saturating_sub(before));
        if let Some(total) = total {
            ACTIVITIES.set_total(activity, total);
        }
        with(|s| {
            if let Some(state) = s.servers.get_mut(&name) {
                state.installing = Some((received, total.or(install.size)));
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
    tracing::info!(server = %name, version = %install.version, "installing a language server");
    let outcome = installer::install(&dir, &name, &install, fetch.as_ref(), &progress, &stop);
    with(|s| {
        s.stops.remove(&name);
        let state = s.servers.entry(name.clone()).or_default();
        state.installing = None;
        match &outcome {
            Ok(found) => state.found = Found::Installed(found.clone()),
            Err(e) => state.error = Some(e.clone()),
        }
    });
    match &outcome {
        Ok(found) => {
            ACTIVITIES.end(activity, Outcome::Done);
            tracing::info!(server = %name, dir = %found.dir.display(), "language server installed");
        }
        Err(e) if stop.load(Ordering::Relaxed) => {
            ACTIVITIES.end(activity, Outcome::Stopped);
            tracing::info!(server = %name, error = %e, "language server install stopped");
        }
        Err(e) => {
            ACTIVITIES.end(activity, Outcome::Failed(e.clone()));
            tracing::warn!(server = %name, error = %e, "language server install failed");
        }
    }
    changed();
    crate::projects::refresh_all();
    if let Some(wake) = wake {
        wake();
    }
    outcome
}

/// Removes the app's install of server `spec`: its servers stop first (their files can't be
/// removed under them on Windows), then its folder goes — on this thread (a background
/// one).
pub fn uninstall(spec: &ServerSpec) -> Result<(), String> {
    let dir = installs_dir().ok_or("no data folder")?;
    let name = spec.name.clone();
    let ours = with(|s| {
        let state = s.servers.entry(name.clone()).or_default();
        let ours = matches!(&state.found, Found::Installed(i) if i.ours);
        if ours {
            state.removing = true;
            state.found = Found::Missing;
            state.error = None;
        }
        ours
    });
    if !ours {
        return Err("not installed by the app".into());
    }
    changed();
    // No longer offered: its servers stop.
    crate::projects::refresh_all();
    let until = Instant::now() + std::time::Duration::from_secs(10);
    while crate::projects::any_running(&name) && Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let outcome = installer::uninstall(&dir, &name);
    with(|s| {
        let state = s.servers.entry(name.clone()).or_default();
        state.removing = false;
        // Found elsewhere, maybe: looked for again.
        state.found = Found::Unknown;
        state.asked_npm = false;
        if let Err(e) = &outcome {
            state.error = Some(e.clone());
        }
    });
    changed();
    match &outcome {
        Ok(()) => tracing::info!(server = %name, "language server uninstalled"),
        Err(e) => tracing::warn!(server = %name, error = %e, "language server uninstall failed"),
    }
    outcome
}

/// Every server the user installs looked for now, and whether their secrets are set, waited
/// for (at most `timeout`: the snapshots, which draw the settings once).
pub fn wait_known(timeout: std::time::Duration) {
    let until = Instant::now() + timeout;
    let catalog = crate::catalog::catalog();
    for spec in catalog.servers.iter().filter(|s| s.install.is_some()) {
        while matches!(state(spec).found, Found::Unknown | Found::Looking) && Instant::now() < until
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for id in spec.secret_options.values() {
            while crate::licence::is_set(id, None).is_none() && Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
}

/// Stops the install of server `name` going on.
pub fn stop(name: &str) -> bool {
    with(|s| s.stops.get(name).map(|f| f.store(true, Ordering::Relaxed))).is_some()
}

#[cfg(test)]
#[path = "../tests/unit/install.rs"]
mod tests;
