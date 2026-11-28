//! What draw.io's page holds that the file doesn't, kept here so a page made anew (a theme
//! changed, a tab hidden long enough, a sleep) comes back as it was:
//!
//! - [`ViewState`]: the page shown, its zoom and position, what's selected — told by our page
//!   (`host.js`) as it changes (`hostState`), given back after the next `load`
//!   (`hostRestore`); and whether something is being typed into a cell or a dialog is open
//!   (then it isn't put to sleep);
//! - [`History`]: what the diagram was before each change, so undo goes on past a page made
//!   anew (draw.io's own history goes with its page): asked by our page only once draw.io's
//!   own has nothing left (`hostUndo`, `hostRedo`).
//!
//! Everything the page says is checked and bounded (it's another program's): numbers finite
//! and in range, at most so many selected cells, ids short.

use std::collections::VecDeque;
use std::sync::Arc;

use serde_json::{Map, Value, json};

/// The most cells kept selected, and the longest id kept.
const MOST_SELECTED: usize = 1_000;
const LONGEST_ID: usize = 64;
/// The most pages a file is taken to have, and the farthest a position may be (pixels).
const MOST_PAGES: u64 = 10_000;
const FARTHEST: f64 = 1e7;
/// The zoom's bounds (draw.io's own are narrower).
const ZOOM: (f64, f64) = (0.01, 64.0);
/// The most earlier states kept, and their bytes at most: the oldest let go first.
const MOST_PAST: usize = 100;
const PAST_BYTES: usize = 16 << 20;

/// Where the user was in the diagram, and whether they're in the middle of something.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    /// The page shown (its place among the file's pages).
    pub page: u64,
    pub scale: f64,
    /// The graph's translation, and the container's scroll.
    pub translate: (f64, f64),
    pub scroll: (f64, f64),
    /// The selected cells' ids.
    pub selection: Vec<String>,
    /// Text being typed into a cell, or a dialog open (what's there isn't in the file yet).
    pub busy: bool,
}

impl ViewState {
    /// A `hostState` message's fields, checked (`None`: not a usable one).
    pub fn read(message: &Map<String, Value>) -> Option<Self> {
        let number = |key: &str| message.get(key).and_then(Value::as_f64).filter(|v| v.is_finite());
        let far = |key: &str| number(key).filter(|v| v.abs() <= FARTHEST);
        let page = message.get("page").and_then(Value::as_u64).filter(|p| *p < MOST_PAGES)?;
        let scale = number("scale").filter(|s| (ZOOM.0..=ZOOM.1).contains(s))?;
        let selection = message
            .get("selection")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .filter(|id| !id.is_empty() && id.len() <= LONGEST_ID)
                    .take(MOST_SELECTED)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            page,
            scale,
            translate: (far("tx")?, far("ty")?),
            scroll: (far("left")?, far("top")?),
            selection,
            busy: message.get("busy").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// As given back to the page.
    pub fn to_json(&self) -> Value {
        json!({
            "page": self.page,
            "scale": self.scale,
            "tx": self.translate.0,
            "ty": self.translate.1,
            "left": self.scroll.0,
            "top": self.scroll.1,
            "selection": self.selection,
        })
    }
}

/// What the diagram was before each change (newest last), and what an undo here took back
/// (to redo). Only the states from before the page was last made are offered: draw.io's own
/// history covers the changes since.
#[derive(Debug, Default)]
pub struct History {
    past: VecDeque<Arc<str>>,
    future: Vec<Arc<str>>,
    /// How many of `past` came before the page was last made.
    before_page: usize,
    /// Their bytes, all of `past`'s.
    bytes: usize,
}

impl History {
    /// The diagram changed from `was` (by the user, in the page): kept, what was undone here
    /// let go (a change after an undo starts another branch).
    pub fn changed(&mut self, was: Arc<str>) {
        self.future.clear();
        self.bytes = self.bytes.saturating_add(was.len());
        self.past.push_back(was);
        while self.past.len() > MOST_PAST || (self.bytes > PAST_BYTES && self.past.len() > 1) {
            let Some(old) = self.past.pop_front() else { break };
            self.bytes = self.bytes.saturating_sub(old.len());
            self.before_page = self.before_page.saturating_sub(1);
        }
    }

    /// The page made anew: what came before it is what an undo here may take back.
    pub fn page_made(&mut self) {
        self.before_page = self.past.len();
    }

    /// Whether there's something to undo here, and to redo.
    pub fn can(&self) -> (bool, bool) {
        (self.before_page > 0, !self.future.is_empty())
    }

    /// One step back from `now`: the state before the page was made (the changes since are
    /// draw.io's own, already undone when it asks), `now` kept to redo.
    pub fn undo(&mut self, now: Arc<str>) -> Option<Arc<str>> {
        if self.before_page == 0 {
            return None;
        }
        while self.past.len() > self.before_page {
            self.drop_newest();
        }
        let back = self.drop_newest()?;
        self.before_page = self.past.len();
        self.future.push(now);
        Some(back)
    }

    /// One step forward again from `now` (after an undo here), `now` kept to undo.
    pub fn redo(&mut self, now: Arc<str>) -> Option<Arc<str>> {
        let next = self.future.pop()?;
        self.bytes = self.bytes.saturating_add(now.len());
        self.past.push_back(now);
        self.before_page = self.past.len();
        Some(next)
    }

    fn drop_newest(&mut self) -> Option<Arc<str>> {
        let newest = self.past.pop_back()?;
        self.bytes = self.bytes.saturating_sub(newest.len());
        Some(newest)
    }
}

#[cfg(test)]
#[path = "../tests/unit/state.rs"]
mod tests;
