//! Whether Focus mode is on: the app's, for all its windows (each asks at its frames). Off
//! at start: a mode for a while, not a setting.

#[cfg(not(test))]
static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    // Each test thread its own (they run side by side).
    static ON: std::sync::atomic::AtomicBool = const { std::sync::atomic::AtomicBool::new(false) };
}

fn with<R>(f: impl FnOnce(&std::sync::atomic::AtomicBool) -> R) -> R {
    #[cfg(not(test))]
    return f(&ON);
    #[cfg(test)]
    ON.with(f)
}

pub fn is_on() -> bool {
    with(|on| on.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set(on: bool) {
    with(|o| o.store(on, std::sync::atomic::Ordering::Relaxed));
}

/// On if it was off, off if it was on; returns the new state.
pub fn toggle() -> bool {
    !with(|on| on.fetch_xor(true, std::sync::atomic::Ordering::Relaxed))
}

#[cfg(test)]
#[path = "../tests/unit/state.rs"]
mod tests;
