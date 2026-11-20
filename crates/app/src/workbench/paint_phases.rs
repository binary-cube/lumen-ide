//! How long each part of a frame took: for the log, a window's first frames (a slow one says
//! which part it was; nothing measured after them); while a profile records, every frame's
//! parts timed under it (`kernel::probe`), so its flame graph says where a frame's time goes.

use std::time::Instant;

/// The frames measured from a window's start.
const FRAMES: u32 = 30;
/// A part shorter than this isn't worth a place in the log (milliseconds).
const WORTH: u128 = 5;

/// A phase's name until the mark that ends it names it.
const PHASE: &str = "paint";

#[derive(Debug, Default)]
pub(crate) struct PaintPhases {
    marks: Vec<(&'static str, Instant)>,
    frames: u32,
    /// While a profile records: the phase under way, timed under the frame (named by the
    /// mark that ends it), so the flame graph says where a frame's time goes.
    phase: Option<kernel::probe::Timer>,
}

impl PaintPhases {
    /// A frame starts.
    pub(crate) fn begin(&mut self) {
        self.marks.clear();
        self.phase = kernel::probe::recording().then(|| kernel::probe::time("app", PHASE));
        if self.frames < FRAMES {
            self.frames += 1;
            self.marks.push(("start", Instant::now()));
        }
    }

    /// The part `name` ended now.
    pub(crate) fn mark(&mut self, name: &'static str) {
        if let Some(mut phase) = self.phase.take() {
            phase.set_what(name);
            drop(phase);
            self.phase = Some(kernel::probe::time("app", PHASE));
        }
        if !self.marks.is_empty() {
            self.marks.push((name, Instant::now()));
        }
    }

    /// The painting is over: what's left of it after the last mark is timed as the rest.
    pub(crate) fn end(&mut self) {
        if let Some(mut phase) = self.phase.take() {
            phase.set_what("rest");
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
