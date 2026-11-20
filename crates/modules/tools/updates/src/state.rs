//! The updates' state, one for the whole app: where it is (nothing yet, a version found,
//! downloading it, ready to be put in place), whether it's looking, and what a window should
//! tell next — taken by the first window that looks ([`take_tell`]), so the app says it once.
//! Every change is made under one lock from what's there then (a look's result never
//! overwrites a download begun meanwhile, a download's end only its own); the windows are
//! woken after the lock is let go.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use self_update::check::Available;
use self_update::install::Outcome;
use self_update::stage::Staged;
use workbench_model::NotificationId;

/// Where the updates are.
#[derive(Debug, Clone, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// A version worth offering.
    Available(Arc<Available>),
    /// Its file coming.
    Downloading { available: Arc<Available>, received: u64, total: u64 },
    /// Downloaded and unpacked: put in place when the app restarts.
    Ready { available: Arc<Available>, staged: Staged },
}

impl Phase {
    /// The version found, whatever is being done with it.
    pub fn available(&self) -> Option<&Arc<Available>> {
        match self {
            Self::Available(a)
            | Self::Downloading { available: a, .. }
            | Self::Ready { available: a, .. } => Some(a),
            Self::Idle => None,
        }
    }
}

/// What a window tells next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tell {
    /// A version found (its notification, with What's New and Update).
    Available,
    /// Asked by the user, and none newer.
    UpToDate,
    /// The download's progress.
    Progress,
    /// Ready: restart to put it in place.
    Ready,
    /// Something went wrong (the user's words).
    Failed(String),
    /// The app was started again after an update: how it went.
    Updated(Outcome),
}

/// How a look ended.
#[derive(Debug)]
pub enum Looked {
    Found(Box<Available>),
    UpToDate,
    /// No network: nothing said unless the user asked (the user's words).
    Offline(String),
    /// The user's words for what went wrong.
    Failed(String),
}

#[derive(Default)]
struct State {
    phase: Phase,
    /// Looking now (`Some(true)`: the user asked, its answer is said).
    looking: Option<bool>,
    tell: Option<Tell>,
    /// The notification standing for the update (updated as it goes, not shown again).
    notification: Option<NotificationId>,
    /// Each open window's wake.
    wakes: Vec<(u128, Arc<dyn Fn() + Send + Sync>)>,
    /// Stops the download running.
    stop: Option<Arc<AtomicBool>>,
    /// The periodic look, waiting for its next time.
    later: Option<kernel::work::Later>,
    /// Why the last download failed (the user's words; gone when one begins again).
    failed: Option<String>,
    /// What's New is open: it shows the download itself (its notifications wait).
    notes_open: bool,
    /// A look the user asked for, its spinner shown (the notification, since when): the
    /// answer is told in the window showing it.
    spinner: Option<(NotificationId, std::time::Instant)>,
    /// When what's to tell was given (an unfocused window takes it only after a while).
    told_at: Option<std::time::Instant>,
    /// A wake asked for later (the app in the background: no window took it then).
    rewake: Option<kernel::work::Later>,
    /// The staged update already put in the app's place (its folder): only a restart left.
    placed: Option<std::path::PathBuf>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(State::default))
}

/// The windows woken (after the lock is let go).
fn wake_all() {
    let wakes: Vec<_> = with(|s| s.wakes.iter().map(|(_, w)| Arc::clone(w)).collect());
    for wake in wakes {
        wake();
    }
}

pub fn phase() -> Phase {
    with(|s| s.phase.clone())
}

/// `tell` told next, the phase as it is.
pub fn tell(tell: Tell) {
    with(|s| set_tell(s, tell));
    wake_all();
}

/// `tell` the next to tell, from now.
fn set_tell(s: &mut State, tell: Tell) {
    s.tell = Some(tell);
    s.told_at = Some(std::time::Instant::now());
}

/// A look begins (`manual`: the user asked): `false` if none should — one is going already
/// (asked by the user meanwhile: its answer will be said), or the version found is being
/// downloaded or is ready (the user asking is told where it is).
pub fn begin_look(manual: bool) -> bool {
    let (begun, told) = with(|s| {
        if let Some(asked) = &mut s.looking {
            *asked |= manual;
            return (false, false);
        }
        let busy = match s.phase {
            Phase::Downloading { .. } => Some(Tell::Progress),
            Phase::Ready { .. } => Some(Tell::Ready),
            Phase::Idle | Phase::Available(_) => None,
        };
        if let Some(tell) = busy {
            if manual {
                set_tell(s, tell);
            }
            return (false, manual);
        }
        s.looking = Some(manual);
        (true, false)
    });
    if told {
        wake_all();
    }
    begun
}

/// The look ended as `looked`: what it found taken unless a download began meanwhile; told
/// as asked (the same version again isn't told again by itself).
pub fn end_look(looked: Looked) {
    with(|s| {
        let manual = s.looking.take().unwrap_or(false);
        if matches!(s.phase, Phase::Downloading { .. } | Phase::Ready { .. }) {
            return;
        }
        match looked {
            Looked::Found(found) => {
                let again =
                    s.phase.available().is_some_and(|a| a.release.version == found.release.version);
                s.phase = Phase::Available(Arc::from(found));
                if manual || !again {
                    set_tell(s, Tell::Available);
                }
            }
            Looked::UpToDate => {
                s.phase = Phase::Idle;
                if manual {
                    set_tell(s, Tell::UpToDate);
                }
            }
            Looked::Offline(why) | Looked::Failed(why) => {
                if manual {
                    set_tell(s, Tell::Failed(why));
                }
            }
        }
    });
    wake_all();
}

/// The version found starts downloading (only from there): it, and the download's stop.
pub fn begin_download() -> Option<(Arc<Available>, Arc<AtomicBool>)> {
    let begun = with(|s| {
        let Phase::Available(available) = &s.phase else { return None };
        let available = Arc::clone(available);
        let stop = Arc::new(AtomicBool::new(false));
        s.stop = Some(Arc::clone(&stop));
        s.failed = None;
        let total = available.file.size;
        s.phase = Phase::Downloading { available: Arc::clone(&available), received: 0, total };
        set_tell(s, Tell::Progress);
        Some((available, stop))
    });
    if begun.is_some() {
        wake_all();
    }
    begun
}

/// The download of `version` ended: ready, or back to found with why said — only if it's
/// still that download (skipped meanwhile: nothing). Stopped: nothing said.
pub fn end_download(version: &app_updates::Version, ended: Result<Staged, String>) {
    with(|s| {
        let Phase::Downloading { available, .. } = &s.phase else { return };
        if available.release.version != *version {
            return;
        }
        let available = Arc::clone(available);
        let stopped = s.stop.take().is_some_and(|stop| stop.load(Ordering::Relaxed));
        match ended {
            Ok(staged) => {
                s.phase = Phase::Ready { available, staged };
                set_tell(s, Tell::Ready);
            }
            Err(why) => {
                s.phase = Phase::Available(available);
                if !stopped {
                    s.failed = Some(why.clone());
                    set_tell(s, Tell::Failed(why));
                }
            }
        }
    });
    wake_all();
}

/// The staged update in `dir` is in the app's place now.
pub fn mark_placed(dir: &std::path::Path) {
    with(|s| s.placed = Some(dir.to_path_buf()));
}

/// Whether the staged update in `dir` is in the app's place already.
pub fn is_placed(dir: &std::path::Path) -> bool {
    with(|s| s.placed.as_deref() == Some(dir))
}

/// The version found let go (skipped): a download running stops, nothing is offered.
pub fn let_go() {
    with(|s| {
        if let Some(stop) = s.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        s.phase = Phase::Idle;
    });
}

/// The download's progress, told only when it moves a percent (no wake for every chunk).
pub fn progress(received: u64, total: u64) {
    let moved = with(|s| match &mut s.phase {
        Phase::Downloading { received: r, total: t, .. } => {
            let percent = |n: u64, of: u64| n.saturating_mul(100).checked_div(of).unwrap_or(0);
            let moved = percent(received, total) != percent(*r, *t);
            (*r, *t) = (received, total);
            if moved {
                set_tell(s, Tell::Progress);
            }
            moved
        }
        _ => false,
    });
    if moved {
        wake_all();
    }
}

/// Why the last download failed (`None`: it didn't, or another began since).
pub fn download_failed() -> Option<String> {
    with(|s| s.failed.clone())
}

/// What's New opened or closed. Closed while the version found downloads or is ready: its
/// notification told then (it follows the download from there).
pub fn notes_open(open: bool) {
    let tell = with(|s| {
        s.notes_open = open;
        match (&s.phase, open) {
            (Phase::Downloading { .. }, false) => Some(Tell::Progress),
            (Phase::Ready { .. }, false) => Some(Tell::Ready),
            _ => None,
        }
    });
    if let Some(tell) = tell {
        self::tell(tell);
    }
}

/// What's New is open (the download's notifications wait).
pub fn is_notes_open() -> bool {
    with(|s| s.notes_open)
}

/// How long the answer waits for the window showing the spinner (closed, or that window
/// gone: then any window tells it).
const SPINNER_HOLD: std::time::Duration = std::time::Duration::from_secs(10);
/// How long what's to tell waits for the focused window (the app in the background: then
/// any window tells it).
const FOCUS_HOLD: std::time::Duration = std::time::Duration::from_millis(1500);

/// The user asked for a look in a window showing notification `id` (its spinner).
pub fn spinning(id: NotificationId) {
    with(|s| s.spinner = Some((id, std::time::Instant::now())));
}

/// What to tell, taken by a window: the one showing the spinner if one was asked for (by
/// `shows`: that window shows that notification), else the `focused` one — any once it
/// waited ([`FOCUS_HOLD`]: the app in the background; a wake asked for then).
pub fn take_tell_for(shows: impl Fn(NotificationId) -> bool, focused: bool) -> Option<Tell> {
    let (spinner, told_at) = with(|s| (s.spinner, s.told_at));
    if let Some((id, since)) = spinner
        && since.elapsed() < SPINNER_HOLD
    {
        if !shows(id) {
            return None;
        }
    } else if !focused && told_at.is_some_and(|at| at.elapsed() < FOCUS_HOLD) {
        let waiting = with(|s| s.tell.is_some() && s.rewake.is_none());
        if waiting {
            let later = kernel::work::Later::run(crate::checker::WAITING, FOCUS_HOLD, || {
                with(|s| s.rewake = None);
                wake_all();
            });
            with(|s| s.rewake = later);
        }
        return None;
    }
    with(|s| {
        let tell = s.tell.take();
        if tell.is_some() {
            s.spinner = None;
        }
        tell
    })
}

/// What to tell, any window taking it (the tests').
#[cfg(test)]
pub fn take_tell() -> Option<Tell> {
    take_tell_for(|_| true, true)
}

/// The notification standing for the update: the one shown, or a new one (kept).
pub fn notification() -> (NotificationId, bool) {
    with(|s| match s.notification {
        Some(id) => (id, false),
        None => {
            let id = NotificationId::new();
            s.notification = Some(id);
            (id, true)
        }
    })
}

/// The update's notification forgotten (the next one is a new one).
pub fn forget_notification() -> Option<NotificationId> {
    with(|s| s.notification.take())
}

/// Window `id`'s wake kept while it's open (with a project or none); `true` for the first
/// one (the looks begin).
pub fn add_wake(id: u128, wake: Arc<dyn Fn() + Send + Sync>) -> bool {
    with(|s| {
        s.wakes.retain(|(p, _)| *p != id);
        s.wakes.push((id, wake));
        s.wakes.len() == 1 && s.later.is_none()
    })
}

pub fn remove_wake(id: u128) {
    with(|s| s.wakes.retain(|(p, _)| *p != id));
}

/// The periodic look's next time kept.
pub fn keep_later(later: Option<kernel::work::Later>) {
    with(|s| s.later = later);
}

/// The tests' state: emptied (they run one at a time: [`serial`]).
#[cfg(test)]
pub(crate) fn reset() {
    *STATE.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The phase set as it is (tests, and a made-up version for the app's snapshot).
pub(crate) fn set(phase: Phase, tell: Option<Tell>) {
    with(|s| {
        s.phase = phase;
        s.tell = tell;
    });
}

/// The state is the process's: tests touching it run one at a time.
#[cfg(test)]
pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    let guard = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    reset();
    guard
}

#[cfg(test)]
#[path = "../tests/unit/state.rs"]
mod tests;
