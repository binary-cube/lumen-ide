//! How long each part of a window's first frames took, for the log: a slow frame says which
//! part it was. Stops after those frames: nothing is measured afterwards.

use std::time::Instant;

/// The frames measured from a window's start.
const FRAMES: u32 = 30;
/// A part shorter than this isn't worth a place in the log (milliseconds).
const WORTH: u128 = 5;

#[derive(Debug, Default)]
pub(crate) struct PaintPhases {
    marks: Vec<(&'static str, Instant)>,
    frames: u32,
}

impl PaintPhases {
    /// A frame starts.
    pub(crate) fn begin(&mut self) {
        self.marks.clear();
        if self.frames < FRAMES {
            self.frames += 1;
            self.marks.push(("start", Instant::now()));
        }
    }

    /// The part `name` ended now.
    pub(crate) fn mark(&mut self, name: &'static str) {
        if !self.marks.is_empty() {
            self.marks.push((name, Instant::now()));
        }
    }

    /// The parts of this frame that took a while (`editor=1840ms …`), slowest first.
    pub(crate) fn text(&self) -> String {
        let mut parts: Vec<(&str, u128)> = self
            .marks
            .windows(2)
            .map(|pair| (pair[1].0, pair[1].1.duration_since(pair[0].1).as_millis()))
            .filter(|(_, ms)| *ms >= WORTH)
            .collect();
        parts.sort_by_key(|part| std::cmp::Reverse(part.1));
        parts.iter().map(|(name, ms)| format!("{name}={ms}ms")).collect::<Vec<_>>().join(" ")
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/paint_phases.rs"]
mod tests;
