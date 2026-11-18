//! Double Shift (press and release Shift twice, nothing else in between) opens the
//! project search, like "Search Everywhere".

use std::time::{Duration, Instant};

use platform_api::Modifiers;

/// The longest pause between the two presses.
const WINDOW: Duration = Duration::from_millis(400);

#[derive(Debug, Default)]
pub(super) struct DoubleShift {
    /// Shift alone is held down now.
    down: bool,
    /// When the previous lone Shift was released.
    released: Option<Instant>,
}

impl DoubleShift {
    /// The modifiers changed. `true` when this completes a double Shift.
    pub fn modifiers(&mut self, m: Modifiers, now: Instant) -> bool {
        let shift_only = m.shift && !m.ctrl && !m.alt && !m.meta;
        let none = !m.shift && !m.ctrl && !m.alt && !m.meta;
        if shift_only && !self.down {
            self.down = true;
            return false;
        }
        if none && self.down {
            self.down = false;
            if self.released.is_some_and(|t| now.duration_since(t) <= WINDOW) {
                self.released = None;
                return true;
            }
            self.released = Some(now);
            return false;
        }
        if !shift_only {
            // Another modifier joined (e.g. ⇧⌘): not a double Shift.
            self.cancel();
        }
        false
    }

    /// A real key or a click happened: the sequence is broken.
    pub fn cancel(&mut self) {
        self.down = false;
        self.released = None;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/double_shift.rs"]
mod tests;
