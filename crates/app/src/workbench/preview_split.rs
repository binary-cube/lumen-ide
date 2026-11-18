//! A preview tab's Split mode (the text on the left, its view on the right: a Markdown's
//! preview, a CSV's data): the separator between the halves is a `Splitter` like the app's
//! other ones (the three-dot handle, its hover, the resize cursor). Dragged, the halves
//! follow; a double click gives the halves back their half; neither side gets narrower than
//! [`MIN_WIDTH`] (a tab too narrow for both: half each, the ratio kept for when it widens).
//!
//! The ratio is the tab's (saved with its file in the session, given back when it reopens);
//! the last one the user chose for a viewer is the one its tabs opened from now on start
//! with, in this project ([`workbench_model::sizes::preview_split`]).
//!
//! While dragging, the text relayouts live (cheap); the view keeps the width it was laid out
//! at, clipped to its new half, and is laid out again at most every [`RELAYOUT_EVERY`] and
//! once the button is released — a long Markdown isn't laid out on every pixel.
//!
//! The halves scroll together when the view follows its text (a Markdown preview, its
//! setting on: [`ScrollSync`]): the side the user scrolled drives, the other follows.

use std::time::{Duration, Instant};

use super::*;

/// The narrowest either half gets (logical pixels).
pub(super) const MIN_WIDTH: f32 = 160.0;

/// How often the view is laid out again while the separator is dragged.
pub(super) const RELAYOUT_EVERY: Duration = Duration::from_millis(150);

/// The halves' ratio when never dragged.
pub(super) const DEFAULT_RATIO: f32 = 0.5;

/// How long the side the user scrolled keeps driving: the other's moves meanwhile (its
/// following, a layout settling) aren't taken for the user's.
pub(super) const LEASE: Duration = Duration::from_millis(300);

/// A side of the Split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Text,
    View,
}

/// What a frame's scroll sync asks: the view to the text's top line, the text to the
/// view's, the view to the caret's line (when it's out of it), or nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Follow {
    None,
    View(f32),
    Text(f32),
    Reveal(f32),
}

/// The text and its view scrolled together, both ways, without echoes: each frame compares
/// the tops (in source lines) with those seen after the last one; the side that moved (by
/// the user: the other's follow was seen already) drives for a [`LEASE`], the other follows.
#[derive(Debug, Default)]
pub(super) struct ScrollSync {
    text: Option<f32>,
    view: Option<f32>,
    caret: Option<usize>,
    driver: Option<(Side, Instant)>,
}

impl ScrollSync {
    /// The tops now (the text's line, the view's source line — `None`: it doesn't follow)
    /// and the caret's line: what follows what.
    pub fn step(&mut self, text: f32, view: Option<f32>, caret: usize, now: Instant) -> Follow {
        let Some(view) = view else {
            *self = Self::default();
            return Follow::None;
        };
        let caret_moved = self.caret.is_some_and(|c| c != caret);
        self.caret = Some(caret);
        let (Some(was_text), Some(was_view)) = (self.text, self.view) else {
            // Just shown side by side: the view where the text is.
            self.text = Some(text);
            return Follow::View(text);
        };
        let leased = |side: Side| {
            self.driver
                .is_some_and(|(s, at)| s == side && now.saturating_duration_since(at) < LEASE)
        };
        if (was_text - text).abs() > 0.01 && !leased(Side::View) {
            self.driver = Some((Side::Text, now));
            return Follow::View(text);
        }
        if (was_view - view).abs() > 0.01 && !leased(Side::Text) {
            self.driver = Some((Side::View, now));
            return Follow::Text(view);
        }
        if caret_moved {
            return Follow::Reveal(caret as f32);
        }
        Follow::None
    }

    /// After the frame: the tops both show (a follow isn't taken for a scroll of its own).
    pub fn seen(&mut self, text: f32, view: Option<f32>) {
        self.text = Some(text);
        self.view = view;
    }
}

/// A preview tab's Split: the text's share of the width and the separator.
#[derive(Debug)]
pub(super) struct PreviewSplit {
    /// The share of the width (without the separator) the text has.
    pub ratio: f32,
    pub splitter: Splitter,
    /// The area both halves share (from the last layout).
    pub area: Rect,
    /// While dragging: the width the view is painted at (its last layout's) and when it
    /// was laid out at it.
    live: Option<(f32, Instant)>,
    /// The halves scrolled together.
    pub sync: ScrollSync,
}

impl Default for PreviewSplit {
    fn default() -> Self {
        Self::new(DEFAULT_RATIO)
    }
}

impl PreviewSplit {
    pub fn new(ratio: f32) -> Self {
        Self {
            ratio: sanitize(ratio),
            splitter: Splitter::new(SplitterStyle::default(), Orientation::Vertical),
            area: Rect::default(),
            live: None,
            sync: ScrollSync::default(),
        }
    }

    /// Dragged at least once to something other than half (what the session keeps).
    pub fn saved(&self) -> Option<f32> {
        ((self.ratio - DEFAULT_RATIO).abs() > f32::EPSILON).then_some(self.ratio)
    }

    /// A press at `p` on the separator: dragged from now on (the view keeps its width,
    /// `preview_width`, until laid out again).
    pub fn begin_drag(&mut self, p: Point, preview_width: f32) -> bool {
        if !self.splitter.begin_drag(p) {
            return false;
        }
        self.live = Some((preview_width, Instant::now()));
        true
    }

    pub fn is_dragging(&self) -> bool {
        self.splitter.is_dragging()
    }

    /// The mouse at `p` while dragging: the new ratio. `false` if it isn't dragged.
    pub fn drag(&mut self, p: Point, scale: f32) -> bool {
        let Some(center) = self.splitter.drag_to(p) else { return false };
        self.ratio = ratio_at(self.area, center, scale);
        true
    }

    /// Lays the halves out in `area`: the text's half and the view's.
    pub fn layout(&mut self, area: Rect, scale: f32) -> (Rect, Rect) {
        self.area = area;
        let (text, gap, view) = halves(area, self.ratio, scale);
        self.splitter.set_bounds(gap, scale);
        (text, view)
    }

    /// After a drag step laid the halves out (the view's half `preview_width` wide): the
    /// view laid out again at it when it's time (`now`).
    pub fn relayout_due(&mut self, preview_width: f32, now: Instant) {
        if let Some((width, at)) = &mut self.live
            && now.saturating_duration_since(*at) >= RELAYOUT_EVERY
        {
            *width = preview_width;
            *at = now;
        }
    }

    /// The width the view is painted at while dragging (`None`: its half's).
    pub fn live_width(&self) -> Option<f32> {
        self.live.map(|(w, _)| w)
    }

    /// The button released: `true` if it was dragged.
    pub fn end_drag(&mut self) -> bool {
        self.live = None;
        self.splitter.end_drag()
    }

    /// A double click: half each again.
    pub fn reset(&mut self) {
        self.end_drag();
        self.ratio = DEFAULT_RATIO;
    }
}

/// A ratio read back (the session, the project's sizes): within `0..=1`, half otherwise.
fn sanitize(ratio: f32) -> f32 {
    if ratio.is_finite() { ratio.clamp(0.0, 1.0) } else { DEFAULT_RATIO }
}

/// The separator's thickness (physical pixels), the same as the groups' and docks'.
fn gap(area: Rect, scale: f32) -> f32 {
    (SPLITTER_WIDTH * scale).round().min(area.size.width.max(0.0))
}

/// The text's width for a wish of `left` out of `room` (physical pixels): each half at
/// least [`MIN_WIDTH`], or half each when there's no room for both.
fn clamp_left(left: f32, room: f32, scale: f32) -> f32 {
    let min = (MIN_WIDTH * scale).round().min(room / 2.0);
    left.clamp(min, (room - min).max(min))
}

/// `area` split at `ratio`: the text's half, the separator, the view's half.
pub(super) fn halves(area: Rect, ratio: f32, scale: f32) -> (Rect, Rect, Rect) {
    let gap = gap(area, scale);
    let room = (area.size.width - gap).max(0.0);
    let left = clamp_left((room * sanitize(ratio)).round(), room, scale).round();
    let (x, y, h) = (area.min_x(), area.min_y(), area.size.height);
    (
        Rect::new(x, y, left, h),
        Rect::new(x + left, y, gap, h),
        Rect::new(x + left + gap, y, (room - left).max(0.0), h),
    )
}

/// The ratio putting the separator's center at `center` (x, physical pixels) in `area`.
pub(super) fn ratio_at(area: Rect, center: f32, scale: f32) -> f32 {
    let gap = gap(area, scale);
    let room = (area.size.width - gap).max(0.0);
    if room <= 0.0 {
        return DEFAULT_RATIO;
    }
    clamp_left(center - gap / 2.0 - area.min_x(), room, scale) / room
}

/// The layers a view painted since `first` clipped to `rect` (its half, while it's painted
/// at the width it was last laid out at).
pub(super) fn clip_layers(scene: &mut Scene, first: usize, rect: Rect) {
    for i in first..scene.layers().len() {
        if let Some(layer) = scene.layer_mut(i) {
            let clip = layer.clip.map_or(Some(rect), |c| c.intersect(&rect));
            layer.clip = Some(clip.unwrap_or(Rect::new(rect.min_x(), rect.min_y(), 0.0, 0.0)));
        }
    }
}

impl Workbench {
    /// Before a frame: the active tab's halves scrolled together (the side the user moved
    /// drives); the caret's line brought into the view when it's out of it.
    pub(super) fn sync_preview_scroll(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Some(v) = tab.viewer.as_mut() else { return };
        if !v.splits() {
            v.split.sync = ScrollSync::default();
            return;
        }
        let top = tab.view.top_line(&tab.folds);
        let caret = tab.doc.buffer().line_of(tab.doc.selections().primary().head);
        match v.split.sync.step(top, v.viewer.source_line_at_top(), caret, Instant::now()) {
            Follow::View(line) => {
                v.viewer.scroll_to_source(line);
            }
            Follow::Text(line) => {
                tab.view.scroll_to_line(line, &tab.doc, &tab.folds);
            }
            Follow::Reveal(line) => {
                v.viewer.reveal_source_line(line);
            }
            Follow::None => {}
        }
    }

    /// After a frame: what the halves show, for the next one to tell the user's scrolls.
    pub(super) fn saw_preview_scroll(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Some(v) = tab.viewer.as_mut().filter(|v| v.splits()) else { return };
        v.split.sync.seen(tab.view.top_line(&tab.folds), v.viewer.source_line_at_top());
    }

    /// The active tab's Split separator, when it shows.
    fn preview_split_mut(&mut self) -> Option<&mut TabViewer> {
        self.tabs.get_mut(self.active)?.viewer.as_mut().filter(|v| v.splits())
    }

    /// A press on the active tab's Split separator: dragged (or, double-clicked, half each
    /// again). `None`: not on it.
    pub(super) fn press_preview_split(&mut self, p: Point, clicks: u32) -> Option<MouseResult> {
        let (area, scale) = (self.editor_area, self.scale);
        let v = self.preview_split_mut()?;
        let width = v.bounds.size.width;
        if !v.split.begin_drag(p, width) {
            return None;
        }
        if clicks >= 2 {
            v.split.reset();
            let key = workbench_model::sizes::preview_split(v.provider);
            self.data.sizes.remove(&key);
            self.tabs[self.active].sync_bounds(area, scale);
            return Some(MouseResult::Redraw);
        }
        Some(MouseResult::StartDrag)
    }

    /// The mouse dragged: the active tab's Split separator follows. `None`: it isn't dragged.
    pub(super) fn drag_preview_split(&mut self, p: Point) -> Option<bool> {
        let (area, scale) = (self.editor_area, self.scale);
        let tab = self.tabs.get_mut(self.active)?;
        let v = tab.viewer.as_mut().filter(|v| v.split.is_dragging())?;
        if !v.split.drag(p, scale) {
            return Some(false);
        }
        tab.sync_bounds(area, scale);
        if let Some(v) = tab.viewer.as_mut() {
            let width = v.bounds.size.width;
            v.split.relayout_due(width, Instant::now());
        }
        Some(true)
    }

    /// The button released: the dragged ratio becomes the default of its viewer's tabs.
    pub(super) fn release_preview_split(&mut self) -> bool {
        let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut()) else {
            return false;
        };
        if !v.split.end_drag() {
            return false;
        }
        let key = workbench_model::sizes::preview_split(v.provider);
        let ratio = v.split.ratio;
        self.data.sizes.set(&key, ratio);
        true
    }

    /// The mouse at `p`: the separator's hover; the resize cursor over it.
    pub(super) fn hover_preview_split(&mut self, p: Point) -> (bool, Option<CursorIcon>) {
        let blocked = self.modal_open();
        let Some(v) = self.preview_split_mut() else { return (false, None) };
        let over = !blocked && v.split.splitter.contains(p);
        let redraw = v.split.splitter.hover(if over { p } else { Point::new(f32::MIN, f32::MIN) });
        (redraw, over.then_some(CursorIcon::ResizeHorizontal))
    }

    /// A new preview tab's split: the last ratio the user chose for its viewer here.
    pub(super) fn new_preview_split(&self, provider: &str) -> PreviewSplit {
        let key = workbench_model::sizes::preview_split(provider);
        PreviewSplit::new(self.data.sizes.get(&key).unwrap_or(DEFAULT_RATIO))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/preview_split.rs"]
mod tests;
