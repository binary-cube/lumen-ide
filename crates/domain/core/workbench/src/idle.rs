//! Something started after a while without input (Relax mode): a module contributes an
//! [`IdleAction`] — after how long, which command —; the window keeps an [`IdleWatch`] per
//! action (when it last had input) and runs the command once per idle spell, when it's
//! due — only if the window is focused, shown and nothing is open over it then (else that
//! spell is skipped). Input starts a new spell.

use std::time::{Duration, Instant};

use command_model::CommandId;

/// A module's command run after a while without input in the window.
pub trait IdleAction: Send + Sync + std::fmt::Debug {
    /// How long without input before it runs (`None`: not now — the user turned it off).
    /// Asked at each frame: cheap.
    fn after(&self) -> Option<Duration>;
    /// The command it runs.
    fn command(&self) -> CommandId;
}

/// When the window last had input, and whether this spell's action came already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleWatch {
    last_input: Instant,
    came: bool,
}

impl IdleWatch {
    pub fn new(now: Instant) -> Self {
        Self { last_input: now, came: false }
    }

    /// Input at `now`: a new spell begins.
    pub fn input(&mut self, now: Instant) {
        self.last_input = now;
        self.came = false;
    }

    /// The user was last active at `at`, in any program: counts as input if it's newer than
    /// what's known (not the same moment read again: that would start the spell over).
    pub fn active_at(&mut self, at: Instant) {
        if at > self.last_input + std::time::Duration::from_secs(1) {
            self.input(at);
        }
    }

    /// When the action is due, `after` without input (`None`: it came this spell).
    pub fn deadline(&self, after: Duration) -> Option<Instant> {
        (!self.came).then(|| self.last_input + after)
    }

    /// Due at `now`: `true` once a spell (it has come then, whether it ran or not).
    pub fn take_due(&mut self, now: Instant, after: Duration) -> bool {
        let due = self.deadline(after).is_some_and(|at| now >= at);
        if due {
            self.came = true;
        }
        due
    }
}

#[cfg(test)]
#[path = "../tests/unit/idle.rs"]
mod tests;
