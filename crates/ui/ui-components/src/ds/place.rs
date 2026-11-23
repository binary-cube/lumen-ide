//! Where a popover goes by what it's anchored to: under it when it fits, else over it (or
//! always over it), from the anchor's left or right edge, kept inside the window.

use kernel::Rect;

/// The side a popover takes by its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverSide {
    /// Under it when it fits (with `bottom` kept free), else over it.
    Below,
    /// Always over it (a status bar's, a footer's).
    Above,
}

/// How a popover is placed (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopoverPlace {
    pub side: PopoverSide,
    /// Its right edge on the anchor's (else its left edge on the anchor's).
    pub right: bool,
    /// Between it and its anchor ([`super::POPOVER_GAP`] as a rule).
    pub gap: f32,
    /// Kept free from the window's sides and top.
    pub margin: f32,
    /// Kept free over the window's bottom when it goes under (the bottom bar's room).
    pub bottom: f32,
}

/// The rectangle of a popover of `size` placed by `anchor` in `view`; whether it went under.
/// It never leaves the window's left or top edge, however small the window.
pub fn popover_rect(
    anchor: Rect,
    (w, h): (f32, f32),
    view: Rect,
    place: PopoverPlace,
) -> (Rect, bool) {
    let from = if place.right { anchor.max_x() - w } else { anchor.min_x() };
    let x = from.min(view.max_x() - w - place.margin).max(view.min_x() + place.margin);
    let below = anchor.max_y() + place.gap;
    let under = place.side == PopoverSide::Below && below + h <= view.max_y() - place.bottom;
    let y = if under {
        below
    } else {
        (anchor.min_y() - place.gap - h).max(view.min_y() + place.margin)
    };
    (Rect::new(x, y, w, h), under)
}

#[cfg(test)]
#[path = "../../tests/unit/ds/place.rs"]
mod tests;
