//! What modules start after a while without input in the window (Relax mode's
//! auto-start): each [`workbench_model::IdleAction`] has its watch here; input starts a new
//! spell for all; a due one runs its command once a spell — only in the frontmost window
//! (the one focused last: focused or not now), shown, with nothing open over it (else that
//! spell is skipped). So of several windows, one starts it at most.

use std::time::Instant;

use super::*;

impl Workbench {
    /// Input came (a key, the mouse): every watch starts again.
    pub fn idle_input(&mut self, now: Instant) {
        for watch in &mut self.idle {
            watch.input(now);
        }
    }

    /// The user was last active at `at`, in any program (the wait is for the computer left
    /// alone, not for this window).
    pub fn active_elsewhere(&mut self, at: Instant) {
        for watch in &mut self.idle {
            watch.active_at(at);
        }
    }

    /// When the next one is due (a frame then), if any is on.
    pub fn idle_deadline(&self) -> Option<Instant> {
        self.support
            .idle_actions
            .iter()
            .zip(&self.idle)
            .filter_map(|(action, watch)| watch.deadline(action.after()?))
            .min()
    }

    /// Runs the ones due at `now`, if the window may (the `frontmost`, shown, nothing over
    /// it). `true` if one ran.
    pub fn run_idle_actions(&mut self, now: Instant, frontmost: bool) -> bool {
        let may = frontmost && !self.occluded && !self.overlay_open();
        let support = Arc::clone(&self.support);
        let mut ran = false;
        for (action, i) in support.idle_actions.iter().zip(0..) {
            let Some(after) = action.after() else { continue };
            if self.idle[i].take_due(now, after) && may {
                tracing::debug!(command = action.command().0, "started after a while idle");
                ran |= self.run_command(action.command()).unwrap_or(false);
            }
        }
        ran
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/idle.rs"]
mod tests;
