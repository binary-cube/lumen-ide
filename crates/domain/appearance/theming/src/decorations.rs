//! A theme's decorations: what it draws around and behind the workbench besides its colors —
//! a list of layers (`[[decorations.layers]]` in its file), drawn in order (the first the
//! furthest back), each of a kind with its own options: a string along the window's edges
//! (an evergreen strand, a wire or a ribbon, with bulbs, beads or flags), particles behind
//! the content (snow, leaves, petals, stars, sparkles, confetti, rain), ornaments in the top
//! corners, a glow along the edges. Pure data: a module draws them (the app only finds it
//! among the contributions). A theme without decorations has none, and nothing is made for
//! it.
//!
//! Every kind's options are described once ([`LayerKind::fields`]): the theme files are read
//! by them, the studio writes and edits by them, the user's own layers are kept in the
//! settings by them ([`encode`] / [`decode`]).

pub mod presets;

use kernel::Rgba;

/// An enum of ids as written in a theme: `id()`, `parse()`, every value and every id.
macro_rules! ids {
    ($(#[$m:meta])* $name:ident default $d:ident { $($(#[$vm:meta])* $v:ident = $id:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($(#[$vm])* $v),+ }

        impl Default for $name {
            fn default() -> Self {
                Self::$d
            }
        }

        impl $name {
            /// Every value, in the order they're offered.
            pub const ALL: &'static [Self] = &[$(Self::$v),+];
            /// Their ids, in the same order.
            pub const IDS: &'static [&'static str] = &[$($id),+];

            /// As written in a theme.
            pub fn id(self) -> &'static str {
                match self { $(Self::$v => $id),+ }
            }

            pub fn parse(s: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.id() == s)
            }
        }
    };
}

ids! {
    /// A layer's kind (its `kind = "…"`).
    LayerKind default Garland {
        /// A string along the window's edges.
        Garland = "garland",
        /// Things drifting behind the content.
        Particles = "particles",
        /// Ornaments in the top corners.
        Corners = "corners",
        /// A glow (or a shade) along the window's edges.
        Glow = "glow",
        /// Columns of falling characters (plan 42's effects, [`EffectSpec`]).
        Matrix = "matrix",
        /// Stars coming toward you, or drifting.
        Starfield = "starfield",
        /// Bands of light waving slowly at the top.
        Aurora = "aurora",
        /// Glowing points wandering and pulsing.
        Fireflies = "fireflies",
        /// Bubbles rising, popping at the top.
        Bubbles = "bubbles",
        /// Rare bursts of sparks.
        Fireworks = "fireworks",
        /// A rare streak across the sky.
        ShootingStars = "shooting-stars",
        /// Two or three waves moving slowly along the bottom edge.
        Waves = "waves",
        /// A glowing line around the window, pulsing or turning its hue.
        Neon = "neon",
        /// Drifting points joined by fine lines when close.
        Constellation = "constellation",
        /// A perspective grid flowing along the bottom, with its sun.
        Synthwave = "synthwave",
        /// Rare flashes lighting the background, a forked bolt with them.
        Lightning = "lightning",
        /// Drops sliding slowly down the glass, a small trail behind them.
        Raindrops = "raindrops",
        /// Soft clouds floating slowly along the top.
        Clouds = "clouds",
        /// Slow bands of fog.
        Fog = "fog",
        /// A few butterflies on short flights, flapping their wings.
        Butterflies = "butterflies",
        /// Balloons rising and swaying, with their strings.
        Balloons = "balloons",
        /// Hearts floating up, gently pulsing.
        Hearts = "hearts",
        /// Bats flitting across.
        Bats = "bats",
        /// Blobs of colored wax rising and sinking, merging and parting.
        LavaLamp = "lava-lamp",
        /// A retro screen: scanlines (or a curved screen's shaded edges), a rare glitch.
        Crt = "crt",
        /// A rare glitch: a moment of shifted bands split in two colors (plan 45).
        Glitch = "glitch",
        /// Fine neon streaks falling, splashing at the bottom.
        NeonRain = "neon-rain",
        /// A few faint neon signs in the corners, flickering now and then.
        HoloSigns = "holo-signs",
    }
}

impl LayerKind {
    /// Whether it may never show behind the editor (its lines and bands would cross the
    /// text): the retro CRT and the glitch keep to the edges and the panels.
    pub fn never_behind_editor(self) -> bool {
        matches!(self, Self::Crt | Self::Glitch)
    }
}

ids! {
    /// What a garland's string is.
    Strand default Evergreen {
        /// An evergreen strand of needles.
        Evergreen = "evergreen",
        /// A thin wire.
        Wire = "wire",
        /// A satin ribbon.
        Ribbon = "ribbon",
    }
}

ids! {
    /// What hangs along a garland's string.
    Items default Bulbs {
        /// Light bulbs, with their glow.
        Bulbs = "bulbs",
        /// Round beads.
        Beads = "beads",
        /// Pennant flags (bunting).
        Flags = "flags",
        /// Nothing (the string alone, its ornaments).
        Nothing = "none",
    }
}

ids! {
    /// Which edges a garland runs along.
    GarlandEdges default TopSides {
        /// Across the top only.
        Top = "top",
        /// Across the top and down both sides (as far as [`GarlandSpec::drop`]).
        TopSides = "top-sides",
    }
}

ids! {
    /// What a particles layer is made of.
    ParticleShape default Snow {
        Snow = "snow",
        Leaves = "leaves",
        Petals = "petals",
        /// Small stars where they are, glowing softly in turn.
        Stars = "stars",
        /// Brief glints here and there.
        Sparkles = "sparkles",
        Confetti = "confetti",
        Rain = "rain",
    }
}

ids! {
    /// What the top corners hold.
    CornerStyle default Bow {
        /// A ribbon bow.
        Bow = "bow",
        /// Holly leaves with berries.
        Holly = "holly",
        /// A small star.
        Star = "star",
        /// A five-petal flower.
        Flower = "flower",
        /// A leaf.
        Leaf = "leaf",
        /// An icon of the app's set ([`CornerSpec::icon`]).
        Icon = "icon",
    }
}

ids! {
    /// How the edges glow.
    GlowStyle default Edge {
        /// A colored light from the edges inward.
        Edge = "edge",
        /// A shade toward the edges.
        Vignette = "vignette",
    }
}

mod effect;
mod fields;
mod layer_spec;
mod line;
mod resolve;

pub use effect::*;
use fields::field;
pub use fields::{Amount, Field, FieldError, FieldKind, FieldValue, hex, number_text};
pub use line::{decode, encode};
pub use resolve::{
    BindTo, Binding, Condition, DecorationLayer, DecorationsSpec, Merge, ResolveWith,
};

impl ParticleShape {
    /// Whether it falls through the window (else it stays where it is).
    pub fn falls(self) -> bool {
        !matches!(self, Self::Stars | Self::Sparkles)
    }

    /// Whether it turns as it falls (drawn in a few orientations).
    pub fn tumbles(self) -> bool {
        matches!(self, Self::Leaves | Self::Petals | Self::Confetti)
    }
}

impl Items {
    /// Whether its items glint (bulbs and beads; flags don't).
    pub fn glints(self) -> bool {
        matches!(self, Self::Bulbs | Self::Beads)
    }
}

/// The most item colors a garland or a particles layer has.
pub const MAX_LIGHT_COLORS: usize = 6;
/// The most ornament colors a garland has.
pub const MAX_ORNAMENT_COLORS: usize = 4;
/// The most a particles layer may cover (it stays behind the content, faint).
pub const MAX_SNOW_OPACITY: f32 = 0.5;
/// The most a glow may cover.
pub const MAX_GLOW_OPACITY: f32 = 0.6;
/// How far down the sides a garland may hang (of the window's height).
pub const MAX_DROP: f32 = 0.8;
/// The most layers a theme (or the user) has.
pub const MAX_LAYERS: usize = 8;

/// A string along the window's edges, complete.
#[derive(Debug, Clone, PartialEq)]
pub struct GarlandSpec {
    pub strand: Strand,
    pub items: Items,
    pub edges: GarlandEdges,
    /// How many items along it (0: none, 1: densest).
    pub lights: f32,
    /// How fast they twinkle (0: steady).
    pub twinkle: f32,
    /// The items' colors, in turn (1..=[`MAX_LIGHT_COLORS`]).
    pub colors: Vec<Rgba>,
    /// The ornaments' colors, in turn (none: no ornaments).
    pub ornaments: Vec<Rgba>,
    /// How much the bulbs glow around them (0: none; lower on light backgrounds).
    pub glow: f32,
    /// The string's color.
    pub wire: Rgba,
    /// How far down the sides it hangs (of the window's height, 0..=[`MAX_DROP`]).
    pub drop: f32,
    pub opacity: f32,
}

impl Default for GarlandSpec {
    fn default() -> Self {
        Self {
            strand: Strand::Evergreen,
            items: Items::Bulbs,
            edges: GarlandEdges::TopSides,
            lights: 0.6,
            twinkle: 0.4,
            colors: vec![Rgba::rgb(0xf2c66d), Rgba::rgb(0xd8434f), Rgba::rgb(0xf6e7c8)],
            ornaments: vec![Rgba::rgb(0xb3263a), Rgba::rgb(0xc9a14a)],
            glow: 0.6,
            wire: Rgba::rgb(0x2f5b3e),
            drop: 0.45,
            opacity: 0.9,
        }
    }
}

impl GarlandSpec {
    /// Whether anything hangs along it.
    pub fn has_items(&self) -> bool {
        self.items != Items::Nothing && self.lights > 0.0
    }
}

/// Particles behind the content, complete.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleSpec {
    pub shape: ParticleShape,
    /// How many (0: none, 1: most).
    pub density: f32,
    /// How fast they fall, or glint (0: barely, 1: fastest).
    pub speed: f32,
    /// Their colors, in turn (1..=[`MAX_LIGHT_COLORS`]).
    pub colors: Vec<Rgba>,
    /// At most [`MAX_SNOW_OPACITY`].
    pub opacity: f32,
    /// Where they show (only around the panels, unless asked).
    pub zone: Zone,
    /// How much shows through the panels (see [`EffectSpec::glass`]).
    pub glass: f32,
}

impl Default for ParticleSpec {
    fn default() -> Self {
        Self {
            shape: ParticleShape::Snow,
            density: 0.35,
            speed: 0.3,
            colors: vec![Rgba::WHITE],
            opacity: 0.16,
            zone: Zone::Edges,
            glass: 0.5,
        }
    }
}

/// The ornaments in the top corners, complete.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerSpec {
    pub style: CornerStyle,
    /// The icon's name, with [`CornerStyle::Icon`].
    pub icon: String,
    /// The main color (the ribbon, the berries, the star, the petals, the icon).
    pub color: Rgba,
    /// The accent (the bow's sprig, the leaves, the flower's heart).
    pub accent: Rgba,
    pub opacity: f32,
}

impl Default for CornerSpec {
    fn default() -> Self {
        Self {
            style: CornerStyle::Bow,
            icon: "bell".to_owned(),
            color: Rgba::rgb(0xb3263a),
            accent: Rgba::rgb(0x2f5b3e),
            opacity: 0.95,
        }
    }
}

/// A glow along the window's edges, complete (still: it never asks for frames).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowSpec {
    pub style: GlowStyle,
    pub color: Rgba,
    /// At most [`MAX_GLOW_OPACITY`].
    pub opacity: f32,
    /// How far in it reaches (0: a thin rim, 1: the widest).
    pub width: f32,
}

impl Default for GlowSpec {
    fn default() -> Self {
        Self { style: GlowStyle::Edge, color: Rgba::rgb(0xf2c66d), opacity: 0.16, width: 0.4 }
    }
}

/// What an effect costs to draw while it moves: light (a few images, rarely changing) or
/// medium (many, every frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cost {
    Light,
    Medium,
}

/// One layer of decorations, complete.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerSpec {
    Garland(GarlandSpec),
    Particles(ParticleSpec),
    Corners(CornerSpec),
    Glow(GlowSpec),
    /// One of the animated effects (Matrix, the stars, the aurora…).
    Effect(EffectSpec),
}

/// A name a theme or the settings may hold (an icon's, an option's): letters, digits, `-`,
/// `_`, `.`.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// A theme's decorations, complete: the layers to draw, in order (none: nothing drawn).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Decorations {
    pub layers: Vec<LayerSpec>,
    /// The theme is dark (the presets the user adds have a variant for each).
    pub dark: bool,
}

/// How the panels let what's behind them show ([`Decorations::see_through`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SeeThrough {
    /// The panels' (and, with `editor`, the editor's) background opacity, 0.85..0.97.
    pub card_opacity: f32,
    pub editor: bool,
    /// The colors drawn behind them, each with its opacity.
    pub colors: Vec<(Rgba, f32)>,
}

/// Where the user's own decorations go: over the theme's, or instead of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayMode {
    #[default]
    Add,
    Replace,
}

impl OverlayMode {
    pub const ALL: [Self; 2] = [Self::Add, Self::Replace];

    pub fn id(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Replace => "replace",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == s)
    }
}

impl Decorations {
    /// No decorations at all: nothing is made for the theme.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Whether anything in it moves.
    pub fn moves(&self) -> bool {
        self.layers.iter().any(LayerSpec::moves)
    }

    /// How the panels let the layers behind them show (none: they stay opaque): the most
    /// translucent card asked for, whether the editor too, and the colors behind them (each
    /// with how strongly it's drawn), for the text's contrast.
    pub fn see_through(&self) -> Option<SeeThrough> {
        let behind: Vec<&LayerSpec> =
            self.layers.iter().filter(|l| l.zone() != Zone::Edges).collect();
        if behind.is_empty() {
            return None;
        }
        let card_opacity =
            behind.iter().map(|l| 0.97 - 0.12 * l.glass().clamp(0.0, 1.0)).fold(1.0, f32::min);
        let editor = behind.iter().any(|l| l.zone() == Zone::Everywhere);
        let colors = behind
            .iter()
            .flat_map(|l| {
                let opacity = match l.get("opacity") {
                    Some(FieldValue::Number(v)) => v,
                    _ => 0.5,
                };
                let list = match l.get("colors") {
                    Some(FieldValue::Colors(list)) => list,
                    _ => Vec::new(),
                };
                list.into_iter().map(move |c| (c, opacity))
            })
            .collect();
        Some(SeeThrough { card_opacity, editor, colors })
    }

    /// The kinds it has, in order.
    pub fn kinds(&self) -> impl Iterator<Item = LayerKind> + '_ {
        self.layers.iter().map(LayerSpec::kind)
    }

    /// Its first layer of `kind`.
    pub fn first(&self, kind: LayerKind) -> Option<&LayerSpec> {
        self.layers.iter().find(|l| l.kind() == kind)
    }

    /// With the user's own layers, over or instead of these (at most [`MAX_LAYERS`]).
    pub fn with_overlay(&self, overlay: &[LayerSpec], mode: OverlayMode) -> Self {
        let mut layers = match mode {
            OverlayMode::Add => self.layers.clone(),
            OverlayMode::Replace => Vec::new(),
        };
        layers.extend(overlay.iter().cloned().map(LayerSpec::clamped));
        layers.truncate(MAX_LAYERS);
        Self { layers, dark: self.dark }
    }
}

#[cfg(test)]
#[path = "../tests/unit/decorations.rs"]
mod tests;
