//! The "Changed" tab's list as a timeline: a fine line down its left side, a ring on it for
//! each day, the day named beside it (and its date), and under it that day's files, each a
//! small dot on the line, with their times — newest first, some air above each day. What's searched narrows it by the files'
//! paths: every word typed (any case) in the path.
//!
//! What each file needs for it (its day, its time written, its path in lower case) is made
//! once per walk, on the walk's worker ([`Stamp::of`]); the list's items again only when
//! the walk, the span, the search or what's in the span changed.

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use ui_components::Date;

use crate::session::RecentFile;

/// What a file of the list shows besides its path: its day, its time, its path to search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub day: Date,
    /// `14:32`, here.
    pub time: String,
    /// Its path in lower case (what a search looks in).
    pub lower: String,
}

impl Stamp {
    /// `file`'s, in the user's time zone.
    pub fn of(file: &RecentFile) -> Self {
        let (day, time) = crate::local_time::day_and_time(file.modified)
            .unwrap_or((Date { year: 1970, month: 1, day: 1 }, String::new()));
        Self { day, time, lower: file.path.to_lowercase() }
    }
}

/// A row of the list: a day (and how many of its files are shown), or a file (its index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Day { day: Date, count: usize },
    File(usize),
}

/// A file's row and a day's (logical pixels), the air above every day but the first, and
/// the list's margins above its first row and under its last.
pub const FILE_H: f32 = 32.0;
pub const DAY_H: f32 = 36.0;
pub const DAY_GAP: f32 = 14.0;
pub const PAD_TOP: f32 = 8.0;
pub const PAD_BOTTOM: f32 = 18.0;

/// Where each item starts (logical pixels from the list's top, its margin in), and its
/// end: one more than the items.
pub fn tops(items: &[Item]) -> Vec<f32> {
    let mut tops = Vec::with_capacity(items.len() + 1);
    let mut y = PAD_TOP;
    for (n, item) in items.iter().enumerate() {
        tops.push(y);
        y += match item {
            Item::Day { .. } if n > 0 => DAY_GAP + DAY_H,
            Item::Day { .. } => DAY_H,
            Item::File(_) => FILE_H,
        };
    }
    tops.push(y);
    tops
}

/// The item at `y` (logical pixels from the list's top), by `tops`.
pub fn item_at(tops: &[f32], y: f32) -> Option<usize> {
    let n = tops.len().checked_sub(1)?;
    if y < *tops.first()? || y >= tops[n] {
        return None;
    }
    Some(tops.partition_point(|&t| t <= y).saturating_sub(1).min(n - 1))
}

/// The day pinned at the list's top while scrolled by `scroll` (logical pixels): its item,
/// and how far the next day's heading pushes it up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pinned {
    pub day: usize,
    pub pushed: f32,
}

/// The day whose files pass under the list's top once its own heading has gone above it
/// (`None`: no heading has yet), pushed up by the next one's coming within its height.
pub fn pinned(items: &[Item], tops: &[f32], scroll: f32) -> Option<Pinned> {
    // A heading's own row: under its air.
    let heading = |n: usize| tops[n + 1] - DAY_H;
    let days = || (0..items.len()).filter(|&n| matches!(items[n], Item::Day { .. }));
    let day = days().take_while(|&n| heading(n) < scroll).last()?;
    let next = days().find(|&n| n > day).map(heading);
    let pushed = next.map_or(0.0, |y| (scroll + DAY_H - y).max(0.0));
    Some(Pinned { day, pushed })
}

/// The words of a search, in lower case (none: everything matches).
pub fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// A path (in lower case) has every word.
pub fn matches(lower: &str, words: &[String]) -> bool {
    words.iter().all(|w| lower.contains(w.as_str()))
}

/// The files `shown` (newest first) under their days: each day once, before its files.
pub fn items(shown: &[usize], stamps: &[Stamp]) -> Vec<Item> {
    let mut items = Vec::with_capacity(shown.len() + 8);
    let mut day_at = None;
    for &i in shown {
        let Some(stamp) = stamps.get(i) else { continue };
        let same =
            day_at.is_some_and(|at| matches!(items[at], Item::Day { day, .. } if day == stamp.day));
        if !same {
            day_at = Some(items.len());
            items.push(Item::Day { day: stamp.day, count: 0 });
        }
        if let Some(Item::Day { count, .. }) = day_at.and_then(|at| items.get_mut(at)) {
            *count += 1;
        }
        items.push(Item::File(i));
    }
    items
}

/// The line from `from` down to `to` at `x`.
pub fn line(layer: &mut Layer, x: f32, (from, to): (f32, f32), w: f32, color: Rgba) {
    if to > from {
        layer.quad(Quad::filled(Rect::new(x - w / 2.0, from, w, to - from), color));
    }
}

/// A day's dot on the line at `at`, `r` across its half: filled, or a ring (today).
pub fn dot(layer: &mut Layer, at: Point, r: f32, ring: Option<(f32, Rgba)>, color: Rgba) {
    let disc = |r: f32| Rect::new(at.x - r, at.y - r, r * 2.0, r * 2.0);
    layer.quad(Quad::filled(disc(r), color).rounded(r));
    if let Some((width, inside)) = ring {
        let inner = (r - width).max(0.0);
        layer.quad(Quad::filled(disc(inner), inside).rounded(inner));
    }
}

#[cfg(test)]
#[path = "../../tests/unit/changed_view/timeline.rs"]
mod tests;
