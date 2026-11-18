//! A splitter dividing a space in two by a share kept in the project's sizes: dragged (never
//! past what each side needs), double-clicked back to where it starts. A layout that doesn't
//! use it hides it (nothing drawn, nothing to take: not where the last layout had it).

use kernel::{Point, Rect};
use render_api::Scene;
use ui_components::{Orientation, Splitter, SplitterStyle, ViewerCursor};
use workbench_model::LayoutSizes;

#[derive(Debug)]
pub(crate) struct Divider {
    splitter: Splitter,
    /// Where the share is kept in the project's sizes.
    key: &'static str,
    default: f32,
    /// The share of the space (without the strip) before it.
    ratio: f32,
    /// What each side keeps at least (logical pixels).
    min_before: f32,
    min_after: f32,
    /// Without room for both, the side after it keeps what it needs.
    after_first: bool,
    /// From the last layout: where the space starts, its length without the strip, the strip's
    /// thickness (physical pixels) and the scale.
    start: f32,
    length: f32,
    strip: f32,
    scale: f32,
    /// Placed by the last layout ([`Self::split`]); [`Self::hide`]: not used by it.
    shown: bool,
}

impl Divider {
    pub(crate) fn new(
        orientation: Orientation,
        key: &'static str,
        default: f32,
        (min_before, min_after): (f32, f32),
    ) -> Self {
        Self {
            splitter: Splitter::new(SplitterStyle::default(), orientation),
            key,
            default,
            ratio: default,
            min_before,
            min_after,
            after_first: false,
            start: 0.0,
            length: 0.0,
            strip: 0.0,
            scale: 1.0,
            shown: false,
        }
    }

    /// Without room for both, the side after it keeps what it needs (fixed parts: a form).
    pub(crate) fn after_first(mut self) -> Self {
        self.after_first = true;
        self
    }

    /// The share saved for the project, if there's a sensible one.
    pub(crate) fn take_sizes(&mut self, sizes: &LayoutSizes) {
        let saved = sizes.get(self.key).filter(|v| (0.0..=1.0).contains(v));
        self.ratio = saved.unwrap_or(self.default);
    }

    fn vertical(&self) -> bool {
        self.splitter.orientation() == Orientation::Vertical
    }

    /// The first side's length, of `length`: its share, each side keeping what it needs.
    fn before(&self, length: f32) -> f32 {
        let (low, high) = (self.min_before * self.scale, length - self.min_after * self.scale);
        let wanted = length * self.ratio;
        // Not room for both: they share it, or what's after it keeps its own.
        (if low <= high {
            wanted.clamp(low, high)
        } else if self.after_first {
            high.max(0.0)
        } else {
            wanted
        })
        .round()
    }

    /// `area` divided by a strip `strip` thick: the two sides.
    pub(crate) fn split(
        &mut self,
        area: Rect,
        strip: f32,
        style: SplitterStyle,
        scale: f32,
    ) -> (Rect, Rect) {
        self.scale = scale;
        self.strip = strip;
        self.shown = true;
        self.splitter.set_style(style);
        let (x, y, width, height) = (area.min_x(), area.min_y(), area.size.width, area.size.height);
        if self.vertical() {
            self.start = x;
            self.length = (width - strip).max(0.0);
            let before = self.before(self.length);
            self.splitter.set_bounds(Rect::new(x + before, y, strip, height), scale);
            let after = x + before + strip;
            (Rect::new(x, y, before, height), Rect::new(after, y, self.length - before, height))
        } else {
            self.start = y;
            self.length = (height - strip).max(0.0);
            let before = self.before(self.length);
            self.splitter.set_bounds(Rect::new(x, y + before, width, strip), scale);
            let after = y + before + strip;
            (Rect::new(x, y, width, before), Rect::new(x, after, width, self.length - before))
        }
    }

    /// Not used by this layout (the message beside the lists, the places not docked): not
    /// drawn, not taken, until it's placed again.
    pub(crate) fn hide(&mut self) {
        self.shown = false;
        self.splitter.end_drag();
    }

    /// A press at `p`: on the strip it starts a drag (two clicks: back to where it starts).
    pub(crate) fn press(&mut self, p: Point, clicks: u32, sizes: Option<&LayoutSizes>) -> bool {
        if !self.shown || !self.splitter.contains(p) {
            return false;
        }
        if clicks >= 2 {
            self.ratio = self.default;
            if let Some(sizes) = sizes {
                sizes.remove(self.key);
            }
        } else {
            self.splitter.begin_drag(p);
        }
        true
    }

    /// The mouse at `p`: `true` while it's dragged.
    pub(crate) fn drag(&mut self, p: Point) -> bool {
        let Some(center) = self.splitter.drag_to(p) else { return false };
        if self.length > 0.0 {
            self.ratio = ((center - self.strip / 2.0 - self.start) / self.length).clamp(0.0, 1.0);
            self.ratio = self.before(self.length) / self.length;
        }
        true
    }

    /// The button released: the share kept. `true` if it was dragged.
    pub(crate) fn release(&mut self, sizes: Option<&LayoutSizes>) -> bool {
        let dragged = self.splitter.end_drag();
        if dragged && let Some(sizes) = sizes {
            sizes.set(self.key, self.ratio);
        }
        dragged
    }

    pub(crate) fn hover(&mut self, p: Point) -> bool {
        self.shown && self.splitter.hover(p)
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.splitter.is_animating()
    }

    /// The cursor it asks for at `p`, over it or while it's dragged.
    pub(crate) fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        let over = self.splitter.is_dragging() || self.splitter.contains(p);
        (self.shown && over).then_some(if self.vertical() {
            ViewerCursor::ResizeHorizontal
        } else {
            ViewerCursor::ResizeVertical
        })
    }

    /// Painted after both sides: its handle reaches over them.
    pub(crate) fn paint(&self, scene: &mut Scene) {
        if self.shown {
            self.splitter.paint(scene);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/divider.rs"]
mod tests;
