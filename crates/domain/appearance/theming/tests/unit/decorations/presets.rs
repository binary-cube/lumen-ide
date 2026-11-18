//! The presets: each exists in both variants, within its limits (resolving changes nothing),
//! the light variant fainter or darker where it matters, and an unknown id has none.

use super::*;
use crate::decorations::{DecorationsSpec, LayerKind, ResolveWith};

#[test]
fn every_preset_has_both_variants_within_their_limits() {
    for id in ALL {
        for dark in [true, false] {
            let layers = layers(id, dark).unwrap_or_else(|| panic!("{id}"));
            assert!(!layers.is_empty(), "{id}");
            let clamped: Vec<LayerSpec> = layers.iter().cloned().map(LayerSpec::clamped).collect();
            assert_eq!(clamped, layers, "{id} ({dark}): within its limits");
            let resolved = DecorationsSpec::of(layers.clone()).resolve(ResolveWith::default());
            assert_eq!(resolved.layers, layers);
        }
    }
    assert!(layers("easter", true).is_none());
}

/// Between them, the presets show every kind and most styles (they're the kinds' examples).
#[test]
fn the_presets_show_the_kinds() {
    let all: Vec<LayerSpec> = ALL.iter().flat_map(|id| layers(id, true).unwrap()).collect();
    for kind in LayerKind::ALL {
        assert!(all.iter().any(|l| l.kind() == *kind), "{kind:?}");
    }
    let shapes: Vec<ParticleShape> = all
        .iter()
        .filter_map(|l| match l {
            LayerSpec::Particles(p) => Some(p.shape),
            _ => None,
        })
        .collect();
    for shape in
        [ParticleShape::Snow, ParticleShape::Leaves, ParticleShape::Petals, ParticleShape::Confetti]
    {
        assert!(shapes.contains(&shape), "{shape:?}");
    }
}

/// Snow on a light theme isn't white (it wouldn't show).
#[test]
fn the_light_variants_read_on_light_backgrounds() {
    for id in ["christmas", "winter"] {
        let light = layers(id, false).unwrap();
        let snow = light
            .iter()
            .find_map(|l| match l {
                LayerSpec::Particles(p) if p.shape == ParticleShape::Snow => Some(p),
                _ => None,
            })
            .unwrap();
        assert!(snow.colors.iter().all(|c| c.r < 0.8), "{id}: {:?}", snow.colors);
    }
}

/// Cyberpunk (plan 45): a neon edge, the grid behind the panels, neon rain, a rare glitch,
/// faint holographic signs; Matrix's rain is a preset of its own, behind the panels.
#[test]
fn cyberpunk_and_matrix_are_what_they_say() {
    use crate::decorations::Zone;
    for dark in [true, false] {
        let kinds: Vec<LayerKind> =
            layers("cyberpunk", dark).unwrap().iter().map(LayerSpec::kind).collect();
        assert_eq!(
            kinds,
            [
                LayerKind::Neon,
                LayerKind::Synthwave,
                LayerKind::NeonRain,
                LayerKind::Glitch,
                LayerKind::HoloSigns
            ]
        );
        let matrix = layers("matrix", dark).unwrap();
        assert_eq!(matrix.len(), 1);
        assert_eq!((matrix[0].kind(), matrix[0].zone()), (LayerKind::Matrix, Zone::Panels));
    }
}
