//! How busy a window is drawing, told from its frames' times alone: frames close together
//! (typing, scrolling, an animation) make it busy; a caret's blink alone doesn't.

use std::time::{Duration, Instant};

/// Frames closer than this are work going on (a caret blinks every ~500 ms).
const BUSY_GAP: Duration = Duration::from_millis(250);
/// Quiet after this long without such frames.
const QUIET_AFTER: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct Activity {
    last_frame: Option<Instant>,
    /// The last two frames were close: the next close one is work going on (two alone can
    /// be a blink and a timer meeting).
    close: bool,
    last_busy: Option<Instant>,
}

impl Activity {
    /// A frame is being drawn.
    pub fn frame(&mut self, now: Instant) {
        let first = self.last_frame.is_none();
        let close = self.last_frame.is_some_and(|last| now.duration_since(last) < BUSY_GAP);
        if first || (close && self.close) {
            self.last_busy = Some(now);
        }
        self.close = close;
        self.last_frame = Some(now);
    }

    pub fn quiet(&self, now: Instant) -> bool {
        self.quiet_at().is_some_and(|at| now >= at)
    }

    /// When it turns quiet if nothing more is drawn.
    pub fn quiet_at(&self) -> Option<Instant> {
        self.last_busy.map(|busy| busy + QUIET_AFTER)
    }
}

#[cfg(test)]
#[path = "../tests/unit/activity.rs"]
mod tests;
