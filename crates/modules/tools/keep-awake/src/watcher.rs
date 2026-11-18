//! Follows the mode the settings say: always while on (the hold taken at once), off (let
//! go), or only while work runs — then a thread of its own (`keep-awake`) asks every few
//! seconds whether work runs that quitting would stop ([`workbench_model::app_wide::busy`]:
//! a program in a terminal, a transfer, a query) and takes or lets go of the hold; each
//! change is told (a notification) and shown (the windows look again: the button, the
//! status bar). It runs whether the window is shown or not (a locked screen is when it
//! matters), and ends when the mode changes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use power::{Mode, PowerError, wants_hold};
use workbench_model::{Notification, app_wide};

/// How often it asks whether work runs.
pub const EVERY: Duration = Duration::from_secs(5);
/// Its thread's name (its CPU on the module's row in the Task Manager).
pub const THREAD: &str = "keep-awake";

/// Bumped at each mode followed: a thread of an older one ends.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Held while the mode changes and while a look acts: an older thread's look never undoes
/// what a newer mode did.
static TURN: Mutex<()> = Mutex::new(());

/// Follows `mode`: the hold taken or let go now (always / off), or watched for (while work
/// runs). `Err`: the system wouldn't keep awake (nothing held).
pub fn follow(mode: Mode) -> Result<(), PowerError> {
    let _turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let keeper = crate::keeper::keeper();
    let keep = crate::settings::config().keep();
    match mode {
        Mode::Off => keeper.set(None),
        // Held already: held again if the screen is asked now (or no longer).
        Mode::Always => keeper.set(Some(keep)),
        Mode::WhileWorking => {
            // Not in tests: they check one step at a time (`step`).
            if !cfg!(test) {
                kernel::work::run(THREAD, move || watch(generation, &app_wide::busy));
            }
            Ok(())
        }
    }
}

/// The watching thread: a step every [`EVERY`] while its generation is the current one.
fn watch(generation: u64, busy: &dyn Fn() -> bool) {
    while GENERATION.load(Ordering::Acquire) == generation {
        let busy = busy();
        let _turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
        // The mode changed while it asked: this thread is done.
        if GENERATION.load(Ordering::Acquire) != generation {
            return;
        }
        step(busy);
        drop(_turn);
        std::thread::sleep(EVERY);
    }
}

/// One look: the hold taken while work runs (`busy`), let go once it ends — told and shown.
pub fn step(busy: bool) {
    let keeper = crate::keeper::keeper();
    let held = keeper.is_held();
    let want = wants_hold(Mode::WhileWorking, busy);
    if want == held {
        return;
    }
    match keeper.set(want.then(|| crate::settings::config().keep())) {
        Ok(()) => {
            let key = if want { "keep_awake.started_working" } else { "keep_awake.ended_working" };
            app_wide::post(Notification::info(i18n::t(key)));
        }
        Err(why) => {
            let text = i18n::tr!("keep_awake.failed", why = why.to_string().as_str());
            app_wide::post(Notification::warning(text));
        }
    }
    app_wide::wake();
}

#[cfg(test)]
#[path = "../tests/unit/watcher.rs"]
mod tests;
