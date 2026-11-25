//! A page's frames' bookkeeping, apart from CEF: their numbers, which are handed over while
//! the app is behind (one now and then, the others dropped), and what each tells changed
//! (nothing, so all of it, after one the app missed).

use std::time::{Duration, Instant};

use cef::Rect;
use web_engine_protocol::{Changed, MAX_CHANGED};

/// While a page's app is behind (a frame not copied in time), a frame is handed over at most
/// this often, the others dropped: handing one over may wait for room in the app's queue
/// (macOS: up to 100 ms on the main thread, which every page shares), and a frame copied
/// tells it caught up. The page is painted again once it has.
pub const BEHIND_RETRY: Duration = Duration::from_millis(500);

/// A page's frames as told to its app.
#[derive(Debug)]
pub struct Frames {
    /// The latest frame's number.
    latest: u64,
    /// When a frame was last handed over while its app was behind (`None`: it isn't).
    probed: Option<Instant>,
    /// A frame was dropped while its app was behind: painted again once it catches up.
    dropped: bool,
    /// The next frame of the view (0) and of the popup (1) is told whole: the app's copy
    /// missed the one before it (the first, one not handed over).
    whole_next: [bool; 2],
}

impl Default for Frames {
    fn default() -> Self {
        Self { latest: 0, probed: None, dropped: false, whole_next: [true; 2] }
    }
}

impl Frames {
    /// The next frame's number.
    pub fn next(&mut self) -> u64 {
        self.latest += 1;
        self.latest
    }

    /// Whether a frame of the view or the popup is handed over now (`behind`: its app is):
    /// always while its app keeps up; while it's behind, one every [`BEHIND_RETRY`], the
    /// others dropped (the next of their kind told whole).
    pub fn hands_over(&mut self, popup: bool, behind: bool, now: Instant) -> bool {
        if !behind {
            self.probed = None;
            return true;
        }
        if may_probe(self.probed, now) {
            self.probed = Some(now);
            return true;
        }
        self.dropped = true;
        self.missed(popup);
        false
    }

    /// A frame of the view or the popup not handed over (or its picture let go by the app):
    /// the next one is told whole.
    pub fn missed(&mut self, popup: bool) {
        self.whole_next[usize::from(popup)] = true;
    }

    /// What changed in a frame handed over now (`dirty`: CEF's rectangles, in its pixels of a
    /// `width` × `height` frame), as the app copies it: nothing (all of it) after a frame
    /// missed, or past [`MAX_CHANGED`] rectangles.
    pub fn changed(
        &mut self,
        popup: bool,
        dirty: Option<&[Rect]>,
        (width, height): (u32, u32),
    ) -> Vec<Changed> {
        if std::mem::replace(&mut self.whole_next[usize::from(popup)], false) {
            return Vec::new();
        }
        let dirty = dirty.unwrap_or_default();
        if dirty.len() > MAX_CHANGED {
            return Vec::new();
        }
        dirty.iter().filter_map(|rect| changed_in(rect, width, height)).collect()
    }

    /// Whether a frame was dropped since its app was last seen to catch up.
    pub fn dropped(&self) -> bool {
        self.dropped
    }

    /// Whether its app caught up (`behind`: it isn't yet) since a frame was dropped: the page
    /// is to be painted again (told once).
    pub fn caught_up(&mut self, behind: bool) -> bool {
        if !self.dropped || behind {
            return false;
        }
        self.dropped = false;
        true
    }
}

/// Whether a frame may be handed over while its app is behind: the first, then one each
/// [`BEHIND_RETRY`] (`last`: the one before).
pub fn may_probe(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| now.saturating_duration_since(last) >= BEHIND_RETRY)
}

/// CEF's changed rectangle cut to a `width` × `height` frame (`None`: nothing of it inside).
pub fn changed_in(rect: &Rect, width: u32, height: u32) -> Option<Changed> {
    let side = |at: i32, size: i32, limit: u32| {
        let from = u32::try_from(at.max(0)).ok()?.min(limit);
        let to = u32::try_from(at.saturating_add(size).max(0)).ok()?.min(limit);
        (to > from).then_some((from, to - from))
    };
    let (x, width) = side(rect.x, rect.width, width)?;
    let (y, height) = side(rect.y, rect.height, height)?;
    Some(Changed { x, y, width, height })
}

#[cfg(test)]
#[path = "../../tests/unit/handlers/frames.rs"]
mod tests;
