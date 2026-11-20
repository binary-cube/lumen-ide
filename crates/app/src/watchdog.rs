//! Event loop watchdog: if handling an event takes too long (the loop is shared by all
//! windows), logs which event is holding it and for how long, so a hang can be found and
//! fixed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The event being handled now, and since when.
static CURRENT: Mutex<Option<(&'static str, Instant)>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);
/// The watchdog sleeps until an event starts (nothing being handled: nothing to watch).
static PARKED: AtomicBool = AtomicBool::new(false);
static STARTS: Condvar = Condvar::new();

/// Beyond this, handling an event is reported as a hang.
const SLOW: Duration = Duration::from_secs(2);

/// Beyond this, handling an event is noted as a hitch (the UI stuttered): what to move off
/// the loop next.
const HITCH: Duration = Duration::from_millis(150);

/// Marks the current event (until the end of the block); starts the watchdog on the
/// first call.
pub fn busy(name: &'static str) -> Busy {
    if !STARTED.swap(true, Ordering::Relaxed) {
        let _ = std::thread::Builder::new().name("watchdog".into()).spawn(watch);
    }
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some((name, Instant::now()));
    }
    if PARKED.load(Ordering::Relaxed) {
        STARTS.notify_one();
    }
    Busy
}

/// Runs `f`, which waits for the user (a native context menu or file dialog runs its own
/// event loop until closed): that time isn't a hang, so the event's clock restarts after.
pub fn waiting_for_user<R>(f: impl FnOnce() -> R) -> R {
    let name = CURRENT.lock().ok().and_then(|mut c| c.take()).map(|(name, _)| name);
    let result = f();
    if let (Some(name), Ok(mut current)) = (name, CURRENT.lock()) {
        *current = Some((name, Instant::now()));
    }
    result
}

/// The event loop has ended: what follows (dropping the windows' parts, the process's own
/// end) is watched as one last event, never finished; slower than [`SLOW`], it is logged
/// and its stacks saved, so what keeps a closed app running can be seen. Still running at
/// [`EXIT_DEADLINE`], the process ends there: with no window left, the system would show it
/// as running in the background and close it itself, asking the user about it.
pub fn exiting() {
    std::mem::forget(busy("exiting"));
    let _ = std::thread::Builder::new().name("exit-deadline".into()).spawn(|| {
        std::thread::sleep(EXIT_DEADLINE);
        tracing::error!(after = ?EXIT_DEADLINE, "the app was still ending: ended now");
        storage::settings::flush();
        logging::flush();
        std::process::exit(0);
    });
}

/// How long ending the app may take once the event loop has ended (its stacks are sampled
/// from [`SLOW`] for 2 s, before it).
const EXIT_DEADLINE: Duration = Duration::from_secs(5);

/// End of handling the event (on drop).
pub struct Busy;

impl Drop for Busy {
    fn drop(&mut self) {
        let Some((name, since)) = CURRENT.lock().ok().and_then(|mut c| c.take()) else { return };
        let elapsed = since.elapsed();
        if elapsed >= SLOW {
            tracing::warn!(event = name, ?elapsed, "the event loop was blocked");
        } else if elapsed >= HITCH {
            tracing::info!(event = name, ?elapsed, "a slow event (the UI stuttered)");
        }
    }
}

fn watch() {
    let mut reported: Option<Instant> = None;
    loop {
        // While events come, looked at twice a second; none being handled (the app idle),
        // it sleeps until the next one starts.
        let current = {
            let guard = CURRENT.lock().unwrap_or_else(PoisonError::into_inner);
            let guard = if guard.is_none() {
                PARKED.store(true, Ordering::Relaxed);
                let guard = STARTS
                    .wait_while(guard, |c| c.is_none())
                    .unwrap_or_else(PoisonError::into_inner);
                PARKED.store(false, Ordering::Relaxed);
                guard
            } else {
                guard
            };
            let (guard, _) = STARTS
                .wait_timeout(guard, Duration::from_millis(500))
                .unwrap_or_else(PoisonError::into_inner);
            *guard
        };
        match current {
            Some((name, since)) if since.elapsed() >= SLOW && reported != Some(since) => {
                tracing::error!(event = name, elapsed = ?since.elapsed(), "the event loop is blocked");
                reported = Some(since);
                sample_stacks();
            }
            _ => {}
        }
    }
}

/// While the loop is still blocked: the stacks of every thread for 2 s, saved next to the
/// log (`hang.txt`), so the code holding the loop can be seen. macOS only (`sample` ships
/// with the system); elsewhere the log line is all there is.
fn sample_stacks() {
    #[cfg(target_os = "macos")]
    {
        let Some(dir) = crate::config::config_dir() else { return };
        let path = dir.join("hang.txt");
        let status = std::process::Command::new("/usr/bin/sample")
            .arg(std::process::id().to_string())
            .args(["2", "-mayDie", "-file"])
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if status.is_ok_and(|s| s.success()) {
            tracing::error!(path = %path.display(), "the threads' stacks were saved");
        }
    }
}
