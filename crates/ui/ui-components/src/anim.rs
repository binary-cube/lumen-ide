//! Short eased transitions (hover, appearances): a value that moves smoothly toward a target.
//! Users request frames only while [`Tween::animating`] is true.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
}

impl Tween {
    pub fn new(value: f32, duration: Duration) -> Self {
        Self { from: value, to: value, start: Instant::now(), duration }
    }

    /// The current value (ease-out cubic).
    pub fn value(&self) -> f32 {
        let t = (self.start.elapsed().as_secs_f32() / self.duration.as_secs_f32().max(1e-3))
            .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * eased
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    /// Starts toward `to` from the current value (no jump, even mid-transition).
    /// `true` if the target changed.
    pub fn retarget(&mut self, to: f32) -> bool {
        if (self.to - to).abs() < f32::EPSILON {
            return false;
        }
        self.from = self.value();
        self.to = to;
        self.start = Instant::now();
        true
    }

    /// Jumps straight to `value` (no transition).
    pub fn set(&mut self, value: f32) {
        self.from = value;
        self.to = value;
    }

    pub fn animating(&self) -> bool {
        (self.from - self.to).abs() > f32::EPSILON && self.start.elapsed() < self.duration
    }
}

/// A window's appearance (a modal, a menu): from 0 to 1 over a short time once it opens
/// (1 before it ever opened). Each window picks its own duration.
#[derive(Debug, Clone, Copy, Default)]
pub struct Appear(Option<Tween>);

impl Appear {
    /// Starts appearing (opening): from 0 toward 1 over `duration`.
    pub fn start(&mut self, duration: std::time::Duration) {
        let mut tween = Tween::new(0.0, duration);
        tween.retarget(1.0);
        self.0 = Some(tween);
    }

    /// How far it appeared (0..=1).
    pub fn value(&self) -> f32 {
        self.0.map_or(1.0, |t| t.value())
    }

    pub fn animating(&self) -> bool {
        self.0.is_some_and(|t| t.animating())
    }
}

#[cfg(test)]
#[path = "../tests/unit/anim.rs"]
mod tests;
