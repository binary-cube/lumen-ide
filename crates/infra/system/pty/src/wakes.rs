//! How often a session's output wakes the UI: at most once per [`INTERVAL`] (about a
//! 120 Hz frame). Each wake runs the whole app's poll pass, and a program streaming output
//! (a build's log, `yes`, a hidden tab's `tail -f`) would otherwise wake it as fast as the
//! poll takes the screen's change, with no limit.
//!
//! The first output after a quiet interval wakes at once (a typed key's echo isn't
//! delayed); output within the interval of the last wake is told by a single wake at the
//! interval's end, from a timer thread of the session's own (started on the first such
//! wake, asleep between, gone with the session): the last output is always shown.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use crate::Waker;

/// The shortest time between two wakes of one session.
pub(crate) const INTERVAL: Duration = Duration::from_millis(8);

/// What a wake asked comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wake {
    /// Quiet long enough: woken now.
    Now,
    /// Too soon: woken once at that time.
    At(Instant),
    /// A later wake is already on its way: it tells this one too.
    Coming,
}

/// When the wakes asked go (pure: the time given).
#[derive(Debug, Default)]
pub(crate) struct Pace {
    last: Option<Instant>,
    coming: bool,
}

impl Pace {
    /// A wake asked at `now`.
    pub(crate) fn ask(&mut self, now: Instant) -> Wake {
        if self.coming {
            return Wake::Coming;
        }
        match self.last.map(|at| at + INTERVAL) {
            Some(next) if now < next => {
                self.coming = true;
                Wake::At(next)
            }
            _ => {
                self.last = Some(now);
                Wake::Now
            }
        }
    }

    /// The wake put off went, at `now`.
    pub(crate) fn fired(&mut self, now: Instant) {
        self.coming = false;
        self.last = Some(now);
    }
}

/// A session's wakes of the UI, paced.
pub(crate) struct Wakes {
    waker: Waker,
    pace: Arc<Mutex<Pace>>,
    /// The timer thread's channel (started when a wake is first put off).
    timer: OnceLock<Sender<Instant>>,
}

impl Wakes {
    pub(crate) fn new(waker: Waker) -> Self {
        Self { waker, pace: Arc::default(), timer: OnceLock::new() }
    }

    /// Wakes the UI now, or once at the interval's end. The pace is only read and written
    /// under its lock: the wake happens after it's let go.
    pub(crate) fn wake(&self) {
        let wake = self.pace.lock().unwrap_or_else(PoisonError::into_inner).ask(Instant::now());
        match wake {
            Wake::Now => (self.waker)(),
            Wake::At(at) => {
                let timer =
                    self.timer.get_or_init(|| timer(Arc::clone(&self.pace), self.waker.clone()));
                if timer.send(at).is_err() {
                    // No thread to wait in: woken now rather than never.
                    self.pace.lock().unwrap_or_else(PoisonError::into_inner).fired(Instant::now());
                    (self.waker)();
                }
            }
            Wake::Coming => {}
        }
    }
}

/// The thread waking the UI at the times it's sent (one at a time: a wake is put off only
/// while none is coming); it ends with the session (its sender dropped).
fn timer(pace: Arc<Mutex<Pace>>, waker: Waker) -> Sender<Instant> {
    let (tx, rx) = channel::<Instant>();
    let spawned =
        std::thread::Builder::new().name("pty-wake".into()).spawn(move || run(&rx, &pace, &waker));
    if let Err(e) = spawned {
        tracing::warn!("can't start the terminal's wake thread: {e}");
    }
    tx
}

fn run(rx: &Receiver<Instant>, pace: &Mutex<Pace>, waker: &Waker) {
    while let Ok(at) = rx.recv() {
        let wait = at.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
        pace.lock().unwrap_or_else(PoisonError::into_inner).fired(Instant::now());
        waker();
    }
}

#[cfg(test)]
#[path = "../tests/unit/wakes.rs"]
mod tests;
