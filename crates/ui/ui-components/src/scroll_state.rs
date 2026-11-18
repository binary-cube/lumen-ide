//! A scrolling list's position, as the projects list keeps it: how far it's scrolled, how
//! tall its content was when last painted and how tall its room is. Never past the end
//! (momentum scrolling doesn't draw a frame past it and snap back), drawn at whole pixels
//! (text and boxes scroll in step), the selected row kept in view.

use kernel::Rect;

/// A list's scroll (physical pixels).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollState {
    offset: f32,
    /// The content's height measured at the last paint.
    content: f32,
    /// The room it scrolls in.
    viewport: f32,
}

impl ScrollState {
    /// How far it's scrolled.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// The content's height measured at the last paint.
    pub fn content(&self) -> f32 {
        self.content
    }

    /// Back to the top.
    pub fn reset(&mut self) {
        self.offset = 0.0;
    }

    /// Its room for this paint (the offset kept within the content as last measured).
    pub fn set_viewport(&mut self, viewport: f32) {
        self.viewport = viewport;
        self.offset = self.offset.min(self.max());
    }

    /// Where the content's top is drawn in a room starting at `top`: whole pixels.
    pub fn origin(&self, top: f32) -> f32 {
        top - self.offset.round()
    }

    /// The content drawn ends at `bottom` (from an origin at `top`, [`Self::origin`]): its
    /// height measured, the offset kept within it.
    pub fn measure(&mut self, top: f32, bottom: f32) {
        self.content = bottom - top;
        self.offset = self.offset.min(self.max());
    }

    /// The content is taller than its room.
    pub fn overflows(&self) -> bool {
        self.content > self.viewport + 0.5
    }

    /// How far it scrolls at most.
    pub fn max(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    /// Scrolled by `dy` (the wheel, the trackpad), never past either end. `true` if it moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.offset;
        self.offset = (self.offset + dy).clamp(0.0, self.max());
        self.offset != before
    }

    /// Scrolled to `offset` (a scrollbar's thumb), within the ends.
    pub fn scroll_to(&mut self, offset: f32) -> bool {
        let before = self.offset;
        self.offset = offset.clamp(0.0, self.max());
        self.offset != before
    }

    /// Row `row` (as drawn) brought into `view` (the list's room): scrolled just enough.
    pub fn keep_visible(&mut self, row: Rect, view: Rect) {
        if row.min_y() < view.min_y() {
            self.offset -= view.min_y() - row.min_y();
        } else if row.max_y() > view.max_y() {
            self.offset += row.max_y() - view.max_y();
        }
        self.offset = self.offset.max(0.0);
    }
}

#[cfg(test)]
#[path = "../tests/unit/scroll_state.rs"]
mod tests;
