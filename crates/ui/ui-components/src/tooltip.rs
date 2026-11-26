//! The interface tooltips, managed in a single place: components only say which
//! tooltip they have under the mouse ([`Tip`]); the tooltip layer appears after a short hover
//! delay, with a fade, and is drawn above everything. Moving from one button to another,
//! the next tooltip appears immediately (as in operating systems).

use std::time::{Duration, Instant};

use kernel::Rect;
use render_api::Scene;
use text_layout::TextSystem;

use crate::anim::Tween;
use crate::ds::{self, Ctx};
use crate::palette::Palette;

/// The hover delay before appearing.
const DELAY: Duration = Duration::from_millis(450);

/// An item's tooltip: its area, the label and the shortcut (already formatted).
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    pub anchor: Rect,
    pub label: String,
    pub shortcut: Option<String>,
    /// How long the mouse rests before it appears.
    pub delay: Duration,
}

impl Tip {
    pub fn new(anchor: Rect, label: impl Into<String>) -> Self {
        Self { anchor, label: label.into(), shortcut: None, delay: DELAY }
    }

    /// Appears only after a longer rest (e.g. a tab's path: not on every pass).
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn with_shortcut(mut self, shortcut: Option<String>) -> Self {
        self.shortcut = shortcut;
        self
    }
}

#[derive(Debug, Default)]
pub struct Tooltips {
    /// The item under the mouse and since when.
    pending: Option<(Tip, Instant)>,
    /// The displayed tooltip and its appearance.
    shown: Option<(Tip, Tween)>,
    /// Where it was last drawn, as shapes (rect, corners' radius; physical): its pill and its
    /// arrow — what must show over a page of the system's web engine.
    drawn: Vec<(Rect, f32)>,
    /// Where the mouse is (told by whoever follows it): a wide item's tooltip shows by it.
    pointer: Option<kernel::Point>,
    /// Where the shown one points: its item, or (a wide one) the place of it the mouse was
    /// on when it appeared — kept while it shows (it doesn't chase the mouse).
    pointing: Option<Rect>,
    /// Where the mouse was when the shown one appeared (its arrow keeps clear of it there:
    /// the pill doesn't move as the mouse does).
    pointer_at: Option<kernel::Point>,
}

/// An item at least this wide (logical pixels): its tooltip shows by the mouse, not at its
/// middle (a field across the window: where the eyes are).
const WIDE: f32 = 280.0;

impl Tooltips {
    /// The mouse is at `p` (physical): a wide item's tooltip shows by it.
    pub fn pointer(&mut self, p: kernel::Point) {
        self.pointer = Some(p);
    }

    /// Where a tooltip of `anchor` points: `anchor`, or — wider than [`WIDE`] at `scale`,
    /// the mouse over it — a sliver of it at the mouse.
    fn pointing_at(&self, anchor: Rect, scale: f32) -> Rect {
        match self.pointer {
            Some(p)
                if anchor.size.width > WIDE * scale
                    && p.x >= anchor.min_x()
                    && p.x <= anchor.max_x() =>
            {
                Rect::new(p.x - scale, anchor.min_y(), 2.0 * scale, anchor.size.height)
            }
            _ => anchor,
        }
    }

    /// The tooltip of the item under the mouse (or none). `true` if a redraw is needed.
    pub fn set(&mut self, tip: Option<Tip>) -> bool {
        let Some(tip) = tip else {
            let changed = self.shown.is_some();
            self.pending = None;
            self.shown = None;
            self.pointing = None;
            self.drawn.clear();
            return changed;
        };
        if self.shown.as_ref().is_some_and(|(t, _)| *t == tip)
            || self.pending.as_ref().is_some_and(|(t, _)| *t == tip)
        {
            return false;
        }
        if self.shown.is_some() {
            // Already visible: switching to another item is immediate.
            self.pointing = None;
            self.shown = Some((tip, Tween::new(1.0, Duration::from_millis(1))));
            self.pending = None;
            return true;
        }
        self.pending = Some((tip, Instant::now()));
        false
    }

    /// Hides it (click, scroll, key).
    pub fn hide(&mut self) -> bool {
        let changed = self.shown.is_some();
        self.pending = None;
        self.shown = None;
        self.pointing = None;
        self.drawn.clear();
        changed
    }

    /// Where it shows (its pill and its arrow, each with its corners' radius; physical), as
    /// last drawn: nothing while none does.
    pub fn areas(&self) -> &[(Rect, f32)] {
        &self.drawn
    }

    /// Appearing: frames must be requested (the wait itself needs only one, see
    /// [`Self::deadline`]).
    pub fn is_animating(&self) -> bool {
        self.shown.as_ref().is_some_and(|(_, t)| t.animating())
    }

    /// When the pending tooltip appears (a redraw is scheduled for then).
    pub fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|(tip, since)| *since + tip.delay)
    }

    pub fn paint(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
        scale: f32,
        bounds: Rect,
    ) {
        if let Some((tip, since)) = &self.pending
            && since.elapsed() >= tip.delay
            && let Some((tip, _)) = self.pending.take()
        {
            let mut appear = Tween::new(0.0, Duration::from_millis(120));
            appear.retarget(1.0);
            self.pointing = None;
            self.shown = Some((tip, appear));
        }
        self.drawn.clear();
        // Where it points, worked out once as it shows (by the mouse, for a wide item).
        if self.pointing.is_none()
            && let Some((tip, _)) = &self.shown
        {
            self.pointing = Some(self.pointing_at(tip.anchor, scale));
            self.pointer_at = self.pointer;
        }
        let Some((tip, appear)) = &self.shown else { return };
        let anchor = self.pointing.unwrap_or(tip.anchor);
        let ctx = Ctx::new(palette, scale).with_opacity(appear.value());
        let layer = scene.push_layer(None);
        let spec = ds::TooltipSpec {
            anchor,
            label: &tip.label,
            shortcut: tip.shortcut.as_deref(),
            bounds,
            pointer: self.pointer_at,
        };
        let shape = ds::tooltip(layer, text, &ctx, &spec);
        // Its pill and its arrow, where they were drawn (the arrow moved off the cursor too).
        self.drawn.push((shape.pill, ctx.px(ds::radius::SM + 1.0)));
        if let Some(arrow) = shape.arrow {
            self.drawn.push((arrow, 0.0));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/tooltip.rs"]
mod tests;
