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

/// How an [`Attention`] draws the eye: its fade in and out, its pulses (how many, how long
/// each) and how low a pulse dips (0..=1 of the whole).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttentionLook {
    pub fade: Duration,
    pub pulses: u32,
    pub period: Duration,
    pub low: f32,
}

impl Default for AttentionLook {
    /// A quick fade, two gentle pulses (a little over a second), then still.
    fn default() -> Self {
        Self {
            fade: Duration::from_millis(220),
            pulses: 2,
            period: Duration::from_millis(650),
            low: 0.4,
        }
    }
}

/// Something drawing the eye for a moment (a tab a hint is about): it fades in, pulses a few
/// times, then holds still — no more frames while it stays; let go, it fades out and is
/// gone. Its value (0..=1) is how strongly it shows; times are given, so it's tested
/// without waiting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attention {
    look: AttentionLook,
    started: Instant,
    /// Let go: when, and how strongly it showed then (it fades from there).
    released: Option<(Instant, f32)>,
}

impl Attention {
    /// Starting now.
    pub fn new(look: AttentionLook, now: Instant) -> Self {
        Self { look, started: now, released: None }
    }

    /// How strongly it shows at `now` (0..=1).
    pub fn value(&self, now: Instant) -> f32 {
        let fade = self.look.fade.as_secs_f32().max(1e-3);
        if let Some((at, from)) = self.released {
            let t = (now.saturating_duration_since(at).as_secs_f32() / fade).clamp(0.0, 1.0);
            return from * (1.0 - ease_out(t));
        }
        let t = now.saturating_duration_since(self.started).as_secs_f32();
        if t < fade {
            return ease_out(t / fade);
        }
        let period = self.look.period.as_secs_f32().max(1e-3);
        let pulsing = t - fade;
        if pulsing >= period * self.look.pulses as f32 {
            return 1.0;
        }
        // Each pulse dips to `low` halfway and comes back, smoothly (a cosine).
        let phase = (pulsing / period).fract();
        let dip = 0.5 - 0.5 * (std::f32::consts::TAU * phase).cos();
        1.0 - (1.0 - self.look.low.clamp(0.0, 1.0)) * dip
    }

    /// Let go at `now`: it fades out from where it is (once; later calls change nothing).
    pub fn release(&mut self, now: Instant) {
        if self.released.is_none() {
            self.released = Some((now, self.value(now)));
        }
    }

    pub fn released(&self) -> bool {
        self.released.is_some()
    }

    /// Faded out after it was let go: nothing left to draw.
    pub fn gone(&self, now: Instant) -> bool {
        self.released.is_some_and(|(at, _)| now.saturating_duration_since(at) >= self.look.fade)
    }

    /// It changes at `now` (frames wanted): fading in, pulsing or fading out — never while it
    /// holds still.
    pub fn animating(&self, now: Instant) -> bool {
        match self.released {
            Some(_) => !self.gone(now),
            None => {
                let moving = self.look.fade + self.look.period * self.look.pulses;
                now.saturating_duration_since(self.started) < moving
            }
        }
    }
}

/// Ease-out cubic of `t` (0..=1).
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
#[path = "../tests/unit/anim.rs"]
mod tests;
