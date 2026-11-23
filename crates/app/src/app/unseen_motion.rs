//! A part saying it moves while nothing on screen changes, told in the log: frames asked for
//! an animation that the renderer then skips (they showed nothing new), long enough to be no
//! transition's end. The parts moving then are named (`Workbench::moving_parts`), once until
//! it stops. The app paces such frames meanwhile (`ANIMATION_FLOOR`); this says whose they are.

use std::time::{Duration, Instant};

/// How long frames may move unseen before it's told: longer than any transition (they last
/// under half a second), short enough to be found while it happens.
pub(super) const UNSEEN_FOR: Duration = Duration::from_secs(2);

/// Frames moving unseen, followed.
#[derive(Debug, Default)]
pub(super) struct UnseenMotion {
    /// Since when every frame moved and showed nothing new.
    since: Option<Instant>,
    /// Told already for this run (until a frame shows something, or nothing moves).
    told: bool,
}

impl UnseenMotion {
    /// A frame at `now`: whether it moved (`moving`, something said it moves) and showed
    /// nothing new (`skipped`). When it's been so for [`UNSEEN_FOR`], `name` is asked who
    /// moves and the names are given back once (to be told).
    pub(super) fn frame(
        &mut self,
        now: Instant,
        (moving, skipped): (bool, bool),
        name: impl FnOnce() -> Vec<&'static str>,
    ) -> Option<Vec<&'static str>> {
        if !(moving && skipped) {
            *self = Self::default();
            return None;
        }
        let since = *self.since.get_or_insert(now);
        if self.told || now.duration_since(since) < UNSEEN_FOR {
            return None;
        }
        self.told = true;
        Some(name())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/unseen_motion.rs"]
mod tests;
