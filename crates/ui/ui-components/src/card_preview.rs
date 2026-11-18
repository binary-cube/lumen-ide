//! The pictures of a settings page's cards (`workbench_model::SettingsRow::Cards`): a
//! module contributes a [`CardPreview`] at `activate`, named as its rows name it; the
//! settings window asks it to paint each card's picture — one card live (the selected or
//! the hovered one: it's asked again at its pace while the page shows), the others still
//! (made once by the module and kept) — and tells it when nothing of it is shown any more
//! (the page left, the window closed): it lets go of what it made.

use std::fmt::Debug;
use std::time::Duration;

use kernel::Rect;
use render_api::Scene;
use text_layout::TextSystem;

use crate::palette::Palette;

/// Where and how a card's picture is painted.
pub struct CardPaint<'a> {
    /// The picture's area (physical pixels).
    pub bounds: Rect,
    /// What may be drawn on (the settings' content showing it): the layers it pushes are
    /// clipped to this.
    pub clip: Rect,
    pub scale: f32,
    pub scene: &'a mut Scene,
    pub text: &'a mut TextSystem,
    pub palette: &'a Palette,
}

impl Debug for CardPaint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardPaint").field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

/// A module's pictures for its cards.
pub trait CardPreview: Send + Sync + Debug {
    /// The name its rows give (`SettingsRow::Cards::preview`).
    fn id(&self) -> &'static str;
    /// Card `value`'s picture, in layers of its own: `live`, as it is now (the next frame
    /// asked after [`Self::frame_interval`]); else its still frame (made once, kept).
    fn paint(&self, value: i64, live: bool, p: CardPaint<'_>);
    /// The live card's pace.
    fn frame_interval(&self) -> Duration {
        Duration::from_millis(33)
    }
    /// None of its cards shows any more: what it made goes.
    fn release(&self);
}
