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
}

impl Tooltips {
    /// The tooltip of the item under the mouse (or none). `true` if a redraw is needed.
    pub fn set(&mut self, tip: Option<Tip>) -> bool {
        let Some(tip) = tip else {
            let changed = self.shown.is_some();
            self.pending = None;
            self.shown = None;
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
            self.shown = Some((tip, appear));
        }
        self.drawn.clear();
        let Some((tip, appear)) = &self.shown else { return };
        let ctx = Ctx::new(palette, scale).with_opacity(appear.value());
        let layer = scene.push_layer(None);
        let pill =
            ds::tooltip(layer, text, &ctx, tip.anchor, &tip.label, tip.shortcut.as_deref(), bounds);
        // Its arrow: between the pill and the element, on the element's middle.
        let (arrow, x) = (ctx.px(7.0), tip.anchor.min_x() + tip.anchor.size.width / 2.0);
        let (top, bottom) = if pill.min_y() >= tip.anchor.max_y() {
            (tip.anchor.max_y(), pill.min_y())
        } else {
            (pill.max_y(), tip.anchor.min_y())
        };
        self.drawn.push((pill, ctx.px(ds::radius::SM + 1.0)));
        if bottom > top {
            self.drawn.push((Rect::new(x - arrow, top, 2.0 * arrow, bottom - top), 0.0));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/tooltip.rs"]
mod tests;
