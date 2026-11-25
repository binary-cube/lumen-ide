//! The notifications told the whole app (`workbench_model::app_wide`: an update, the web
//! engine, a setting kept for every window), handed to one window each, after every event:
//! one shown somewhere already (updated in place, closed) goes to the window showing it —
//! it stays where it is until it's done, wherever the focus went —, a new one to the
//! window the user is in (focused, or the one focused last: the app in the background, its
//! notification is where the user left it). One the user asked for (`Told::Bring`) goes to
//! the window the user is in, and is closed in the window that showed it before. Each is
//! handed once; with no window yet, they wait for the first.

use workbench_model::NotificationId;
use workbench_model::app_wide::Told;

/// A window as the handing sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    /// When it last took the focus, among the windows (the highest: the one the user is
    /// in, or was in last).
    pub focus_turn: u64,
    /// It shows the notification told about (not leaving).
    pub shows: bool,
}

/// The window (its index in `windows`) that takes `told`; `None`: none (no window, or a
/// notification closed that shows nowhere).
pub fn target(told: &Told, windows: &[Candidate]) -> Option<usize> {
    let front = || windows.iter().enumerate().max_by_key(|(_, w)| w.focus_turn).map(|(i, _)| i);
    if let Told::Bring(_) = told {
        return front();
    }
    if let Some(i) = windows.iter().position(|w| w.shows) {
        return Some(i);
    }
    match told {
        Told::Dismiss(_) => None,
        Told::Show(_) | Told::Update(_) | Told::Bring(_) => front(),
    }
}

/// How the windows are seen and told: each window's look for a notification, and what a
/// window does with what it's handed.
pub struct Handing<L, T> {
    pub look: L,
    pub take: T,
}

/// Each of `told`, in order, handed to the one window of `windows` that [`target`] names
/// (looked at anew for each: one handed may change where the next goes).
pub fn hand<W, L, T>(told: Vec<Told>, windows: &mut [W], mut handing: Handing<L, T>)
where
    L: Fn(&W, NotificationId) -> Candidate,
    T: FnMut(&mut W, Told),
{
    for told in told {
        let id = told.id();
        let seen: Vec<Candidate> = windows.iter().map(|w| (handing.look)(w, id)).collect();
        let taker = target(&told, &seen);
        // Brought to the user: gone from the window that showed it before.
        if let Told::Bring(_) = told {
            for (i, window) in windows.iter_mut().enumerate() {
                if seen[i].shows && Some(i) != taker {
                    (handing.take)(window, Told::Dismiss(id));
                }
            }
        }
        if let Some(window) = taker.and_then(|i| windows.get_mut(i)) {
            (handing.take)(window, told);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/app_notifications.rs"]
mod tests;
