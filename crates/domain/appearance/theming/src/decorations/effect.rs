//! The animated effects (plan 42): Matrix rain, a star field, an aurora, fireflies, bubbles,
//! fireworks, shooting stars, waves, a neon edge, a constellation, a synthwave grid,
//! lightning, raindrops on the glass, clouds, fog, butterflies, balloons, hearts, bats, a
//! lava lamp, a retro CRT; plan 45's glitch, neon rain and holographic signs. One
//! shape of data for all of them ([`EffectSpec`]): a style (its kind's own choices: Matrix's
//! characters, the stars' motion, the neon's pulse…), colors, a density, a speed, a size, a
//! glow, an opacity, and — for those behind the content — where they show ([`Zone`]) and how
//! much of them shows through the panels ([`EffectSpec::glass`]). Each kind says which of
//! these it has and their limits ([`fields`]).

use kernel::Rgba;

use super::{Amount, Field, FieldKind, LayerKind, MAX_SNOW_OPACITY, field};

ids! {
    /// Where an effect behind the content shows.
    Zone default Edges {
        /// Only around the panels: the top bar, the strips, the gaps (as before).
        Edges = "edges",
        /// Also behind the side panels and the bottom window, seen through them (faintly:
        /// they turn translucent, their text stays as it is).
        Panels = "panels",
        /// Behind the editor too.
        Everywhere = "everywhere",
    }
}

/// Matrix's characters.
pub const MATRIX_STYLES: &[&str] = &["katakana", "digits", "binary", "latin"];
/// How the stars move: toward you from the middle, or drifting.
pub const STARFIELD_STYLES: &[&str] = &["warp", "drift"];
/// How the neon edge changes: it pulses, or its hue turns.
pub const NEON_STYLES: &[&str] = &["pulse", "hue"];
/// The synthwave grid: with its sun, the grid alone, or a city's skyline on its horizon.
pub const SYNTHWAVE_STYLES: &[&str] = &["sun", "grid", "city"];
/// A glitch: bands across its zone, or short blocks.
pub const GLITCH_STYLES: &[&str] = &["bands", "blocks"];
/// Lightning: a forked bolt with its flash, or the flash alone.
pub const LIGHTNING_STYLES: &[&str] = &["bolt", "flash"];
/// Fog: lying along the bottom, or veils across the whole window.
pub const FOG_STYLES: &[&str] = &["ground", "veil"];
/// A retro CRT: scanlines over its zone, or a curved screen's shaded edges.
pub const CRT_STYLES: &[&str] = &["scanlines", "curved"];
/// Where a retro CRT may show: never behind the editor (its lines would cross the text).
pub const CRT_ZONES: &[&str] = &["edges", "panels"];

/// The most a retro CRT may cover (subtle: it's over a whole zone).
pub const MAX_CRT_OPACITY: f32 = 0.3;

/// The most a neon edge may cover (it's thin, on the edges only).
pub const MAX_NEON_OPACITY: f32 = 0.9;

const UNIT: FieldKind = FieldKind::Amount(Amount(100));
const FAINT: FieldKind = FieldKind::Amount(Amount(50));

const ZONE: Field = field("zone", FieldKind::Choice(Zone::IDS));
const GLASS: Field = field("glass", UNIT);

const MATRIX: &[Field] = &[
    field("style", FieldKind::Choice(MATRIX_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const STARFIELD: &[Field] = &[
    field("style", FieldKind::Choice(STARFIELD_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 4 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const AURORA: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 4 }),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const FIREFLIES: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const BUBBLES: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const FIREWORKS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 6 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];

const SHOOTING_STARS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const WAVES: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const NEON: &[Field] = &[
    field("style", FieldKind::Choice(NEON_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 4 }),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FieldKind::Amount(Amount(90))),
];
const CONSTELLATION: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const SYNTHWAVE: &[Field] = &[
    field("style", FieldKind::Choice(SYNTHWAVE_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const LIGHTNING: &[Field] = &[
    field("style", FieldKind::Choice(LIGHTNING_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 2 }),
    field("density", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const RAINDROPS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const CLOUDS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 2 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const FOG: &[Field] = &[
    field("style", FieldKind::Choice(FOG_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const BUTTERFLIES: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 4 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const BALLOONS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 6 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const HEARTS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 4 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const BATS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 2 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const LAVA_LAMP: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const CRT: &[Field] = &[
    field("style", FieldKind::Choice(CRT_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("size", UNIT),
    field("opacity", FieldKind::Amount(Amount(30))),
    field("zone", FieldKind::Choice(CRT_ZONES)),
    GLASS,
];

const GLITCH: &[Field] = &[
    field("style", FieldKind::Choice(GLITCH_STYLES)),
    field("colors", FieldKind::Colors { min: 1, max: 3 }),
    field("density", UNIT),
    field("size", UNIT),
    field("opacity", FAINT),
    field("zone", FieldKind::Choice(CRT_ZONES)),
    GLASS,
];
const NEON_RAIN: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 4 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];
const HOLO_SIGNS: &[Field] = &[
    field("colors", FieldKind::Colors { min: 1, max: 4 }),
    field("density", UNIT),
    field("speed", UNIT),
    field("size", UNIT),
    field("glow", UNIT),
    field("opacity", FAINT),
    ZONE,
    GLASS,
];

/// The options of an effect of `kind` (none for the kinds that aren't effects).
pub fn fields(kind: LayerKind) -> &'static [Field] {
    match kind {
        LayerKind::Matrix => MATRIX,
        LayerKind::Starfield => STARFIELD,
        LayerKind::Aurora => AURORA,
        LayerKind::Fireflies => FIREFLIES,
        LayerKind::Bubbles => BUBBLES,
        LayerKind::Fireworks => FIREWORKS,
        LayerKind::ShootingStars => SHOOTING_STARS,
        LayerKind::Waves => WAVES,
        LayerKind::Neon => NEON,
        LayerKind::Constellation => CONSTELLATION,
        LayerKind::Synthwave => SYNTHWAVE,
        LayerKind::Lightning => LIGHTNING,
        LayerKind::Raindrops => RAINDROPS,
        LayerKind::Clouds => CLOUDS,
        LayerKind::Fog => FOG,
        LayerKind::Butterflies => BUTTERFLIES,
        LayerKind::Balloons => BALLOONS,
        LayerKind::Hearts => HEARTS,
        LayerKind::Bats => BATS,
        LayerKind::LavaLamp => LAVA_LAMP,
        LayerKind::Crt => CRT,
        LayerKind::Glitch => GLITCH,
        LayerKind::NeonRain => NEON_RAIN,
        LayerKind::HoloSigns => HOLO_SIGNS,
        _ => &[],
    }
}

/// An effect, complete (what an effect's kind doesn't have keeps its default, unused).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectSpec {
    pub kind: LayerKind,
    /// Its style: one of its kind's ([`MATRIX_STYLES`]…; none: the first).
    pub style: &'static str,
    pub colors: Vec<Rgba>,
    /// How many (0: none, 1: most): columns, stars, fireflies, bubbles, bursts, waves,
    /// streaks, signs…; how often for the rare ones (lightning, glitches).
    pub density: f32,
    /// How fast it moves (0: barely).
    pub speed: f32,
    /// How big: Matrix's trails, the stars, the aurora's height, a burst, a shooting star's
    /// streak, the waves' height, the neon's width, the links' reach, the grid's height, a
    /// bolt's length, a drop, a cloud, the fog's depth, a butterfly, a balloon, a heart, a
    /// bat, a blob of wax, the scanlines' pitch, a glitch's bands, a neon streak, a sign.
    pub size: f32,
    /// How bright its lights are (Matrix's heads, the stars' twinkle, the fireflies'
    /// pulse, the neon's halo, the lightning's flash, the hearts' halo and beat, the grid's
    /// horizon, the neon rain's heads and splashes, the signs' halo).
    pub glow: f32,
    pub opacity: f32,
    pub zone: Zone,
    /// How much of it shows through the panels (0: barely — they stay 97 % opaque; 1:
    /// most — 85 %), with [`Zone::Panels`] / [`Zone::Everywhere`].
    pub glass: f32,
}

impl EffectSpec {
    /// An effect of `kind` with its defaults (tasteful: faint, slow).
    pub fn new(kind: LayerKind) -> Self {
        let rgb = |list: &[u32]| list.iter().map(|c| Rgba::rgb(*c)).collect::<Vec<_>>();
        let base = Self {
            kind,
            style: "",
            colors: rgb(&[0xffffff]),
            density: 0.5,
            speed: 0.4,
            size: 0.5,
            glow: 0.5,
            opacity: 0.3,
            zone: Zone::Edges,
            glass: 0.5,
        };
        let style = |list: &[&'static str]| list[0];
        match kind {
            LayerKind::Matrix => Self {
                style: style(MATRIX_STYLES),
                colors: rgb(&[0x3cff7a]),
                density: 0.55,
                speed: 0.45,
                size: 0.5,
                glow: 0.7,
                opacity: 0.35,
                ..base
            },
            LayerKind::Starfield => Self {
                style: style(STARFIELD_STYLES),
                colors: rgb(&[0xffffff, 0xcfe0ff]),
                density: 0.5,
                speed: 0.3,
                size: 0.4,
                glow: 0.5,
                opacity: 0.4,
                ..base
            },
            LayerKind::Aurora => Self {
                colors: rgb(&[0x3ce6a0, 0x46b4ff, 0xa86bff]),
                speed: 0.25,
                size: 0.65,
                opacity: 0.4,
                ..base
            },
            LayerKind::Fireflies => Self {
                colors: rgb(&[0xf5e27a, 0xc8f07a]),
                density: 0.4,
                speed: 0.3,
                size: 0.4,
                glow: 0.6,
                opacity: 0.45,
                ..base
            },
            LayerKind::Bubbles => Self {
                colors: rgb(&[0x9fd8ff, 0xc9f1ff]),
                density: 0.4,
                speed: 0.35,
                size: 0.45,
                opacity: 0.3,
                ..base
            },
            LayerKind::Fireworks => Self {
                colors: rgb(&[0xffd166, 0xef476f, 0x06d6a0, 0x4cc9f0, 0xffffff]),
                density: 0.35,
                speed: 0.5,
                size: 0.5,
                opacity: 0.45,
                ..base
            },
            LayerKind::ShootingStars => Self {
                colors: rgb(&[0xffffff, 0xbfe0ff]),
                density: 0.35,
                speed: 0.6,
                size: 0.5,
                opacity: 0.45,
                ..base
            },
            LayerKind::Waves => Self {
                colors: rgb(&[0x2f80ed, 0x56ccf2, 0x9be7ff]),
                density: 0.6,
                speed: 0.3,
                size: 0.45,
                opacity: 0.25,
                ..base
            },
            LayerKind::Neon => Self {
                style: style(NEON_STYLES),
                colors: rgb(&[0xff2bd6, 0x00e5ff]),
                speed: 0.35,
                size: 0.3,
                glow: 0.6,
                opacity: 0.7,
                ..base
            },
            LayerKind::Constellation => Self {
                colors: rgb(&[0xbcd4ff]),
                density: 0.45,
                speed: 0.25,
                size: 0.5,
                opacity: 0.35,
                ..base
            },
            LayerKind::Synthwave => Self {
                style: style(SYNTHWAVE_STYLES),
                colors: rgb(&[0xff2bd6, 0xffb347, 0xff4f8b]),
                speed: 0.35,
                size: 0.45,
                opacity: 0.35,
                ..base
            },
            LayerKind::Lightning => Self {
                style: style(LIGHTNING_STYLES),
                colors: rgb(&[0xe4ecff, 0xa9b8ff]),
                density: 0.35,
                size: 0.5,
                glow: 0.6,
                opacity: 0.4,
                ..base
            },
            LayerKind::Raindrops => Self {
                colors: rgb(&[0xcfe3ff]),
                density: 0.45,
                speed: 0.3,
                size: 0.45,
                opacity: 0.4,
                ..base
            },
            LayerKind::Clouds => Self {
                colors: rgb(&[0xe9eef6, 0xb8c4d4]),
                density: 0.45,
                speed: 0.25,
                size: 0.5,
                opacity: 0.22,
                ..base
            },
            LayerKind::Fog => Self {
                style: style(FOG_STYLES),
                colors: rgb(&[0xc9d3df, 0xaeb9c8]),
                density: 0.6,
                speed: 0.25,
                size: 0.5,
                opacity: 0.2,
                ..base
            },
            LayerKind::Butterflies => Self {
                colors: rgb(&[0xffa94d, 0x74c0fc, 0xf783ac]),
                density: 0.35,
                speed: 0.4,
                size: 0.45,
                opacity: 0.45,
                ..base
            },
            LayerKind::Balloons => Self {
                colors: rgb(&[0xef476f, 0xffd166, 0x06d6a0, 0x4cc9f0, 0x9b6cf0]),
                density: 0.35,
                speed: 0.3,
                size: 0.5,
                opacity: 0.4,
                ..base
            },
            LayerKind::Hearts => Self {
                colors: rgb(&[0xff5c8a, 0xff8fab, 0xe63e6d]),
                density: 0.35,
                speed: 0.3,
                size: 0.45,
                glow: 0.5,
                opacity: 0.38,
                ..base
            },
            LayerKind::Bats => Self {
                colors: rgb(&[0x6a5a86]),
                density: 0.3,
                speed: 0.5,
                size: 0.45,
                opacity: 0.45,
                ..base
            },
            LayerKind::LavaLamp => Self {
                colors: rgb(&[0xff6b35, 0xff3d7f, 0xffb703]),
                density: 0.45,
                speed: 0.3,
                size: 0.55,
                opacity: 0.22,
                ..base
            },
            LayerKind::Crt => Self {
                style: style(CRT_STYLES),
                colors: rgb(&[0x000000, 0xff3b6b, 0x3bd6ff]),
                density: 0.3,
                size: 0.35,
                opacity: 0.07,
                ..base
            },
            LayerKind::Glitch => Self {
                style: style(GLITCH_STYLES),
                colors: rgb(&[0xff2b6b, 0x2bd6ff, 0xffffff]),
                density: 0.3,
                size: 0.45,
                opacity: 0.3,
                ..base
            },
            LayerKind::NeonRain => Self {
                colors: rgb(&[0xff2bd6, 0x00e5ff]),
                density: 0.45,
                speed: 0.5,
                size: 0.45,
                glow: 0.5,
                opacity: 0.3,
                ..base
            },
            LayerKind::HoloSigns => Self {
                colors: rgb(&[0xff2bd6, 0x00e5ff, 0xfcee0a]),
                density: 0.35,
                speed: 0.4,
                size: 0.5,
                glow: 0.6,
                opacity: 0.4,
                zone: Zone::Panels,
                glass: 0.8,
                ..base
            },
            _ => base,
        }
    }

    /// The most it may cover (faint behind the content; the neon edge brighter).
    pub fn max_opacity(&self) -> f32 {
        match self.kind {
            LayerKind::Neon => MAX_NEON_OPACITY,
            LayerKind::Crt => MAX_CRT_OPACITY,
            _ => MAX_SNOW_OPACITY,
        }
    }

    /// The styles its kind offers.
    pub fn styles(&self) -> &'static [&'static str] {
        match self.kind {
            LayerKind::Matrix => MATRIX_STYLES,
            LayerKind::Starfield => STARFIELD_STYLES,
            LayerKind::Neon => NEON_STYLES,
            LayerKind::Synthwave => SYNTHWAVE_STYLES,
            LayerKind::Lightning => LIGHTNING_STYLES,
            LayerKind::Fog => FOG_STYLES,
            LayerKind::Crt => CRT_STYLES,
            LayerKind::Glitch => GLITCH_STYLES,
            _ => &[],
        }
    }

    /// The card opacity it asks for where it shows through the panels (97 %…85 %).
    pub fn card_opacity(&self) -> f32 {
        0.97 - 0.12 * self.glass.clamp(0.0, 1.0)
    }
}
