//! What a module's background work tells the whole app, from any thread, without a window
//! at hand: that what the windows show changed ([`wake`]: they look again at their next
//! turn), something the user must hear ([`post`]: a notification of the app's, not of a
//! project's — an update, the web engine, a setting kept for every window), and whether
//! work runs that closing the app would stop ([`busy`]: asked of every module's reporter,
//! as the closing does). The app sets the waker and the probe once its first window opens;
//! until then (and in tests) they do nothing.
//!
//! What's told the app waits here until the app hands it to a window ([`take_told`]): a new
//! notification to the window the user is in (focused, or the one focused last), one
//! updated in place ([`post_update`]) or closed ([`post_dismiss`]) to the window showing
//! it — it stays where it shows until it's done — else to the one the user is in; one the
//! user asked for ([`post_brought`]: a look for a newer version) is brought to the window
//! the user is in, gone from any other. Each is handed to one window, once; a project's own notifications never come here (they stay on
//! its window: `host.notify`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::{Notification, NotificationId};

type Waker = Arc<dyn Fn() + Send + Sync>;
type Probe = Arc<dyn Fn() -> bool + Send + Sync>;

static WAKER: RwLock<Option<Waker>> = RwLock::new(None);
static PROBE: RwLock<Option<Probe>> = RwLock::new(None);
static TOLD: Mutex<Vec<Told>> = Mutex::new(Vec::new());
/// Something waits in [`TOLD`] (looked at after every event without locking).
static PENDING: AtomicBool = AtomicBool::new(false);

/// What's told the whole app about one of its notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Told {
    /// A notification shown (merged with an identical one shown).
    Show(Notification),
    /// The one with the same id replaced (work in progress that went on or ended); shown
    /// if it's gone.
    Update(Notification),
    /// Notification `id` closed.
    Dismiss(NotificationId),
    /// The one with the same id replaced and brought to the window the user is in (where
    /// they asked for it), closed in any other showing it.
    Bring(Notification),
}

impl Told {
    /// The notification it's about.
    pub fn id(&self) -> NotificationId {
        match self {
            Self::Show(n) | Self::Update(n) | Self::Bring(n) => n.id,
            Self::Dismiss(id) => *id,
        }
    }
}

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

/// `told` kept for the app to hand to a window, which it wakes.
fn tell(told: Told) {
    TOLD.lock().unwrap_or_else(PoisonError::into_inner).push(told);
    PENDING.store(true, Ordering::Release);
    wake();
}

/// A notification for the user, from any thread: shown on the window the user is in.
pub fn post(notification: Notification) {
    tell(Told::Show(notification));
}

/// Notification `notification.id` replaced (shown if it's gone), from any thread: where it
/// shows, else on the window the user is in.
pub fn post_update(notification: Notification) {
    tell(Told::Update(notification));
}

/// Notification `notification.id` replaced and shown on the window the user is in (it asked
/// for it there: the answer must come there), closed in any other window showing it.
pub fn post_brought(notification: Notification) {
    tell(Told::Bring(notification));
}

/// Notification `id` closed, from any thread, wherever it shows.
pub fn post_dismiss(id: NotificationId) {
    tell(Told::Dismiss(id));
}

/// What was told since the last call, in order (the app hands each to a window). A plain
/// load while nothing waits.
pub fn take_told() -> Vec<Told> {
    if !PENDING.swap(false, Ordering::Acquire) {
        return Vec::new();
    }
    std::mem::take(&mut *TOLD.lock().unwrap_or_else(PoisonError::into_inner))
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
