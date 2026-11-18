//! What a module's background work tells the whole app, from any thread, without a window
//! at hand: that what the windows show changed ([`wake`]: they look again at their next
//! turn), something the user must hear ([`post`]: a notification, shown by the window that
//! looks first), and whether work runs that closing the app would stop ([`busy`]: asked of
//! every module's reporter, as the closing does). The app sets the waker and the probe once
//! its first window opens; until then (and in tests) they do nothing.

use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::Notification;

type Waker = Arc<dyn Fn() + Send + Sync>;
type Probe = Arc<dyn Fn() -> bool + Send + Sync>;

static WAKER: RwLock<Option<Waker>> = RwLock::new(None);
static PROBE: RwLock<Option<Probe>> = RwLock::new(None);
static POSTED: Mutex<Vec<Notification>> = Mutex::new(Vec::new());

/// The app's waker (its event loop's).
pub fn set_waker(waker: Waker) {
    *WAKER.write().unwrap_or_else(PoisonError::into_inner) = Some(waker);
}

/// The windows look again (their indicators, their buttons) at their next turn.
pub fn wake() {
    let waker = WAKER.read().unwrap_or_else(PoisonError::into_inner).clone();
    if let Some(wake) = waker {
        wake();
    }
}

/// A notification for the user, from any thread: shown by the window that looks first.
pub fn post(notification: Notification) {
    POSTED.lock().unwrap_or_else(PoisonError::into_inner).push(notification);
    wake();
}

/// The notifications posted since the last call (a window shows them).
pub fn take_posted() -> Vec<Notification> {
    std::mem::take(&mut *POSTED.lock().unwrap_or_else(PoisonError::into_inner))
}

/// How the app says whether work runs (its modules' reporters).
pub fn set_busy_probe(probe: Probe) {
    *PROBE.write().unwrap_or_else(PoisonError::into_inner) = Some(probe);
}

/// Work runs that closing the app would stop (a program in a terminal, a transfer, a
/// query): what the closing would ask about. `false` before the app said how to know.
pub fn busy() -> bool {
    let probe = PROBE.read().unwrap_or_else(PoisonError::into_inner).clone();
    probe.is_some_and(|probe| probe())
}

#[cfg(test)]
#[path = "../tests/unit/app_wide.rs"]
mod tests;
