//! When to keep the computer awake ([`Mode`], [`wants_hold`]) and what the screen does while
//! it's kept awake and nobody touches anything ([`ScreenCare`]): dimmed after a while, and a
//! small mark drifting so that nothing stays hours in the same place (burn-in). Pure: the same
//! inputs give the same answer.

use std::time::Duration;

/// When the computer is kept awake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Never (the module's button off).
    #[default]
    Off,
    /// While it's on, whatever runs.
    Always,
    /// While it's on and the app has work running (a terminal's program, transfers…).
    WhileWorking,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::Off, Self::Always, Self::WhileWorking];

    /// Its id, as saved.
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Always => "always",
            Self::WhileWorking => "working",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }
}

/// Whether a hold is wanted in `mode`, `busy` saying whether work runs now.
pub fn wants_hold(mode: Mode, busy: bool) -> bool {
    match mode {
        Mode::Off => false,
        Mode::Always => true,
        Mode::WhileWorking => busy,
    }
}

/// How long the dim takes to reach its level (seconds): gently, not a blink.
pub const DIM_FADE: f32 = 3.0;

/// What the screen does while kept awake and untouched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenCare {
    /// After how long without input it dims (`None`: never).
    pub dim_after: Option<Duration>,
    /// How dark it gets (0: not at all, 1: black).
    pub level: f32,
    /// A small mark drifts across the dimmed screen (nothing stays in one place).
    pub drift: bool,
}

impl ScreenCare {
    /// How dark the screen is `since` the dim began: from nothing to its level in
    /// [`DIM_FADE`] seconds.
    pub fn dim_at(&self, since: Duration) -> f32 {
        let t = (since.as_secs_f32() / DIM_FADE).clamp(0.0, 1.0);
        // Eased: it starts slowly (a glance away doesn't see it jump).
        self.level.clamp(0.0, 1.0) * t * t * (3.0 - 2.0 * t)
    }
}

/// How long a drifting mark takes to cross its area (seconds): slow enough to rest the eye,
/// fast enough that no pixel stays lit for long.
pub const DRIFT_PERIOD: f32 = 180.0;

/// Where a mark `item` big (width, height) is at `t` seconds within `area` (width, height):
/// its top left corner, moving smoothly over the whole area (two slow waves of different
/// periods, so its path doesn't repeat for a long time), always wholly inside.
pub fn drift_position(t: f32, area: (f32, f32), item: (f32, f32)) -> (f32, f32) {
    let room = ((area.0 - item.0).max(0.0), (area.1 - item.1).max(0.0));
    let tau = std::f32::consts::TAU;
    let u = 0.5 + 0.5 * (tau * t / DRIFT_PERIOD).sin();
    let v = 0.5 + 0.5 * (tau * t / (DRIFT_PERIOD * 1.618) + 1.3).sin();
    (room.0 * u, room.1 * v)
}

#[cfg(test)]
#[path = "../tests/unit/policy.rs"]
mod tests;
