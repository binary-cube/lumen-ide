//! Pages drawn by the app's own web engine (one host process shared by every window and
//! app instance, `web-engine`): each page is a live image in the window's scene, its frames
//! copied on the GPU as they come. Nothing of it runs on the UI thread but handing data
//! over:
//! - connecting (and starting the host when none runs) is a worker's (`web-engine`); the
//!   pages opened meanwhile wait, their views' loaders turning;
//! - a frame is imported on the connection's reader thread, where it arrives, then the
//!   window is woken — the frame only draws the image already there;
//! - what a page tells (its messages, its links) is queued there and handed to its view's
//!   inbox at the next frame ([`platform_api::WebView::poll`]).
//!
//! Used only while the engine is there (`EDITOR_WEB_HOST`, until its download, plan 99b);
//! otherwise, and once it failed, pages are the system's as before.

//!
//! Its parts: the page ([`page`]: a [`platform_api::WebView`]), where its engine's events go
//! on the reader thread ([`sink`]), the input method's steps ([`ime`]), the accessibility
//! tree and pointers as the window's ([`access`]).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use render_api::FrameImporter;
use web_engine::protocol::Adapter;
use web_engine::{Connection, Launch, install};

mod access;
mod ime;
mod page;
mod sink;

pub(super) use page::{EnginePage, Made};

type Wake = Arc<dyn Fn() + Send + Sync>;

/// How long after a failed start the engine isn't tried again (pages are the system's
/// meanwhile).
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// Pages gone this many times within [`CRASH_WINDOW`] (a page that can't be made, a
/// renderer crashing as it loads): the engine is given up as if it hadn't started.
const CRASH_LIMIT: usize = 3;
/// How far back pages gone are counted toward [`CRASH_LIMIT`].
const CRASH_WINDOW: Duration = Duration::from_secs(30);

/// Frames in a row that couldn't be opened here (the engine draws on another GPU than the
/// window's, a driver that can't share): the pages' frames come as pixels from then on.
const IMPORTS_FAILED: u32 = 3;

/// The GPU's shared textures don't work here (they failed, or the engine runs on another
/// GPU): every page's frames come as pixels for the rest of the run.
static PIXELS: AtomicBool = AtomicBool::new(false);

/// Whether a page's frames come as pixels: the window's GPU can't open the system's shared
/// textures (another graphics API, Linux until its dmabufs), they failed this run, or
/// `EDITOR_WEB_FRAMES=pixels` asks it (to try that way where textures work).
fn pixels_wanted(importer: &dyn FrameImporter) -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let asked =
        *ASKED.get_or_init(|| std::env::var("EDITOR_WEB_FRAMES").is_ok_and(|v| v == "pixels"));
    asked || PIXELS.load(Ordering::Acquire) || !importer.opens_textures()
}

/// From now on, frames as pixels (said once in the log).
fn frames_as_pixels(why: &str) {
    if !PIXELS.swap(true, Ordering::AcqRel) {
        tracing::warn!(why, "the web engine's frames come as pixels from now on");
    }
}

/// An engine built here, named by `EDITOR_WEB_HOST` (its developers'), used instead of the
/// one installed. Looked at once (a click never touches the disk).
fn built_here() -> Option<PathBuf> {
    static PROGRAM: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    PROGRAM
        .get_or_init(|| {
            std::env::var_os("EDITOR_WEB_HOST").map(PathBuf::from).filter(|p| p.is_file())
        })
        .clone()
}

/// How the host is started ([`launch`]).
struct Launching {
    launch: Launch,
    /// The installed version it is (`None`: one built here).
    version: Option<String>,
}

/// The installed version the next start would run (`None`: none installed, or none known).
fn chosen_version() -> Option<String> {
    let root = install::service::root()?;
    install::layout::chosen(&root, web_engine::protocol::PROTOCOL).map(|(_, engine)| engine.version)
}

/// How the host is started now, drawing on `adapter` (the window's GPU): the engine built
/// here when there is one, else the installed version chosen (`None`: none installed). It
/// reads the disk: only on the connecting worker.
fn launch(adapter: Option<Adapter>) -> Option<Launching> {
    let channel = web_engine::channel();
    let root = install::service::root();
    if let Some(program) = built_here() {
        let data = files::places::data_dir().unwrap_or_else(std::env::temp_dir);
        let profile = install::layout::profile(&install::layout::root(&data));
        let launch = Launch { program, channel, profile, hold: None, adapter };
        return Some(Launching { launch, version: None });
    }
    let root = root?;
    let (dir, engine) = install::layout::chosen(&root, web_engine::protocol::PROTOCOL)?;
    let launch = Launch {
        program: install::layout::program(&dir),
        channel,
        profile: install::layout::profile(&root),
        hold: Some(dir.join(install::layout::IN_USE)),
        adapter,
    };
    Some(Launching { launch, version: Some(engine.version) })
}

/// The connection as far as the app got with it.
enum Link {
    Idle,
    /// A worker connects; who to wake when it's done.
    Connecting(Vec<Wake>),
    Ready(Arc<Connection>),
    Failed(Instant),
}

static LINK: Mutex<Link> = Mutex::new(Link::Idle);

/// Whether pages can be drawn by the engine now (installed, not failed lately): read from
/// memory only.
pub(super) fn usable() -> bool {
    // Frames cross processes as textures on macOS and Windows, as pixels anywhere.
    if !cfg!(any(target_os = "macos", windows, target_os = "linux")) {
        return false;
    }
    let there = built_here().is_some()
        || match install::service::status() {
            install::service::Status::Installed { .. } => true,
            // A page wants it: its download is offered (once a run).
            install::service::Status::Missing => {
                install::service::want();
                false
            }
            _ => false,
        };
    there && !failed_lately()
}

/// The engine's install as a window last saw it (each window moves its own pages).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct EngineSeen {
    /// The install's generation (its status read again only when it moves).
    generation: u64,
    /// It was installed and usable then (its pages moved to it).
    installed: bool,
}

/// The engine became usable since a window last asked (`seen`, its own): the pages the
/// system shows can be made again by it. Only as it comes — into installed from not, and
/// usable — never at another change of its install (an update looked for, one failed, a
/// start told). Memory only; the status read only when the generation moved.
pub(super) fn became_usable(seen: &mut EngineSeen) -> bool {
    if !newer(&mut seen.generation, install::service::generation()) {
        return false;
    }
    let installed =
        matches!(install::service::status(), install::service::Status::Installed { .. });
    came(seen, installed, usable)
}

/// Whether the engine came now (`installed` now, it wasn't as `seen`, and `usable` says
/// so): kept as seen only then, or as it goes (installed but not usable yet: told again at
/// the next change).
fn came(seen: &mut EngineSeen, installed: bool, usable: impl FnOnce() -> bool) -> bool {
    if !installed {
        seen.installed = false;
        return false;
    }
    if seen.installed || !usable() {
        return false;
    }
    seen.installed = true;
    true
}

/// Whether `generation` is another than the one `seen` (kept as seen now).
fn newer(seen: &mut u64, generation: u64) -> bool {
    std::mem::replace(seen, generation) != generation
}

/// The engine failed (it didn't start, its pages kept going) within [`RETRY_AFTER`].
fn failed_lately() -> bool {
    matches!(&*LINK.lock().unwrap_or_else(PoisonError::into_inner),
        Link::Failed(at) if at.elapsed() < RETRY_AFTER)
}

/// The host a page was on ended (`host`: its connection): counted once toward
/// [`CRASH_LIMIT`] however many pages it had — the next to see it end isn't counted.
fn host_gone(host: &Weak<Connection>) {
    static LAST: Mutex<Option<Weak<Connection>>> = Mutex::new(None);
    let first = first_of(&mut LAST.lock().unwrap_or_else(PoisonError::into_inner), host);
    if first {
        page_gone();
    }
}

/// Whether `host` is another than the `last` one counted (kept as counted now). A `Weak`
/// keeps its allocation, so another host is never at the same address.
fn first_of<T>(last: &mut Option<Weak<T>>, host: &Weak<T>) -> bool {
    if last.as_ref().is_some_and(|l| Weak::ptr_eq(l, host)) {
        return false;
    }
    *last = Some(Weak::clone(host));
    true
}

/// A page's engine went (its renderer crashed, or its host ended: [`host_gone`]): past
/// [`CRASH_LIMIT`] of them lately, the engine is given up for a while (its pages made again
/// are the system's).
fn page_gone() {
    static GONE: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
    let mut gone = GONE.lock().unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    gone.retain(|at| now.duration_since(*at) < CRASH_WINDOW);
    gone.push(now);
    if gone.len() >= CRASH_LIMIT {
        gone.clear();
        drop(gone);
        tracing::warn!("the web engine's pages keep going: the system's engine for a while");
        *LINK.lock().unwrap_or_else(PoisonError::into_inner) = Link::Failed(now);
    }
}

/// The GPU the window draws on, as the engine is told it (its frames open only there).
fn adapter_of(importer: &dyn FrameImporter) -> Option<Adapter> {
    importer.adapter().map(|a| Adapter { high: a.high, low: a.low })
}

/// The connection if it's there; else one is asked for on a worker (`wake` called when
/// it's done), its host drawing on `adapter`, and `None` now. Never blocks.
fn connection(wake: &Wake, adapter: Option<Adapter>) -> Option<Arc<Connection>> {
    let mut link = LINK.lock().unwrap_or_else(PoisonError::into_inner);
    match &mut *link {
        Link::Ready(connection) if connection.is_alive() => return Some(Arc::clone(connection)),
        Link::Connecting(waiting) => {
            // Each waker once: a page asks at every poll while it waits (each page has a
            // waker of its own, so a window with several is woken once for each).
            if !waiting.iter().any(|w| Arc::ptr_eq(w, wake)) {
                waiting.push(Arc::clone(wake));
            }
            return None;
        }
        Link::Failed(at) if at.elapsed() < RETRY_AFTER => return None,
        Link::Ready(_) | Link::Failed(_) | Link::Idle => {}
    }
    *link = Link::Connecting(vec![Arc::clone(wake)]);
    drop(link);
    kernel::work::run("web-engine", move || {
        let started = Instant::now();
        // The version started (none: one already ran, or one built here).
        let mut launched = None;
        let made = web_engine::shared(adapter, || {
            let Launching { launch, version } = launch(adapter)?;
            launched = version;
            Some(launch)
        });
        // An installed version's start told to its history (two failed: the one before),
        // only as the version that answered: a host of another version (started meanwhile by
        // another window) says nothing of this one.
        if let Some(version) = &launched {
            let outcome = match &made {
                Ok(connection) if connection.engine() != version.as_str() => None,
                made => Some(install::service::StartOutcome::of(made)),
            };
            if let Some(outcome) = outcome {
                install::service::started(version, outcome);
            }
        }
        let next = match made {
            Ok(connection) => {
                tracing::info!(elapsed = ?started.elapsed(), engine = connection.engine(), "the web engine is connected");
                // A host of an older version still runs (an update came since it started):
                // pages use it until it ends with its last page; the next start is the new one.
                if let Some(chosen) = chosen_version()
                    && chosen != connection.engine()
                {
                    tracing::info!(running = connection.engine(), chosen = %chosen, "an older web engine still runs");
                }
                Link::Ready(connection)
            }
            // A host already drawing on another GPU (another window's): its textures can't
            // open here; its frames come as pixels (any GPU), joined again at once.
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
                frames_as_pixels("the web engine draws on another GPU");
                Link::Idle
            }
            Err(e) => {
                tracing::warn!(error = %e, "the web engine didn't connect");
                Link::Failed(Instant::now())
            }
        };
        let waiting = match std::mem::replace(
            &mut *LINK.lock().unwrap_or_else(PoisonError::into_inner),
            next,
        ) {
            Link::Connecting(waiting) => waiting,
            _ => Vec::new(),
        };
        for wake in waiting {
            wake();
        }
    });
    None
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_engine/mod.rs"]
mod tests;
