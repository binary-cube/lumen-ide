//! When CEF's own work is due on the main thread: CEF says it (`on_schedule_message_pump_work`,
//! from any thread), the main loop sleeps until then (or until an app asks something), never
//! polling — an engine with nothing to draw wakes nobody.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// How long after something happened the pages' work keeps the quick pace.
pub const ACTIVE_FOR: Duration = Duration::from_secs(2);

/// "Nothing scheduled" in [`Pump::due_at`].
const NONE: u64 = u64::MAX;

/// Microseconds in a millisecond (CEF's delays are milliseconds, the schedule microseconds).
const MICROS_PER_MILLI: u64 = 1000;

/// The main loop's schedule of CEF's work.
#[derive(Clone)]
pub struct Pump {
    shared: Arc<Shared>,
}

struct Shared {
    /// When the work is due, in microseconds since `start` ([`NONE`]: nothing scheduled).
    due_at: AtomicU64,
    /// When something last happened (an app's command, a frame drawn), the same way.
    touched_at: AtomicU64,
    start: Instant,
    /// The main loop's thread: a schedule asked there needs no wake (the loop looks at the
    /// schedule again before it sleeps).
    main: ThreadId,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for Pump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pump").field("until_due", &self.until_due()).finish_non_exhaustive()
    }
}

/// The pump CEF's handler tells (set once, before CEF starts).
static CURRENT: OnceLock<Pump> = OnceLock::new();

impl Pump {
    /// The pump of the main loop running on this thread, the one CEF's handler tells.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let pump = Self::unregistered(wake);
        let _ = CURRENT.set(pump.clone());
        pump
    }

    /// A pump nobody tells but its owner.
    fn unregistered(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Shared {
            due_at: AtomicU64::new(0),
            touched_at: AtomicU64::new(0),
            start: Instant::now(),
            main: std::thread::current().id(),
            wake: Box::new(wake),
        };
        Self { shared: Arc::new(shared) }
    }

    /// CEF's work asked for in `delay_ms` (0 or less: now), from any thread.
    pub fn schedule(delay_ms: i64) {
        let Some(pump) = CURRENT.get() else { return };
        tracing::trace!(delay_ms, "CEF's work scheduled");
        pump.ask(delay_ms);
    }

    /// The work asked for in `delay_ms`: the sooner of what was asked kept (a delay too long
    /// to tell is as good as never). Asked from another thread sooner than the main loop
    /// sleeps for, the loop is woken.
    fn ask(&self, delay_ms: i64) {
        let delay = u64::try_from(delay_ms).unwrap_or(0).saturating_mul(MICROS_PER_MILLI);
        let at = self.micros(Instant::now()).saturating_add(delay).min(NONE - 1);
        let before = self.shared.due_at.fetch_min(at, Ordering::AcqRel);
        if at < before && std::thread::current().id() != self.shared.main {
            (self.shared.wake)();
        }
    }

    /// Something happened on a page (an app's command, a frame drawn): CEF's work runs at
    /// the pages' pace for [`ACTIVE_FOR`] after it.
    pub fn touch() {
        let Some(pump) = CURRENT.get() else { return };
        pump.mark();
    }

    fn mark(&self) {
        let now = self.micros(Instant::now());
        self.shared.touched_at.store(now, Ordering::Release);
    }

    /// Something happened within [`ACTIVE_FOR`].
    pub fn active(&self) -> bool {
        let touched = self.shared.touched_at.load(Ordering::Acquire);
        let now = self.micros(Instant::now());
        now.saturating_sub(touched) < u64::try_from(ACTIVE_FOR.as_micros()).unwrap_or(u64::MAX)
    }

    /// How long until the work is due (zero: now; long: nothing scheduled).
    pub fn until_due(&self) -> Duration {
        match self.shared.due_at.load(Ordering::Acquire) {
            NONE => Duration::MAX,
            at => Duration::from_micros(at.saturating_sub(self.micros(Instant::now()))),
        }
    }

    /// The work is due now: taken (CEF asks again for what's left).
    pub fn take_due(&self) -> bool {
        let now = self.micros(Instant::now());
        let at = self.shared.due_at.load(Ordering::Acquire);
        at <= now
            && self
                .shared
                .due_at
                .compare_exchange(at, NONE, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    fn micros(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.shared.start).as_micros())
            .unwrap_or(NONE - 1)
    }
}

#[cfg(test)]
#[path = "../tests/unit/pump.rs"]
mod tests;
