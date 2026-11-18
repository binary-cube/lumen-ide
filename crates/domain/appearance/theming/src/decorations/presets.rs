//! Ready-made decorations — starting points, not locks: chosen in the studio they become
//! the theme's layers (then edited freely); chosen in the settings they're drawn over any
//! theme. Each has a variant for dark themes and one for light ones (fainter glows, darker
//! snow, so they read on either).

use kernel::Rgba;

use super::{
    CornerSpec, CornerStyle, EffectSpec, GarlandEdges, GarlandSpec, GlowSpec, GlowStyle, Items,
    LayerKind, LayerSpec, ParticleShape, ParticleSpec, Strand, Zone,
};

/// The presets' ids, in the order they're offered (their names: the locales'
/// `decorations.preset_<id>`, `-` as `_`).
pub const ALL: &[&str] = &[
    "christmas",
    "winter",
    "autumn",
    "spring",
    "halloween",
    "new-year",
    "minimal",
    "matrix",
    "cyberpunk",
    "space",
    "ocean",
    "synthwave",
    "storm",
    "valentine",
    "party",
    "retro-crt",
];

fn rgb(list: &[u32]) -> Vec<Rgba> {
    list.iter().map(|c| Rgba::rgb(*c)).collect()
}

/// Preset `id`'s layers, for a dark theme or a light one (`None`: no such preset).
pub fn layers(id: &str, dark: bool) -> Option<Vec<LayerSpec>> {
    let pick = |d: f32, l: f32| if dark { d } else { l };
    let pick_c = |d: u32, l: u32| Rgba::rgb(if dark { d } else { l });
    Some(match id {
        "christmas" => vec![
            LayerSpec::Garland(GarlandSpec {
                strand: Strand::Evergreen,
                items: Items::Bulbs,
                edges: GarlandEdges::TopSides,
                lights: 0.55,
                twinkle: 0.35,
                glow: pick(0.7, 0.3),
                colors: rgb(&[0xf0b860, 0xf4e8c8, 0xb5293c, 0xf0b860, 0xf4e8c8]),
                ornaments: rgb(&[0xb5293c, if dark { 0xe6c172 } else { 0xc99a2e }, 0x8e1f30]),
                wire: pick_c(0x3a6b49, 0x2e6440),
                drop: 0.42,
                opacity: pick(0.92, 0.95),
            }),
            LayerSpec::Corners(CornerSpec {
                style: CornerStyle::Bow,
                color: Rgba::rgb(0xb5293c),
                accent: pick_c(0x3a6b49, 0x2e6440),
                ..CornerSpec::default()
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Snow,
                density: 0.5,
                speed: 0.25,
                colors: vec![pick_c(0xffffff, 0x8fa6bd)],
                opacity: pick(0.26, 0.35),
                ..ParticleSpec::default()
            }),
        ],
        "winter" => vec![
            LayerSpec::Glow(GlowSpec {
                style: GlowStyle::Edge,
                color: pick_c(0x9fd0f5, 0x7fb2dc),
                opacity: pick(0.14, 0.16),
                width: 0.5,
            }),
            LayerSpec::Garland(GarlandSpec {
                strand: Strand::Wire,
                items: Items::Beads,
                edges: GarlandEdges::Top,
                lights: 0.7,
                twinkle: 0.3,
                glow: pick(0.3, 0.1),
                colors: if dark {
                    rgb(&[0xe8f1f8, 0xa9c8e0, 0xffffff, 0x7fa7c9])
                } else {
                    rgb(&[0x8fb3d1, 0x5f8db5, 0xb9cfe2, 0x46769f])
                },
                ornaments: Vec::new(),
                wire: pick_c(0x9fb3c4, 0x8193a3),
                drop: 0.4,
                opacity: 0.85,
            }),
            LayerSpec::Corners(CornerSpec {
                style: CornerStyle::Star,
                color: pick_c(0xd4e7f5, 0x5f8db5),
                accent: Rgba::WHITE,
                ..CornerSpec::default()
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Snow,
                density: 0.7,
                speed: 0.3,
                colors: vec![pick_c(0xffffff, 0x88a2bb)],
                opacity: pick(0.3, 0.38),
                ..ParticleSpec::default()
            }),
        ],
        "autumn" => vec![
            LayerSpec::Glow(GlowSpec {
                style: if dark { GlowStyle::Vignette } else { GlowStyle::Edge },
                color: pick_c(0x2a1206, 0xe3a35a),
                opacity: pick(0.3, 0.14),
                width: 0.55,
            }),
            LayerSpec::Garland(GarlandSpec {
                strand: Strand::Wire,
                items: Items::Flags,
                edges: GarlandEdges::Top,
                lights: 0.5,
                twinkle: 0.0,
                glow: 0.0,
                colors: rgb(&[0xc8642a, 0xd9a03f, 0x8a3b1f, 0xb5832e]),
                ornaments: Vec::new(),
                wire: pick_c(0x8a6a4a, 0x6b4a32),
                drop: 0.4,
                opacity: 0.9,
            }),
            LayerSpec::Corners(CornerSpec {
                style: CornerStyle::Leaf,
                color: Rgba::rgb(0xc8642a),
                accent: pick_c(0x8a5a34, 0x6b4a32),
                ..CornerSpec::default()
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Leaves,
                density: 0.45,
                speed: 0.3,
                colors: rgb(&[0xd2691e, 0xe0a33a, 0xa63d1e, 0x9c7a2c]),
                opacity: pick(0.42, 0.5),
                ..ParticleSpec::default()
            }),
        ],
        "spring" => vec![
            LayerSpec::Garland(GarlandSpec {
                strand: Strand::Wire,
                items: Items::Flags,
                edges: GarlandEdges::Top,
                lights: 0.65,
                twinkle: 0.0,
                glow: 0.0,
                colors: rgb(&[0xf4a7b9, 0xa8d8b9, 0xf9e0a0, 0xb9c8f0]),
                ornaments: Vec::new(),
                wire: pick_c(0x9aa88a, 0x7d8b6d),
                drop: 0.4,
                opacity: 0.9,
            }),
            LayerSpec::Corners(CornerSpec {
                style: CornerStyle::Flower,
                color: Rgba::rgb(0xf19bb4),
                accent: Rgba::rgb(0xf2c94c),
                ..CornerSpec::default()
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Petals,
                density: 0.4,
                speed: 0.25,
                colors: rgb(&[0xf6c1cf, 0xfbe3ea, 0xf3a6bd]),
                opacity: pick(0.4, 0.5),
                ..ParticleSpec::default()
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0xffb86b, 0x8cc8ff, 0xf7a1c4])
                } else {
                    rgb(&[0xe8862a, 0x3f8fd8, 0xd9578f])
                },
                density: 0.3,
                opacity: pick(0.45, 0.5),
                ..EffectSpec::new(LayerKind::Butterflies)
            }),
        ],
        "halloween" => vec![
            LayerSpec::Glow(GlowSpec {
                style: if dark { GlowStyle::Vignette } else { GlowStyle::Edge },
                color: pick_c(0x2b0d3d, 0xff8a1f),
                opacity: pick(0.35, 0.12),
                width: 0.6,
            }),
            LayerSpec::Garland(GarlandSpec {
                strand: Strand::Wire,
                items: Items::Bulbs,
                edges: GarlandEdges::TopSides,
                lights: 0.55,
                twinkle: 0.45,
                glow: pick(0.8, 0.35),
                colors: rgb(&[0xff8a1f, 0x9b59d0, 0x7ed957]),
                ornaments: Vec::new(),
                wire: pick_c(0x3a3a3a, 0x2b2b2b),
                drop: 0.35,
                opacity: 0.92,
            }),
            LayerSpec::Corners(CornerSpec {
                style: CornerStyle::Star,
                color: Rgba::rgb(0xff8a1f),
                accent: Rgba::rgb(0x9b59d0),
                ..CornerSpec::default()
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Sparkles,
                density: 0.35,
                speed: 0.4,
                colors: rgb(&[0xff8a1f, 0xb07ce0]),
                opacity: pick(0.4, 0.45),
                ..ParticleSpec::default()
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0x7a5c9e, 0x4c3a66]) } else { rgb(&[0x9a88b4, 0x7d6a99]) },
                density: 0.5,
                speed: 0.2,
                opacity: pick(0.22, 0.2),
                ..EffectSpec::new(LayerKind::Fog)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0x8e7bb0, 0x6f5c92]) } else { rgb(&[0x2d2238, 0x463457]) },
                density: 0.3,
                opacity: pick(0.45, 0.4),
                ..EffectSpec::new(LayerKind::Bats)
            }),
        ],
        "new-year" => vec![
            LayerSpec::Garland(GarlandSpec {
                strand: Strand::Wire,
                items: Items::Beads,
                edges: GarlandEdges::Top,
                lights: 0.8,
                twinkle: 0.5,
                glow: pick(0.4, 0.15),
                colors: if dark {
                    rgb(&[0xf2c66d, 0xe8e8f0, 0xd4af37])
                } else {
                    rgb(&[0xc99a2e, 0x9a9aa8, 0xb08a1e])
                },
                ornaments: Vec::new(),
                wire: pick_c(0xc9a14a, 0xa07f30),
                drop: 0.4,
                opacity: 0.9,
            }),
            LayerSpec::Corners(CornerSpec {
                style: CornerStyle::Star,
                color: pick_c(0xf2c66d, 0xc99a2e),
                accent: Rgba::WHITE,
                ..CornerSpec::default()
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Confetti,
                density: 0.45,
                speed: 0.35,
                colors: rgb(&[0xf2c66d, 0xe94f8a, 0x3cc6e8, 0x9b6cf0, 0x7ed3a1]),
                opacity: pick(0.42, 0.5),
                ..ParticleSpec::default()
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Sparkles,
                density: 0.25,
                speed: 0.5,
                colors: vec![pick_c(0xf2c66d, 0xc99a2e), pick_c(0xffffff, 0x9a9aa8)],
                opacity: pick(0.45, 0.4),
                ..ParticleSpec::default()
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0xf2c66d, 0xe94f8a, 0x3cc6e8, 0xffffff])
                } else {
                    rgb(&[0xc99a2e, 0xc2306c, 0x1d8fb0, 0x7a5bd0])
                },
                density: 0.3,
                opacity: pick(0.45, 0.5),
                ..EffectSpec::new(LayerKind::Fireworks)
            }),
        ],
        "minimal" => vec![LayerSpec::Garland(GarlandSpec {
            strand: Strand::Wire,
            items: Items::Bulbs,
            edges: GarlandEdges::Top,
            lights: 0.25,
            twinkle: 0.2,
            glow: pick(0.4, 0.2),
            colors: vec![pick_c(0xf2c66d, 0xe0a63a)],
            ornaments: Vec::new(),
            wire: pick_c(0x5a5f66, 0x9aa0a8),
            drop: 0.4,
            opacity: 0.75,
        })],
        // The Matrix theme's digital rain (plan 45), for any theme: faint, behind the panels.
        "matrix" => vec![LayerSpec::Effect(EffectSpec {
            colors: vec![pick_c(0x00ff41, 0x138a3a)],
            density: 0.4,
            speed: 0.4,
            glow: 0.6,
            opacity: pick(0.2, 0.28),
            zone: Zone::Panels,
            glass: 0.35,
            ..EffectSpec::new(LayerKind::Matrix)
        })],
        // Plan 45: a neon edge, the neon grid behind the panels, neon rain, a rare glitch and
        // a few faint holographic signs.
        "cyberpunk" => vec![
            LayerSpec::Effect(EffectSpec {
                style: "hue",
                colors: if dark { rgb(&[0xff2bd6, 0x00e5ff]) } else { rgb(&[0xc0179f, 0x0090b0]) },
                size: 0.2,
                opacity: pick(0.45, 0.45),
                ..EffectSpec::new(LayerKind::Neon)
            }),
            LayerSpec::Effect(EffectSpec {
                style: "grid",
                colors: if dark {
                    rgb(&[0x00e5ff, 0xff2bd6, 0xff2bd6])
                } else {
                    rgb(&[0x0090b0, 0xb0128f, 0xb0128f])
                },
                speed: 0.3,
                size: 0.4,
                opacity: pick(0.22, 0.28),
                zone: Zone::Panels,
                glass: 0.3,
                ..EffectSpec::new(LayerKind::Synthwave)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0xff2bd6, 0x00e5ff]) } else { rgb(&[0xb0128f, 0x0080a0]) },
                density: 0.35,
                opacity: pick(0.26, 0.32),
                ..EffectSpec::new(LayerKind::NeonRain)
            }),
            LayerSpec::Effect(EffectSpec {
                density: 0.2,
                opacity: pick(0.28, 0.3),
                ..EffectSpec::new(LayerKind::Glitch)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0xff2bd6, 0x00e5ff, 0xfcee0a])
                } else {
                    rgb(&[0xb0128f, 0x0080a0, 0x9a8a00])
                },
                density: 0.3,
                opacity: pick(0.32, 0.36),
                glass: 0.6,
                ..EffectSpec::new(LayerKind::HoloSigns)
            }),
        ],
        "space" => vec![
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0x6a5cff, 0x3ca0ff]) } else { rgb(&[0x7b6cf0, 0x4a90d9]) },
                size: 0.55,
                opacity: pick(0.3, 0.25),
                ..EffectSpec::new(LayerKind::Aurora)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0xffffff, 0xcfe0ff]) } else { rgb(&[0x5a6478, 0x8090b0]) },
                zone: Zone::Panels,
                ..EffectSpec::new(LayerKind::Starfield)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0x9fb8ff]) } else { rgb(&[0x6070a0]) },
                density: 0.35,
                opacity: pick(0.3, 0.35),
                ..EffectSpec::new(LayerKind::Constellation)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0xffffff, 0xbfe0ff]) } else { rgb(&[0x4a5878, 0x5a78b0]) },
                ..EffectSpec::new(LayerKind::ShootingStars)
            }),
        ],
        "ocean" => vec![
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0x9fd8ff, 0xc9f1ff]) } else { rgb(&[0x3d8fc4, 0x5aa8d6]) },
                opacity: pick(0.3, 0.38),
                ..EffectSpec::new(LayerKind::Bubbles)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0x3ce6c8, 0x7af0ff]) } else { rgb(&[0x149c88, 0x1d8fb0]) },
                density: 0.3,
                opacity: pick(0.4, 0.45),
                ..EffectSpec::new(LayerKind::Fireflies)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0x1f5fbf, 0x2f8ae0, 0x6cc4f5])
                } else {
                    rgb(&[0x2f80ed, 0x56a8f0, 0x8fcaf5])
                },
                opacity: pick(0.28, 0.32),
                ..EffectSpec::new(LayerKind::Waves)
            }),
        ],
        "synthwave" => vec![
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0xff2bd6, 0xffb347, 0xff4f8b])
                } else {
                    rgb(&[0xc0179f, 0xf0a030, 0xe0306c])
                },
                opacity: pick(0.35, 0.4),
                ..EffectSpec::new(LayerKind::Synthwave)
            }),
            LayerSpec::Effect(EffectSpec {
                style: "hue",
                colors: rgb(&[0xff2bd6, 0x7b5cff, 0x00e5ff]),
                size: 0.25,
                opacity: pick(0.55, 0.5),
                ..EffectSpec::new(LayerKind::Neon)
            }),
        ],
        "storm" => vec![
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0xe4ecff, 0xa9b8ff]) } else { rgb(&[0x4a5fc8, 0x8f9fe6]) },
                density: 0.4,
                opacity: pick(0.4, 0.35),
                ..EffectSpec::new(LayerKind::Lightning)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0x8792a6, 0x4d5668]) } else { rgb(&[0x8c97a8, 0x5f6a7c]) },
                density: 0.6,
                speed: 0.2,
                opacity: pick(0.3, 0.3),
                ..EffectSpec::new(LayerKind::Clouds)
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Rain,
                density: 0.5,
                speed: 0.6,
                colors: vec![pick_c(0x9fb4cc, 0x6d84a0)],
                opacity: pick(0.3, 0.35),
                ..ParticleSpec::default()
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark { rgb(&[0xcfe3ff]) } else { rgb(&[0x6f8db0]) },
                density: 0.4,
                opacity: pick(0.4, 0.45),
                ..EffectSpec::new(LayerKind::Raindrops)
            }),
        ],
        "valentine" => vec![
            LayerSpec::Glow(GlowSpec {
                style: GlowStyle::Edge,
                color: pick_c(0xff5c8a, 0xf28aa8),
                opacity: pick(0.14, 0.14),
                width: 0.5,
            }),
            LayerSpec::Corners(CornerSpec {
                style: CornerStyle::Flower,
                color: Rgba::rgb(0xf27a9b),
                accent: Rgba::rgb(0xffd6e0),
                ..CornerSpec::default()
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0xff5c8a, 0xff8fab, 0xe63e6d])
                } else {
                    rgb(&[0xe0406f, 0xf06f95, 0xc22a58])
                },
                density: 0.4,
                opacity: pick(0.4, 0.42),
                ..EffectSpec::new(LayerKind::Hearts)
            }),
        ],
        "party" => vec![
            LayerSpec::Garland(GarlandSpec {
                strand: Strand::Wire,
                items: Items::Flags,
                edges: GarlandEdges::Top,
                lights: 0.75,
                twinkle: 0.0,
                glow: 0.0,
                colors: rgb(&[0xef476f, 0xffd166, 0x06d6a0, 0x4cc9f0, 0x9b6cf0]),
                ornaments: Vec::new(),
                wire: pick_c(0xb0b4bc, 0x8a8f99),
                drop: 0.4,
                opacity: 0.9,
            }),
            LayerSpec::Particles(ParticleSpec {
                shape: ParticleShape::Confetti,
                density: 0.4,
                speed: 0.35,
                colors: rgb(&[0xef476f, 0xffd166, 0x06d6a0, 0x4cc9f0, 0x9b6cf0]),
                opacity: pick(0.42, 0.5),
                ..ParticleSpec::default()
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0xef476f, 0xffd166, 0x06d6a0, 0x4cc9f0, 0x9b6cf0])
                } else {
                    rgb(&[0xd63a5f, 0xe8b440, 0x05a87e, 0x2a9fd0, 0x7d55d8])
                },
                density: 0.3,
                opacity: pick(0.42, 0.45),
                zone: Zone::Panels,
                ..EffectSpec::new(LayerKind::Balloons)
            }),
        ],
        "retro-crt" => vec![
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0xff6b35, 0xff3d7f, 0xffb703])
                } else {
                    rgb(&[0xf08a5d, 0xf06292, 0xf2b84b])
                },
                density: 0.35,
                opacity: pick(0.2, 0.22),
                ..EffectSpec::new(LayerKind::LavaLamp)
            }),
            LayerSpec::Effect(EffectSpec {
                colors: if dark {
                    rgb(&[0x8fa3b8, 0xff3b6b, 0x3bd6ff])
                } else {
                    rgb(&[0x3a4050, 0xe0305c, 0x1aa0c8])
                },
                opacity: pick(0.06, 0.05),
                zone: Zone::Panels,
                ..EffectSpec::new(LayerKind::Crt)
            }),
        ],
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../../tests/unit/decorations/presets.rs"]
mod tests;
