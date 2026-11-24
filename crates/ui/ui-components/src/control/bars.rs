//! Scrollbars and separators as controls: a thumb held and dragged, a track pressed, the
//! wheel over a scrolled area, a divider moved between panels.

use std::time::Instant;

use kernel::{Point, Rect};

use crate::ViewerCursor;

use super::{Control, NOWHERE, Press};

/// A press on an edge scrollbar: its thumb held, the offset it gives handed to `set` (the
/// parts below differ only in where the offset goes).
fn bar_press(bar: &mut crate::scrollbar::EdgeScrollbar, p: Point, set: impl FnOnce(f32)) -> Press {
    match bar.press(p) {
        Some(offset) => {
            set(offset);
            Press::Held
        }
        None => Press::Ignored,
    }
}

/// `offset` moved to `to`: whether it changed.
fn move_to(offset: &mut f32, to: f32) -> bool {
    let changed = to != *offset;
    *offset = to;
    changed
}

/// A scrollbar and the offset it moves, as one control: pressed on its lane, its thumb is
/// held (or the offset jumps there) and follows the pointer.
#[derive(Debug)]
pub struct Thumb<'a> {
    pub bar: &'a mut crate::scrollbar::EdgeScrollbar,
    pub offset: &'a mut f32,
}

impl Control for Thumb<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        bar_press(self.bar, p, |offset| *self.offset = offset)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.bar.drag(p).is_some_and(|offset| move_to(self.offset, offset))
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.bar.next_frame()
    }
}

/// A scrollbar and the [`crate::ScrollState`] it moves, as one control (as [`Thumb`] for
/// a list whose scroll keeps its own bounds).
#[derive(Debug)]
pub struct ScrollThumb<'a> {
    pub bar: &'a mut crate::scrollbar::EdgeScrollbar,
    pub scroll: &'a mut crate::ScrollState,
}

impl Control for ScrollThumb<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        bar_press(self.bar, p, |offset| {
            self.scroll.scroll_to(offset);
        })
    }

    fn drag(&mut self, p: Point) -> bool {
        self.bar.drag(p).is_some_and(|offset| self.scroll.scroll_to(offset))
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.bar.next_frame()
    }
}

/// A scrolled area with its bar: the bar's thumb held (or the offset jumps where its track
/// is pressed), the wheel anywhere over `area`, the offset kept within `0..=max`.
#[derive(Debug)]
pub struct Scrolled<'a> {
    pub bar: &'a mut crate::scrollbar::EdgeScrollbar,
    pub offset: &'a mut f32,
    pub area: Rect,
    pub max: f32,
}

impl Control for Scrolled<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p) || self.area.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let max = self.max.max(0.0);
        bar_press(self.bar, p, |offset| *self.offset = offset.clamp(0.0, max))
    }

    fn drag(&mut self, p: Point) -> bool {
        let max = self.max.max(0.0);
        self.bar.drag(p).is_some_and(|offset| move_to(self.offset, offset.clamp(0.0, max)))
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let to = (*self.offset + dy).clamp(0.0, self.max.max(0.0));
        move_to(self.offset, to)
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.bar.next_frame()
    }
}

/// A scrollbar drawn with its own metrics (across a code block, along a column) and the
/// offset it moves, as one control.
#[derive(Debug)]
pub struct BarThumb<'a> {
    pub bar: &'a mut crate::scrollbar::Scrollbar,
    pub metrics: crate::scrollbar::ScrollMetrics,
    pub offset: &'a mut f32,
}

impl BarThumb<'_> {
    /// `offset` kept within what can be scrolled.
    fn within(&self, offset: f32) -> f32 {
        let m = self.metrics;
        offset.clamp(0.0, (m.content - m.viewport).max(0.0))
    }
}

impl Control for BarThumb<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        match self.bar.begin_drag(p, self.metrics) {
            Some(offset) => {
                *self.offset = self.within(offset);
                Press::Held
            }
            None => Press::Ignored,
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag_to(p, self.metrics) else { return false };
        let offset = self.within(offset);
        move_to(self.offset, offset)
    }

    fn release(&mut self) -> bool {
        self.bar.end_drag()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }
}

/// A separator between panels as a control: pressed on it, it's held and the pointer moves
/// it; where its line's center goes is written to `at` (the owner turns it into a size).
#[derive(Debug)]
pub struct Divider<'a> {
    pub splitter: &'a mut crate::splitter::Splitter,
    pub at: &'a mut Option<f32>,
}

impl Control for Divider<'_> {
    fn hit(&self, p: Point) -> bool {
        self.splitter.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        if self.splitter.begin_drag(p) { Press::Held } else { Press::Ignored }
    }

    fn drag(&mut self, p: Point) -> bool {
        let to = self.splitter.drag_to(p);
        let changed = to.is_some() && to != *self.at;
        *self.at = to;
        changed
    }

    fn release(&mut self) -> bool {
        self.splitter.end_drag()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.splitter.hover(p.unwrap_or(NOWHERE))
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(match self.splitter.orientation() {
            crate::splitter::Orientation::Vertical => ViewerCursor::ResizeHorizontal,
            crate::splitter::Orientation::Horizontal => ViewerCursor::ResizeVertical,
        })
    }

    fn animating(&self) -> bool {
        self.splitter.is_animating()
    }
}
