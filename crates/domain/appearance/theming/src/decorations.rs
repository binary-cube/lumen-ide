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
pub use effect::*;

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

/// The kind of value an option of a layer takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// One of these ids.
    Choice(&'static [&'static str]),
    /// A number from 0 to its maximum.
    Amount(Amount),
    Color,
    /// A list of colors, from `min` to `max` of them.
    Colors {
        min: usize,
        max: usize,
    },
    /// A name (an icon's).
    Text,
}

/// An amount's maximum, kept exactly (`f32` isn't `Eq`): in hundredths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(pub u16);

impl Amount {
    pub fn max(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

/// An option of a layer: its key (as written in a theme) and the value it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub key: &'static str,
    pub kind: FieldKind,
}

const fn field(key: &'static str, kind: FieldKind) -> Field {
    Field { key, kind }
}

const UNIT: FieldKind = FieldKind::Amount(Amount(100));

const GARLAND_FIELDS: &[Field] = &[
    field("strand", FieldKind::Choice(Strand::IDS)),
    field("items", FieldKind::Choice(Items::IDS)),
    field("edges", FieldKind::Choice(GarlandEdges::IDS)),
    field("lights", UNIT),
    field("twinkle", UNIT),
    field("glow", UNIT),
    field("colors", FieldKind::Colors { min: 1, max: MAX_LIGHT_COLORS }),
    field("ornaments", FieldKind::Colors { min: 0, max: MAX_ORNAMENT_COLORS }),
    field("wire", FieldKind::Color),
    field("drop", FieldKind::Amount(Amount(80))),
    field("opacity", UNIT),
];

const PARTICLE_FIELDS: &[Field] = &[
    field("shape", FieldKind::Choice(ParticleShape::IDS)),
    field("density", UNIT),
    field("speed", UNIT),
    field("colors", FieldKind::Colors { min: 1, max: MAX_LIGHT_COLORS }),
    field("opacity", FieldKind::Amount(Amount(50))),
    field("zone", FieldKind::Choice(Zone::IDS)),
    field("glass", UNIT),
];

const CORNER_FIELDS: &[Field] = &[
    field("style", FieldKind::Choice(CornerStyle::IDS)),
    field("icon", FieldKind::Text),
    field("colors", FieldKind::Colors { min: 1, max: 2 }),
    field("opacity", UNIT),
];

const GLOW_FIELDS: &[Field] = &[
    field("style", FieldKind::Choice(GlowStyle::IDS)),
    field("color", FieldKind::Color),
    field("opacity", FieldKind::Amount(Amount(60))),
    field("width", UNIT),
];

impl LayerKind {
    /// Its options, in the order they're written and shown.
    pub fn fields(self) -> &'static [Field] {
        match self {
            Self::Garland => GARLAND_FIELDS,
            Self::Particles => PARTICLE_FIELDS,
            Self::Corners => CORNER_FIELDS,
            Self::Glow => GLOW_FIELDS,
            kind => effect::fields(kind),
        }
    }

    /// Its option `key`.
    pub fn field(self, key: &str) -> Option<&'static Field> {
        self.fields().iter().find(|f| f.key == key)
    }
}

/// A value of a layer's option.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Number(f32),
    Text(String),
    Color(Rgba),
    Colors(Vec<Rgba>),
}

/// Why a layer's option can't take a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldError {
    /// The kind has no such option.
    Unknown,
    /// Not a value it takes (why).
    Invalid(&'static str),
}

/// A number written as a theme writes it (two decimals at most, no trailing zeros).
pub fn number_text(v: f32) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    format!("{rounded}")
}

/// A color as a theme writes it (`#rrggbb`, `#rrggbbaa` when translucent).
pub fn hex(c: Rgba) -> String {
    let [r, g, b, a] = c.to_rgba8();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

fn amount(value: &FieldValue, max: f32) -> Result<f32, FieldError> {
    match value {
        FieldValue::Number(v) if v.is_finite() && (0.0..=max + 1e-6).contains(v) => Ok(v.min(max)),
        FieldValue::Number(_) => {
            Err(FieldError::Invalid(if max == 1.0 { "from 0 to 1" } else { "out of range" }))
        }
        _ => Err(FieldError::Invalid("a number")),
    }
}

fn choice<T: Copy>(value: &FieldValue, parse: fn(&str) -> Option<T>) -> Result<T, FieldError> {
    match value {
        FieldValue::Text(s) => parse(s).ok_or(FieldError::Invalid("not one of its choices")),
        _ => Err(FieldError::Invalid("not one of its choices")),
    }
}

fn color(value: &FieldValue) -> Result<Rgba, FieldError> {
    match value {
        FieldValue::Color(c) => Ok(*c),
        _ => Err(FieldError::Invalid("a color")),
    }
}

fn colors(value: &FieldValue, min: usize, max: usize) -> Result<Vec<Rgba>, FieldError> {
    match value {
        FieldValue::Colors(list) if (min..=max).contains(&list.len()) => Ok(list.clone()),
        FieldValue::Colors(_) => Err(FieldError::Invalid("too few or too many colors")),
        _ => Err(FieldError::Invalid("a list of colors")),
    }
}

impl LayerSpec {
    /// A layer of `kind` with its defaults.
    pub fn new(kind: LayerKind) -> Self {
        match kind {
            LayerKind::Garland => Self::Garland(GarlandSpec::default()),
            LayerKind::Particles => Self::Particles(ParticleSpec::default()),
            LayerKind::Corners => Self::Corners(CornerSpec::default()),
            LayerKind::Glow => Self::Glow(GlowSpec::default()),
            kind => Self::Effect(EffectSpec::new(kind)),
        }
    }

    pub fn kind(&self) -> LayerKind {
        match self {
            Self::Garland(_) => LayerKind::Garland,
            Self::Particles(_) => LayerKind::Particles,
            Self::Corners(_) => LayerKind::Corners,
            Self::Glow(_) => LayerKind::Glow,
            Self::Effect(e) => e.kind,
        }
    }

    /// Where it shows (behind the content: around the panels, or also through them).
    pub fn zone(&self) -> Zone {
        match self {
            Self::Particles(p) => p.zone,
            // The neon edge is on the edges only.
            Self::Effect(e) if e.kind == LayerKind::Neon => Zone::Edges,
            // A retro screen never goes behind the editor (its lines would cross the text).
            Self::Effect(e) if e.kind.never_behind_editor() && e.zone == Zone::Everywhere => {
                Zone::Panels
            }
            Self::Effect(e) => e.zone,
            _ => Zone::Edges,
        }
    }

    /// How much of it shows through the panels (0..1; with a zone other than the edges).
    pub fn glass(&self) -> f32 {
        match self {
            Self::Particles(p) => p.glass,
            Self::Effect(e) => e.glass,
            _ => 0.0,
        }
    }

    /// The value of its option `key`.
    pub fn get(&self, key: &str) -> Option<FieldValue> {
        use FieldValue::{Color, Colors, Number, Text};
        let text = |s: &str| Some(Text(s.to_owned()));
        match self {
            Self::Garland(g) => match key {
                "strand" => text(g.strand.id()),
                "items" => text(g.items.id()),
                "edges" => text(g.edges.id()),
                "lights" => Some(Number(g.lights)),
                "twinkle" => Some(Number(g.twinkle)),
                "glow" => Some(Number(g.glow)),
                "colors" => Some(Colors(g.colors.clone())),
                "ornaments" => Some(Colors(g.ornaments.clone())),
                "wire" => Some(Color(g.wire)),
                "drop" => Some(Number(g.drop)),
                "opacity" => Some(Number(g.opacity)),
                _ => None,
            },
            Self::Particles(p) => match key {
                "shape" => text(p.shape.id()),
                "density" => Some(Number(p.density)),
                "speed" => Some(Number(p.speed)),
                "colors" => Some(Colors(p.colors.clone())),
                "opacity" => Some(Number(p.opacity)),
                "zone" => text(p.zone.id()),
                "glass" => Some(Number(p.glass)),
                _ => None,
            },
            Self::Effect(e) => {
                self.kind().field(key)?;
                match key {
                    "style" => text(e.style),
                    "colors" => Some(Colors(e.colors.clone())),
                    "density" => Some(Number(e.density)),
                    "speed" => Some(Number(e.speed)),
                    "size" => Some(Number(e.size)),
                    "glow" => Some(Number(e.glow)),
                    "opacity" => Some(Number(e.opacity)),
                    "zone" => text(e.zone.id()),
                    "glass" => Some(Number(e.glass)),
                    _ => None,
                }
            }
            Self::Corners(c) => match key {
                "style" => text(c.style.id()),
                "icon" => text(&c.icon),
                "colors" => Some(Colors(vec![c.color, c.accent])),
                "opacity" => Some(Number(c.opacity)),
                _ => None,
            },
            Self::Glow(g) => match key {
                "style" => text(g.style.id()),
                "color" => Some(Color(g.color)),
                "opacity" => Some(Number(g.opacity)),
                "width" => Some(Number(g.width)),
                _ => None,
            },
        }
    }

    /// Sets its option `key` (checked against the option's kind and limits). A particles
    /// layer also takes `color` (one color: the format before the layers had one).
    pub fn set(&mut self, key: &str, value: &FieldValue) -> Result<(), FieldError> {
        let Some(field) = self.kind().field(key).copied().or_else(|| {
            (self.kind() == LayerKind::Particles && key == "color")
                .then_some(field("color", FieldKind::Color))
        }) else {
            return Err(FieldError::Unknown);
        };
        let max = match field.kind {
            FieldKind::Amount(a) => a.max(),
            _ => 1.0,
        };
        let (min_n, max_n) = match field.kind {
            FieldKind::Colors { min, max } => (min, max),
            _ => (0, 0),
        };
        match self {
            Self::Garland(g) => match key {
                "strand" => g.strand = choice(value, Strand::parse)?,
                "items" => g.items = choice(value, Items::parse)?,
                "edges" => {
                    g.edges = choice(value, |s| {
                        // `sides`: an alias written by the first version of the format.
                        if s == "sides" {
                            Some(GarlandEdges::TopSides)
                        } else {
                            GarlandEdges::parse(s)
                        }
                    })?;
                }
                "lights" => g.lights = amount(value, max)?,
                "twinkle" => g.twinkle = amount(value, max)?,
                "glow" => g.glow = amount(value, max)?,
                "colors" => g.colors = colors(value, min_n, max_n)?,
                "ornaments" => g.ornaments = colors(value, min_n, max_n)?,
                "wire" => g.wire = color(value)?,
                "drop" => g.drop = amount(value, max)?,
                "opacity" => g.opacity = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Particles(p) => match key {
                "shape" => p.shape = choice(value, ParticleShape::parse)?,
                "density" => p.density = amount(value, max)?,
                "speed" => p.speed = amount(value, max)?,
                "colors" => p.colors = colors(value, min_n, max_n)?,
                "color" => p.colors = vec![color(value)?],
                "opacity" => p.opacity = amount(value, max)?,
                "zone" => p.zone = choice(value, Zone::parse)?,
                "glass" => p.glass = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Effect(e) => match key {
                "style" => {
                    let styles = e.styles();
                    e.style = match value {
                        FieldValue::Text(s) => styles
                            .iter()
                            .copied()
                            .find(|x| x == s)
                            .ok_or(FieldError::Invalid("not one of its choices"))?,
                        _ => return Err(FieldError::Invalid("not one of its choices")),
                    };
                }
                "colors" => e.colors = colors(value, min_n, max_n)?,
                "density" => e.density = amount(value, max)?,
                "speed" => e.speed = amount(value, max)?,
                "size" => e.size = amount(value, max)?,
                "glow" => e.glow = amount(value, max)?,
                "opacity" => e.opacity = amount(value, max)?,
                "zone" => {
                    let zone = choice(value, Zone::parse)?;
                    // Only the places its kind offers.
                    if !matches!(field.kind, FieldKind::Choice(ids) if ids.contains(&zone.id())) {
                        return Err(FieldError::Invalid("not one of its choices"));
                    }
                    e.zone = zone;
                }
                "glass" => e.glass = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Corners(c) => match key {
                "style" => c.style = choice(value, CornerStyle::parse)?,
                "icon" => match value {
                    FieldValue::Text(s) if valid_name(s) => c.icon = s.clone(),
                    _ => return Err(FieldError::Invalid("an icon's name")),
                },
                "colors" => {
                    let list = colors(value, min_n, max_n)?;
                    c.color = list[0];
                    if let Some(accent) = list.get(1) {
                        c.accent = *accent;
                    }
                }
                "opacity" => c.opacity = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Glow(g) => match key {
                "style" => g.style = choice(value, GlowStyle::parse)?,
                "color" => g.color = color(value)?,
                "opacity" => g.opacity = amount(value, max)?,
                "width" => g.width = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
        }
        Ok(())
    }

    /// Color `index` of option `key` (a list's, or the color itself at 0).
    pub fn color_at(&self, key: &str, index: usize) -> Option<Rgba> {
        match self.get(key)? {
            FieldValue::Color(c) if index == 0 => Some(c),
            FieldValue::Colors(list) => list.get(index).copied(),
            _ => None,
        }
    }

    /// Sets color `index` of option `key` (a list's, or the color itself at 0). `false` if
    /// there's no such color.
    pub fn set_color_at(&mut self, key: &str, index: usize, c: Rgba) -> bool {
        let value = match self.get(key) {
            Some(FieldValue::Color(_)) if index == 0 => FieldValue::Color(c),
            Some(FieldValue::Colors(mut list)) if index < list.len() => {
                list[index] = c;
                FieldValue::Colors(list)
            }
            _ => return false,
        };
        self.set(key, &value).is_ok()
    }

    /// Whether anything in it moves (glinting items, falling or glowing particles).
    pub fn moves(&self) -> bool {
        match self {
            Self::Garland(g) => g.twinkle > 0.0 && g.has_items() && g.items.glints(),
            Self::Particles(p) => p.density > 0.0 && (p.shape.falls() || p.speed > 0.0),
            Self::Corners(_) | Self::Glow(_) => false,
            Self::Effect(e) => match e.kind {
                LayerKind::Aurora | LayerKind::Neon | LayerKind::Synthwave => e.speed > 0.0,
                // Rare strikes (glitches): something happens now and then, whatever the speed.
                LayerKind::Lightning | LayerKind::Crt | LayerKind::Glitch => e.density > 0.0,
                _ => e.density > 0.0 && e.speed > 0.0,
            },
        }
    }

    /// What it costs to draw while it moves, roughly (the studio says it beside each
    /// effect, and adds them up).
    pub fn cost(&self) -> Cost {
        match self {
            Self::Corners(_) | Self::Glow(_) => Cost::Light,
            Self::Garland(g) if g.twinkle > 0.0 && g.has_items() => Cost::Medium,
            Self::Garland(_) => Cost::Light,
            Self::Particles(p) if p.density > 0.5 => Cost::Medium,
            Self::Particles(_) => Cost::Light,
            Self::Effect(e) => match e.kind {
                LayerKind::Matrix
                | LayerKind::Fireworks
                | LayerKind::Constellation
                | LayerKind::LavaLamp => Cost::Medium,
                LayerKind::Starfield | LayerKind::NeonRain if e.density > 0.5 => Cost::Medium,
                _ => Cost::Light,
            },
        }
    }

    /// The values kept in their limits (what a reader already checked, made safe for any
    /// caller: a layer built in code, the studio, the settings).
    pub fn clamped(mut self) -> Self {
        let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        match &mut self {
            Self::Garland(g) => {
                g.lights = unit(g.lights);
                g.twinkle = unit(g.twinkle);
                g.glow = unit(g.glow);
                g.drop = unit(g.drop).min(MAX_DROP);
                g.opacity = unit(g.opacity);
                g.colors.truncate(MAX_LIGHT_COLORS);
                if g.colors.is_empty() {
                    g.colors = GarlandSpec::default().colors;
                }
                g.ornaments.truncate(MAX_ORNAMENT_COLORS);
            }
            Self::Particles(p) => {
                p.density = unit(p.density);
                p.speed = unit(p.speed);
                p.opacity = unit(p.opacity).min(MAX_SNOW_OPACITY);
                p.colors.truncate(MAX_LIGHT_COLORS);
                if p.colors.is_empty() {
                    p.colors = ParticleSpec::default().colors;
                }
            }
            Self::Corners(c) => {
                c.opacity = unit(c.opacity);
                if !valid_name(&c.icon) {
                    c.icon = CornerSpec::default().icon;
                }
            }
            Self::Glow(g) => {
                g.opacity = unit(g.opacity).min(MAX_GLOW_OPACITY);
                g.width = unit(g.width);
            }
            Self::Effect(e) => {
                e.density = unit(e.density);
                e.speed = unit(e.speed);
                e.size = unit(e.size);
                e.glow = unit(e.glow);
                e.glass = unit(e.glass);
                e.opacity = unit(e.opacity).min(e.max_opacity());
                e.colors.truncate(MAX_LIGHT_COLORS);
                if e.colors.is_empty() {
                    e.colors = EffectSpec::new(e.kind).colors;
                }
                if e.kind.never_behind_editor() && e.zone == Zone::Everywhere {
                    e.zone = Zone::Panels;
                }
                if !e.styles().contains(&e.style) {
                    e.style = e.styles().first().copied().unwrap_or("");
                }
            }
        }
        if let Self::Particles(p) = &mut self {
            p.glass = unit(p.glass);
        }
        self
    }
}

/// A name a theme or the settings may hold (an icon's, an option's): letters, digits, `-`,
/// `_`, `.`.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// What a layer's option follows instead of its own value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindTo {
    /// A name of the theme's palette (`"$name"`): a theme option may change it.
    Palette(String),
    /// A theme option's value (`"@id"`).
    Option(String),
}

/// One of a layer's options following something: the option, which of its colors (a
/// list's; 0 otherwise), and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub key: &'static str,
    pub index: usize,
    pub to: BindTo,
}

/// When a layer is drawn: a theme option on (`"id"`), off (`"!id"`), or a choice
/// (`"id=value"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub option: String,
    pub value: Option<String>,
    pub negate: bool,
}

impl Condition {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (negate, s) = match s.strip_prefix('!') {
            Some(rest) => (true, rest.trim()),
            None => (false, s),
        };
        let (option, value) = match s.split_once('=') {
            Some((o, v)) => (o.trim(), Some(v.trim().to_owned())),
            None => (s, None),
        };
        (valid_name(option) && value.as_deref().is_none_or(valid_name)).then(|| Self {
            option: option.to_owned(),
            value,
            negate,
        })
    }

    /// As written in a theme.
    pub fn text(&self) -> String {
        let not = if self.negate { "!" } else { "" };
        match &self.value {
            Some(v) => format!("{not}{}={v}", self.option),
            None => format!("{not}{}", self.option),
        }
    }
}

/// A layer as a theme declares it: its values (with its defaults for what it doesn't say),
/// when it's drawn and what of it follows the theme's palette or options.
#[derive(Debug, Clone, PartialEq)]
pub struct DecorationLayer {
    pub spec: LayerSpec,
    pub when: Option<Condition>,
    pub bindings: Vec<Binding>,
    /// Drawn (`enabled = false` in a theme: kept, its options too, but not drawn — the
    /// studio's switch).
    pub enabled: bool,
}

impl DecorationLayer {
    /// A layer with only its own values.
    pub fn plain(spec: LayerSpec) -> Self {
        Self { spec, when: None, bindings: Vec::new(), enabled: true }
    }

    /// What option `key`'s color `index` follows.
    pub fn binding(&self, key: &str, index: usize) -> Option<&BindTo> {
        self.bindings.iter().find(|b| b.key == key && b.index == index).map(|b| &b.to)
    }

    /// Option `key` set to a value of its own: it follows nothing any more.
    pub fn unbind(&mut self, key: &str) {
        self.bindings.retain(|b| b.key != key);
    }

    /// Option `key`'s color `index` set to a value of its own.
    pub fn unbind_at(&mut self, key: &str, index: usize) {
        self.bindings.retain(|b| b.key != key || b.index != index);
    }
}

/// How a theme's decorations combine with those of the theme it extends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Merge {
    /// It declares none: the extended theme's.
    #[default]
    Inherit,
    /// The first format's sections (`[decorations.garland]`…): each kind it declares
    /// replaces the extended theme's of that kind, the rest stay.
    ByKind,
    /// A list of layers: it replaces the extended theme's whole.
    Replace,
}

/// What a theme says of its decorations.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DecorationsSpec {
    pub layers: Vec<DecorationLayer>,
    pub merge: Merge,
}

/// What a theme's layers are resolved with: its options' values, the names of its palette
/// they change, and whether it's dark.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolveWith<'a> {
    pub values: &'a [(String, crate::options::OptionValue)],
    pub palette: &'a [(String, Rgba)],
    pub dark: bool,
}

impl DecorationsSpec {
    /// A list of plain layers (replacing what a theme extends).
    pub fn of(layers: impl IntoIterator<Item = LayerSpec>) -> Self {
        Self {
            layers: layers.into_iter().map(DecorationLayer::plain).collect(),
            merge: Merge::Replace,
        }
    }

    /// `self` over `under` (see [`Merge`]).
    pub fn over(self, under: Self) -> Self {
        match self.merge {
            Merge::Inherit => under,
            Merge::Replace => self,
            Merge::ByKind => {
                let mut layers = under.layers;
                for layer in self.layers {
                    let kind = layer.spec.kind();
                    match layers.iter().position(|l| l.spec.kind() == kind) {
                        Some(at) => layers[at] = layer,
                        None => layers.push(layer),
                    }
                }
                Self { layers, merge: Merge::Replace }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The decorations to draw: each layer whose condition holds, what it follows applied,
    /// limits kept.
    pub fn resolve(self, with: ResolveWith<'_>) -> Decorations {
        use crate::options::OptionValue;
        let value = |id: &str| with.values.iter().find(|(k, _)| k == id).map(|(_, v)| v);
        let layers = self
            .layers
            .into_iter()
            .filter(|layer| {
                if !layer.enabled {
                    return false;
                }
                let Some(c) = &layer.when else { return true };
                let holds = match (value(&c.option), &c.value) {
                    // An option the theme doesn't have: drawn (its file got a note).
                    (None, _) => return true,
                    (Some(OptionValue::Bool(b)), None) => *b,
                    (Some(OptionValue::Choice(chosen)), Some(v)) => chosen == v,
                    (Some(OptionValue::Choice(_)), None) => true,
                    (Some(OptionValue::Number(n)), None) => *n > 0.0,
                    (Some(_), Some(_)) => false,
                    (Some(OptionValue::Color(_)), None) => true,
                };
                holds != c.negate
            })
            .take(MAX_LAYERS)
            .map(|layer| {
                let mut spec = layer.spec;
                for b in &layer.bindings {
                    match &b.to {
                        BindTo::Palette(name) => {
                            if let Some((_, c)) = with.palette.iter().find(|(n, _)| n == name) {
                                spec.set_color_at(b.key, b.index, *c);
                            }
                        }
                        BindTo::Option(id) => match value(id) {
                            Some(OptionValue::Number(n)) => {
                                let max = match spec.kind().field(b.key).map(|f| f.kind) {
                                    Some(FieldKind::Amount(a)) => a.max(),
                                    _ => 1.0,
                                };
                                let _ = spec.set(b.key, &FieldValue::Number(n.clamp(0.0, max)));
                            }
                            Some(OptionValue::Color(c)) => {
                                spec.set_color_at(b.key, b.index, *c);
                            }
                            Some(OptionValue::Bool(on)) => {
                                let _ = spec
                                    .set(b.key, &FieldValue::Number(if *on { 1.0 } else { 0.0 }));
                            }
                            _ => {}
                        },
                    }
                }
                spec.clamped()
            })
            .collect();
        Decorations { layers, dark: with.dark }
    }
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

// ---------------------------------------------------------------------------------------
// One layer as one line of text (the user's own layers, in the settings):
// `kind=garland;strand=wire;lights=0.5;colors=#f2c66d,#ffffff`.

/// A layer as one line: its kind, then every option.
pub fn encode(layer: &LayerSpec) -> String {
    let mut out = format!("kind={}", layer.kind().id());
    for f in layer.kind().fields() {
        let Some(value) = layer.get(f.key) else { continue };
        let text = match value {
            FieldValue::Number(v) => number_text(v),
            FieldValue::Text(s) => s,
            FieldValue::Color(c) => hex(c),
            FieldValue::Colors(list) => list.iter().map(|c| hex(*c)).collect::<Vec<_>>().join(","),
        };
        out.push_str(&format!(";{}={text}", f.key));
    }
    out
}

/// A layer from its line (an unknown kind or option, a wrong value: why).
pub fn decode(line: &str) -> Result<LayerSpec, String> {
    let mut parts = line.split(';').map(str::trim).filter(|p| !p.is_empty());
    let kind = parts
        .next()
        .and_then(|p| p.strip_prefix("kind="))
        .and_then(LayerKind::parse)
        .ok_or_else(|| format!("no kind in `{line}`"))?;
    let mut spec = LayerSpec::new(kind);
    for part in parts {
        let (key, text) = part.split_once('=').ok_or_else(|| format!("`{part}`: no value"))?;
        let field =
            kind.field(key).ok_or_else(|| format!("{}.{key}: no such option", kind.id()))?;
        let value = match field.kind {
            FieldKind::Choice(_) | FieldKind::Text => FieldValue::Text(text.to_owned()),
            FieldKind::Amount(_) => FieldValue::Number(
                text.parse().map_err(|_| format!("{}.{key} = {text}: not a number", kind.id()))?,
            ),
            FieldKind::Color => FieldValue::Color(
                Rgba::from_hex(text).ok_or_else(|| format!("{}.{key} = {text}", kind.id()))?,
            ),
            FieldKind::Colors { .. } => FieldValue::Colors(
                text.split(',')
                    .filter(|c| !c.is_empty())
                    .map(|c| {
                        Rgba::from_hex(c.trim()).ok_or_else(|| format!("{}.{key} = {c}", kind.id()))
                    })
                    .collect::<Result<_, _>>()?,
            ),
        };
        spec.set(key, &value).map_err(|e| match e {
            FieldError::Unknown => format!("{}.{key}: no such option", kind.id()),
            FieldError::Invalid(why) => format!("{}.{key} = {text}: {why}", kind.id()),
        })?;
    }
    Ok(spec)
}

#[cfg(test)]
#[path = "../tests/unit/decorations.rs"]
mod tests;
