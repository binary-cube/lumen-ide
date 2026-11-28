//! The timer: a thread of its own (`sleep-watch`) that looks at every view that can sleep once
//! a minute (idle times are minutes long) while some lives and some kind's sleeping is on.
//! For each: an order never taken let go; why it stays awake written to the log when that
//! changes; one left alone long enough ordered to sleep, its window woken (its view takes the
//! order at its poll). Nothing else: it knows no view, and holds no lock while it logs or
//! wakes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ui_components::sleep::{self as views, Slot};

/// How often it looks.
const LOOK_EVERY: Duration = Duration::from_secs(60);

/// The thread runs.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether there's work: some view lives and some kind's sleeping is on.
fn needed() -> bool {
    views::any() && views::any_on()
}

/// The thread started if it's needed and not running (a view registered, a kind turned on).
pub(crate) fn ensure_running() {
    if !needed() || RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    // A thread of its own: it waits between looks for as long as it's needed (a pool's would
    // be held all that while), and ends when it isn't.
    #[allow(clippy::disallowed_methods)]
    let spawned = std::thread::Builder::new().name("sleep-watch".into()).spawn(run);
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::Release);
        tracing::warn!(error = %e, "can't start the sleeping's thread");
    }
}

/// The loop: while it's needed; started again when it is.
fn run() {
    loop {
        std::thread::sleep(LOOK_EVERY);
        if !needed() {
            RUNNING.store(false, Ordering::Release);
            // Needed again meanwhile: go on (else a new thread is started by it).
            if !needed() || RUNNING.swap(true, Ordering::AcqRel) {
                return;
            }
        }
        look(Instant::now());
    }
}

/// One look at every view (see the module).
pub(crate) fn look(now: Instant) {
    for slot in views::slots() {
        slot.drop_stale_order(now);
        let (kind, facts) = {
            let state = slot.lock();
            (state.kind, state.facts())
        };
        let Some(idle) = views::idle_for(kind.id) else {
            tell_awake(&slot, Some("its kind's sleeping is off"));
            continue;
        };
        let awake = sleep::awake_because(&facts, idle, now);
        tell_awake(&slot, awake);
        if awake.is_none() {
            slot.sleep_idle(idle);
        }
    }
}

/// Why the view in `slot` stays awake (`None`: it may sleep), written to the log only when that
/// changed since it last was (the lock let go first).
fn tell_awake(slot: &Slot, awake: Option<&'static str>) {
    let told = {
        let mut state = slot.lock();
        if state.told_awake == Some(awake) {
            return;
        }
        state.told_awake = Some(awake);
        (state.kind.id, state.key.clone())
    };
    let (kind, key) = told;
    tracing::debug!(kind, key, awake = awake.unwrap_or("no: may sleep"), "a view's sleep");
}

#[cfg(test)]
#[path = "../tests/unit/timer.rs"]
mod tests;
