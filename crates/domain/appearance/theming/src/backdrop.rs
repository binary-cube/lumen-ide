//! What's under a window over the editor (a modal, a dialog, the menu drawer): the window
//! behind dimmed by a color (`Simple`), or seen through frosted glass — blurred, the color
//! over it (`Glass`). A theme says what it wants (`[backdrop]`), the user may choose their
//! own over every theme; what neither says keeps today's look.

use kernel::Rgba;

/// How the window behind is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackdropKind {
    /// Dimmed by a color.
    #[default]
    Simple,
    /// Blurred, the color over it (where the renderer can blur; elsewhere `Simple`).
    Glass,
}

/// How far the glass blurs by default, in logical pixels: gently (the window behind stays
/// recognizable, only softened).
pub const DEFAULT_BLUR: f32 = 4.0;

/// The farthest it may blur, in logical pixels (0: not at all).
pub const MAX_BLUR: f32 = 12.0;

/// What a theme says of its backdrop (each part optional: the rest from the theme it
/// extends, then the defaults — simple, black, the overlay's own opacity,
/// [`DEFAULT_BLUR`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BackdropSpec {
    pub style: Option<BackdropKind>,
    /// The color (its alpha ignored: [`Self::opacity`] says how much of it).
    pub color: Option<Rgba>,
    /// How much of the color (0–1); none: each overlay's own (a dialog's light dimming, the
    /// menu drawer's stronger one).
    pub opacity: Option<f32>,
    /// How far the glass blurs, logical pixels (0–[`MAX_BLUR`]; 0: not at all).
    pub blur: Option<f32>,
}

impl BackdropSpec {
    /// `self` over `under` (what `self` doesn't say, `under` does).
    pub fn over(self, under: Self) -> Self {
        Self {
            style: self.style.or(under.style),
            color: self.color.or(under.color),
            opacity: self.opacity.or(under.opacity),
            blur: self.blur.or(under.blur),
        }
    }

    /// Nothing said.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The highest opacity a backdrop may have (the window behind must stay visible).
pub const MAX_OPACITY: f32 = 0.85;

/// A floating panel's glass (`[floating]`: the menu drawer now; any panel over the editor
/// later): the same parts as a backdrop's — `style`, its tint (`color`; none: the panel's
/// own color), `opacity` (the panel's, [`MIN_PANEL_OPACITY`]–1) and `blur` — read the same
/// way, inherited the same way. Glass: the panel slightly translucent over a blurred copy of
/// what's under it; Simple: opaque.
pub type SurfaceSpec = BackdropSpec;

/// A glass panel's opacity when nothing says: the code behind only a soft color.
pub const PANEL_OPACITY: f32 = 0.9;

/// The least opacity a glass panel may have (its text must stay perfectly readable).
pub const MIN_PANEL_OPACITY: f32 = 0.6;

impl BackdropKind {
    /// As written in a theme / the settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Glass => "glass",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "simple" | "dim" => Self::Simple,
            "glass" | "blur" => Self::Glass,
            _ => return None,
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/backdrop.rs"]
mod tests;
