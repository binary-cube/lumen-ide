//! Scrolling the history by pixels, not whole lines: the wheel's / trackpad's deltas add up
//! to a position (lines from the bottom, with a fraction); the emulator keeps the whole part
//! (its display offset, in lines), the view draws the grid shifted down by the fraction of a
//! line, the line above the screen in the gap. At the bottom it's exactly there (following
//! the output: nothing shifted), at the history's top too.

/// The part of a line the screen is scrolled beyond the emulator's display offset.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SmoothScroll {
    /// 0..1 of a line, upward (toward the history).
    frac: f32,
}

impl SmoothScroll {
    /// The wheel moved by `dy` physical pixels (positive: toward the bottom, as the platform
    /// says), each line `line` pixels tall, the screen `offset` lines back in a `history`
    /// that long: the whole lines the emulator scrolls by (positive: back into the history).
    pub fn wheel(&mut self, dy: f32, line: f32, offset: usize, history: usize) -> i32 {
        let line = line.max(1.0);
        let at = offset as f32 + self.frac - dy / line;
        let at = at.clamp(0.0, history as f32);
        let whole = at.floor();
        self.frac = at - whole;
        // At the history's top there's no line above to show: exactly there.
        if whole as usize >= history || self.frac < 1e-3 {
            self.frac = 0.0;
        } else if self.frac > 1.0 - 1e-3 {
            self.frac = 0.0;
            return whole as i32 + 1 - offset as i32;
        }
        whole as i32 - offset as i32
    }

    /// Back at a whole line (the emulator moved by itself: the output followed, the history
    /// cleared, a key typed).
    pub fn reset(&mut self) {
        self.frac = 0.0;
    }

    /// How far down the grid is drawn, in pixels, for lines `line` tall (0: not shifted).
    pub fn shift(&self, line: f32) -> f32 {
        (self.frac * line).round()
    }

    /// Scrolled by part of a line (the line above the screen is drawn too).
    pub fn partial(&self) -> bool {
        self.frac > 0.0
    }

    /// The state as the emulator is now: at its bottom (offset 0, nothing to scroll back) or
    /// with no history above, nothing partial remains.
    pub fn settle(&mut self, offset: usize, history: usize) {
        if history == 0 || offset >= history {
            self.frac = 0.0;
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/scroll.rs"]
mod tests;
