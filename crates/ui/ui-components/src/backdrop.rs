//! The backdrop under every window over the editor (modals, dialogs, About, the project
//! search, the menu drawer): one style, resolved once from the theme (and the user's own
//! choice over it) and kept in the [`Palette`](crate::palette::Palette); [`crate::ds`]
//! draws it ([`crate::ds::backdrop`], [`crate::ds::backdrop_faded`]).
//!
//! `Simple` dims the window behind with a color; `Glass` blurs it a little (the renderer's
//! backdrop blur) and lays the color over it.

use kernel::Rgba;
pub use theming::backdrop::{
    BackdropKind, BackdropSpec, DEFAULT_BLUR, MAX_BLUR, MAX_OPACITY, MIN_PANEL_OPACITY,
    PANEL_OPACITY, SurfaceSpec,
};

/// Which window a backdrop is under: each has its own strength when neither the theme nor
/// the user sets one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropRole {
    /// A modal, a dialog, About, the project search: a light dimming.
    Dialog,
    /// The menu drawer: the window behind clearly set back (more under a dark theme).
    Drawer,
}

/// The backdrop's style, resolved: what it is, its color and opacity, how much it blurs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackdropStyle {
    pub kind: BackdropKind,
    /// The color (opaque).
    pub color: Rgba,
    /// How much of the color (`None`: each role's own, see [`Self::opacity`]).
    pub opacity: Option<f32>,
    /// How far the glass blurs, logical pixels (0: not at all).
    pub blur: f32,
    /// What the theme says (kept to resolve the user's choice over it again).
    theme: BackdropSpec,
    /// The window background's brightness (the drawer's own opacity follows it).
    brightness: f32,
}

/// A dialog's dimming when nothing else is said: black at 16% (as it always was).
const DIALOG_OPACITY: f32 = 0.16;

impl BackdropStyle {
    /// The style a theme asks for (`spec`), its window background `window`.
    pub fn resolve(spec: BackdropSpec, window: Rgba) -> Self {
        let mut style = Self {
            kind: BackdropKind::Simple,
            color: Rgba::BLACK,
            opacity: None,
            blur: DEFAULT_BLUR,
            theme: spec,
            brightness: window.luminance(),
        };
        style.apply(spec);
        style
    }

    /// The theme's style with the user's own choice over it (what `user` doesn't say stays
    /// the theme's).
    pub fn with_user(&self, user: BackdropSpec) -> Self {
        let style = Self::resolve(user.over(self.theme), Rgba::BLACK);
        Self { theme: self.theme, brightness: self.brightness, ..style }
    }

    /// The same, never blurred (a renderer that can't blur).
    pub fn without_blur(self) -> Self {
        Self { kind: BackdropKind::Simple, ..self }
    }

    /// What the theme says of it.
    pub fn theme_spec(&self) -> BackdropSpec {
        self.theme
    }

    fn apply(&mut self, spec: BackdropSpec) {
        self.kind = spec.style.unwrap_or(self.kind);
        self.color = spec.color.map_or(self.color, |c| c.with_alpha(1.0));
        self.opacity = spec.opacity.map(|o| o.clamp(0.0, MAX_OPACITY)).or(self.opacity);
        self.blur = spec.blur.map_or(self.blur, |b| b.clamp(0.0, MAX_BLUR));
    }

    /// How much of the color goes over the window behind under `role`.
    pub fn opacity(&self, role: BackdropRole) -> f32 {
        self.opacity.unwrap_or(match role {
            BackdropRole::Dialog => DIALOG_OPACITY,
            // About half black under a dark theme, a quarter under a light one, in between
            // by the window's brightness — the drawer clearly in front.
            BackdropRole::Drawer => (0.55 - 0.3 * self.brightness).clamp(0.25, 0.55),
        })
    }

    /// The color laid over the window behind under `role` (with its opacity).
    pub fn tint(&self, role: BackdropRole) -> Rgba {
        self.color.with_alpha(self.opacity(role))
    }

    /// The blur's reach in physical pixels at `scale` (`None`: not blurred).
    pub fn blur_radius(&self, scale: f32) -> Option<f32> {
        let radius = (self.blur * scale).round();
        (self.kind == BackdropKind::Glass && radius >= 1.0).then_some(radius)
    }
}

impl Default for BackdropStyle {
    fn default() -> Self {
        Self::resolve(BackdropSpec::default(), Rgba::BLACK)
    }
}

/// A floating panel's glass, resolved (the menu drawer's; any floating panel's later): Simple
/// — opaque, as always —, or Glass — its color at `opacity` over a blurred copy of what's
/// under it, a faint lighter edge on top. From the theme (`[floating]`), the user's choice
/// over it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceStyle {
    pub kind: BackdropKind,
    /// Its tint (opaque; none: the panel's own color).
    pub tint: Option<Rgba>,
    /// The panel's opacity when Glass ([`MIN_PANEL_OPACITY`]–1).
    pub opacity: f32,
    /// How far the glass blurs, logical pixels.
    pub blur: f32,
    /// What the theme says (kept to resolve the user's choice over it again).
    theme: SurfaceSpec,
}

impl SurfaceStyle {
    /// The style a theme asks for.
    pub fn resolve(spec: SurfaceSpec) -> Self {
        Self {
            kind: spec.style.unwrap_or_default(),
            tint: spec.color.map(|c| c.with_alpha(1.0)),
            opacity: spec.opacity.unwrap_or(PANEL_OPACITY).clamp(MIN_PANEL_OPACITY, 1.0),
            blur: spec.blur.unwrap_or(DEFAULT_BLUR).clamp(0.0, MAX_BLUR),
            theme: spec,
        }
    }

    /// The theme's style with the user's choice over it (what `user` doesn't say stays the
    /// theme's).
    pub fn with_user(&self, user: SurfaceSpec) -> Self {
        Self { theme: self.theme, ..Self::resolve(user.over(self.theme)) }
    }

    /// The same, never blurred (a renderer that can't blur): opaque.
    pub fn without_blur(self) -> Self {
        Self { kind: BackdropKind::Simple, ..self }
    }

    /// What the theme says of it.
    pub fn theme_spec(&self) -> SurfaceSpec {
        self.theme
    }

    pub fn is_glass(&self) -> bool {
        self.kind == BackdropKind::Glass
    }

    /// The panel's fill: `base` (its own color) or the tint, translucent when Glass.
    pub fn fill(&self, base: Rgba) -> Rgba {
        let color = self.tint.unwrap_or(base);
        if self.is_glass() { color.with_alpha(color.a.min(1.0) * self.opacity) } else { color }
    }

    /// The blur's reach in physical pixels at `scale` (`None`: not blurred).
    pub fn blur_radius(&self, scale: f32) -> Option<f32> {
        let radius = (self.blur * scale).round();
        (self.is_glass() && radius >= 1.0).then_some(radius)
    }
}

impl Default for SurfaceStyle {
    fn default() -> Self {
        Self::resolve(SurfaceSpec::default())
    }
}

#[cfg(test)]
#[path = "../tests/unit/backdrop.rs"]
mod tests;
