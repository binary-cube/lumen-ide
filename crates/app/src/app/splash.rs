//! When the start-up's splash shows: only while the window has nothing complete to show
//! (the active tab's colors not there yet), never for a fixed time. Ready within
//! [`GRACE`], there's no splash at all (the window's background meanwhile); later, it stays
//! at least [`MIN_SHOWN`] (no flash), until ready — or [`MAX_SHOWN`], ready or not — then
//! fades out over the workbench in [`FADE`].

use std::time::{Duration, Instant};

/// Ready this soon after the first frame: no splash at all.
pub(super) const GRACE: Duration = Duration::from_millis(120);
/// Once shown, the splash stays at least this long (it never flashes).
pub(super) const MIN_SHOWN: Duration = Duration::from_millis(250);
/// The splash never stays longer than this, ready or not (a parse that never ends).
pub(super) const MAX_SHOWN: Duration = Duration::from_secs(2);
/// Its fade-out over the workbench.
pub(super) const FADE: Duration = Duration::from_millis(150);

/// What a frame shows at the start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum SplashShow {
    /// The workbench alone (no splash, or it's gone).
    Workbench,
    /// Only the window's background: not ready, too early for the splash.
    Background,
    /// The splash, shown for `elapsed` (its animation's phase); `opacity` 1 alone, less
    /// over the workbench (fading out).
    Splash { elapsed: Duration, opacity: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No frame yet.
    NotStarted,
    /// The first frame came at this moment; not ready yet.
    Waiting(Instant),
    /// Shown since then.
    Shown(Instant),
    /// Shown since `shown`, fading out since `since`.
    Fading {
        shown: Instant,
        since: Instant,
    },
    Gone,
}

/// The splash's clock (see the module's doc).
#[derive(Debug, Clone)]
pub(super) struct SplashClock {
    phase: Phase,
}

impl SplashClock {
    /// `enabled`: `false` never shows it (`EDITOR_NO_SPLASH`, the resize diagnostic).
    pub fn new(enabled: bool) -> Self {
        Self { phase: if enabled { Phase::NotStarted } else { Phase::Gone } }
    }

    /// The splash is over (or never was): frames show the workbench alone.
    pub fn is_gone(&self) -> bool {
        self.phase == Phase::Gone
    }

    /// What the frame at `now` shows, `ready` telling whether the workbench has everything
    /// to show. `Some(shown)` in the second value the moment the splash ends (`shown`:
    /// whether it was ever drawn).
    pub fn frame(&mut self, now: Instant, ready: bool) -> (SplashShow, Option<bool>) {
        loop {
            match self.phase {
                Phase::NotStarted => {
                    if ready {
                        self.phase = Phase::Gone;
                        return (SplashShow::Workbench, Some(false));
                    }
                    self.phase = Phase::Waiting(now);
                }
                Phase::Waiting(first) => {
                    if ready {
                        self.phase = Phase::Gone;
                        return (SplashShow::Workbench, Some(false));
                    }
                    if now.saturating_duration_since(first) < GRACE {
                        return (SplashShow::Background, None);
                    }
                    self.phase = Phase::Shown(now);
                }
                Phase::Shown(since) => {
                    let elapsed = now.saturating_duration_since(since);
                    if (ready && elapsed >= MIN_SHOWN) || elapsed >= MAX_SHOWN {
                        self.phase = Phase::Fading { shown: since, since: now };
                        continue;
                    }
                    return (SplashShow::Splash { elapsed, opacity: 1.0 }, None);
                }
                Phase::Fading { shown, since } => {
                    let fading = now.saturating_duration_since(since);
                    if fading >= FADE {
                        self.phase = Phase::Gone;
                        return (SplashShow::Workbench, Some(true));
                    }
                    let opacity = 1.0 - fading.as_secs_f32() / FADE.as_secs_f32();
                    let elapsed = now.saturating_duration_since(shown);
                    return (SplashShow::Splash { elapsed, opacity: opacity.max(0.0) }, None);
                }
                Phase::Gone => return (SplashShow::Workbench, None),
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/splash.rs"]
mod tests;
