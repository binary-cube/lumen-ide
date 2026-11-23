//! A chart's transitions (see [`super::ChartMotion`]): its first appearance, its move to new
//! data (bars and points from their old places, the old masks fading into the new), and its
//! lit element fading in under the mouse. Each a moment it began and an eased progress read
//! while drawing; nothing runs between frames, and frames are asked only while one moves.

use std::sync::Arc;
use std::time::{Duration, Instant};

use charts_model::geometry::{Mark, MarkShape};
use kernel::{Point, Rect};

use super::look::{ChartMotion, Enter, Update};
use super::raster::Piece;

/// How long the lit element takes to light up.
const HOVER: Duration = Duration::from_millis(120);

/// What a transition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Enter,
    Update,
}

/// A chart's motion under way.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    /// The transition under way, and when it began.
    pub transition: Option<(Instant, Kind)>,
    /// The marks before new data (moved from), the masks before it (faded out), and when
    /// the new masks came (they fade in from then).
    pub before: Vec<Mark>,
    /// `before`'s places sorted by (series, index): a mark's earlier self found by a
    /// binary search (a scatter's thousands of points each frame of a transition).
    pub before_order: Vec<u32>,
    pub old_pieces: Option<Arc<Vec<Piece>>>,
    pub pieces_since: Option<Instant>,
    /// When the element under the mouse was lit.
    pub lit_since: Option<Instant>,
    /// The motion asked last (the look's, "reduce motion" applied).
    pub asked: ChartMotion,
}

/// Eased progress (ease-out cubic) of `since` over `length`.
fn eased(since: Instant, length: Duration) -> f32 {
    let t = (since.elapsed().as_secs_f32() / length.as_secs_f32().max(1e-3)).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

impl Motion {
    fn length(&self) -> Duration {
        Duration::from_millis(u64::from(self.asked.millis))
    }

    /// New data came: `first` (nothing shown before) or after other data (its marks and
    /// masks kept to move from).
    pub fn data(&mut self, first: bool, marks: &[Mark], pieces: Option<Arc<Vec<Piece>>>) {
        let now = Instant::now();
        if first {
            if self.asked.enter != Enter::None {
                self.transition = Some((now, Kind::Enter));
            }
            self.before.clear();
            self.before_order.clear();
            self.old_pieces = None;
        } else if self.asked.update != Update::None {
            self.transition = Some((now, Kind::Update));
            self.before.clear();
            self.before.extend_from_slice(marks);
            self.before_order.clear();
            self.before_order.extend(0..self.before.len() as u32);
            let before = &self.before;
            self.before_order.sort_unstable_by_key(|i| {
                let m = &before[*i as usize];
                (m.series, m.index)
            });
            self.old_pieces = pieces;
        }
        self.pieces_since = None;
    }

    /// New masks came (they fade in from now).
    pub fn pieces_came(&mut self) {
        if self.transition.is_some() {
            self.pieces_since = Some(Instant::now());
        }
    }

    /// The element under the mouse changed (lit again from nothing).
    pub fn lit(&mut self) {
        self.lit_since = self.asked.hover.then(Instant::now);
    }

    /// How lit the element under the mouse is (0–1).
    pub fn lit_amount(&self) -> f32 {
        self.lit_since.map_or(1.0, |s| eased(s, HOVER))
    }

    /// The marks' progress (0–1), and what's moving.
    pub fn marks(&self) -> (f32, Option<Kind>) {
        match self.transition {
            Some((since, kind)) => (eased(since, self.length()), Some(kind)),
            None => (1.0, None),
        }
    }

    /// The new masks' opacity, and the old ones' (fading out).
    pub fn masks(&self) -> (f32, f32) {
        match (self.transition, self.pieces_since) {
            (None, _) => (1.0, 0.0),
            // Not come yet: the old ones stand (they're still the ones drawn).
            (Some(_), None) => (1.0, 0.0),
            (Some((_, Kind::Enter)), Some(since)) => (eased(since, self.length()), 0.0),
            (Some((_, Kind::Update)), Some(since)) => {
                let t = eased(since, self.length());
                (t, 1.0 - t)
            }
        }
    }

    /// Something moves (a frame is wanted).
    pub fn moving(&self) -> bool {
        let length = self.length();
        let running = |since: Instant| since.elapsed() < length;
        self.transition.is_some_and(|(since, _)| running(since))
            || self.pieces_since.is_some_and(running)
            || self.lit_since.is_some_and(|s| s.elapsed() < HOVER)
    }

    /// A transition over (its marks and its masks): what it kept let go.
    pub fn settle(&mut self) {
        let length = self.length();
        // Its marks moved, and its masks faded in (or none were to come: bars only).
        if self.transition.is_some_and(|(s, _)| s.elapsed() >= length)
            && self.pieces_since.is_none_or(|s| s.elapsed() >= length)
        {
            self.transition = None;
            self.pieces_since = None;
            self.before = Vec::new();
            self.before_order = Vec::new();
            self.old_pieces = None;
        }
    }

    /// Mark `m` as drawn now (a bar grown or moved, a point grown or moved), and its
    /// opacity; `base`: where bars grow from.
    pub fn mark(&self, m: &Mark, base: f32) -> (MarkShape, f32) {
        let (t, kind) = self.marks();
        let grow = |shape: MarkShape| match shape {
            MarkShape::Bar(r) => {
                let (top, bottom) = (lerp(base, r.min_y(), t), lerp(base, r.max_y(), t));
                MarkShape::Bar(Rect::new(
                    r.min_x(),
                    top.min(bottom),
                    r.size.width,
                    (bottom - top).abs(),
                ))
            }
            MarkShape::Point { center, radius } => MarkShape::Point { center, radius: radius * t },
            slice => slice,
        };
        match kind {
            None => (m.shape, 1.0),
            Some(Kind::Enter) => match self.asked.enter {
                Enter::Grow => (grow(m.shape), 1.0),
                Enter::Fade => (m.shape, t),
                Enter::None => (m.shape, 1.0),
            },
            Some(Kind::Update) => match self.asked.update {
                Update::Morph => {
                    let from = self.earlier(m);
                    match (from.map(|b| b.shape), m.shape) {
                        (Some(MarkShape::Bar(a)), MarkShape::Bar(b)) => {
                            (MarkShape::Bar(lerp_rect(a, b, t)), 1.0)
                        }
                        (
                            Some(MarkShape::Point { center: a, radius: ra }),
                            MarkShape::Point { center: b, radius: rb },
                        ) => {
                            let center = Point::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t));
                            (MarkShape::Point { center, radius: lerp(ra, rb, t) }, 1.0)
                        }
                        // New: it grows.
                        _ => (grow(m.shape), 1.0),
                    }
                }
                Update::Fade => (m.shape, t),
                Update::None => (m.shape, 1.0),
            },
        }
    }

    /// The mark `m` was before the new data (the same series and index), if it was there.
    fn earlier(&self, m: &Mark) -> Option<&Mark> {
        let key = (m.series, m.index);
        let found = self.before_order.binary_search_by_key(&key, |i| {
            let b = &self.before[*i as usize];
            (b.series, b.index)
        });
        found.ok().and_then(|at| self.before.get(*self.before_order.get(at)? as usize))
    }

    /// The marks gone with new data, fading out (an update's fade).
    pub fn leaving(&self) -> impl Iterator<Item = (&Mark, f32)> {
        let (t, kind) = self.marks();
        let fading = kind == Some(Kind::Update) && self.asked.update == Update::Fade;
        self.before.iter().filter(move |_| fading).map(move |m| (m, 1.0 - t))
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::new(
        lerp(a.min_x(), b.min_x(), t),
        lerp(a.min_y(), b.min_y(), t),
        lerp(a.size.width, b.size.width, t),
        lerp(a.size.height, b.size.height, t),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/charts/motion.rs"]
mod tests;
