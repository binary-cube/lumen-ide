//! The drawings: each the size its layout gives, anti-aliased (partly covered pixels at the
//! edges), transparent around, never darkened at the edges (the transparent pixels carry
//! the shape's color), mirrored exactly.

use super::*;
use theming::decorations::{
    CornerSpec, CornerStyle, GarlandSpec, GlowSpec, GlowStyle, Items, ParticleShape, Strand,
};

fn alphas(pixels: &[u8]) -> Vec<u8> {
    pixels.chunks(4).map(|p| p[3]).collect()
}

#[test]
fn every_drawing_has_its_size_soft_edges_and_clear_corners() {
    let g = GarlandSpec::default();
    for scale in [1.0, 2.0, 3.0] {
        let shape = SwagShape {
            width: 150.0 * scale,
            height: (18.0 * scale).ceil(),
            scale,
            lights: 6,
            hang: 2.6,
        };
        let side = SideShape { width: 14.0 * scale, length: 90.0 * scale, scale, hang: 2.6 };
        let mut drawings = vec![
            ("swag", art_swag(&shape, &g)),
            ("side", side_art(&side, &g)),
            ("ornament", ornament(scale, g.ornaments[0])),
            ("sparkle", sparkle(scale, g.colors[0])),
            ("flake 0", flake(scale, 0, kernel::Rgba::WHITE)),
            ("flake 2", flake(scale, 2, kernel::Rgba::WHITE)),
            ("bow", corner(scale, &CornerSpec::default())),
            (
                "holly",
                corner(scale, &CornerSpec { style: CornerStyle::Holly, ..CornerSpec::default() }),
            ),
            (
                "star",
                corner(scale, &CornerSpec { style: CornerStyle::Star, ..CornerSpec::default() }),
            ),
            (
                "flower",
                corner(scale, &CornerSpec { style: CornerStyle::Flower, ..CornerSpec::default() }),
            ),
            (
                "leaf",
                corner(scale, &CornerSpec { style: CornerStyle::Leaf, ..CornerSpec::default() }),
            ),
        ];
        // Every string with every kind of item.
        for strand in Strand::ALL {
            for items in Items::ALL {
                let g = GarlandSpec { strand: *strand, items: *items, ..GarlandSpec::default() };
                drawings.push(("garland swag", art_swag(&shape, &g)));
                drawings.push(("garland side", side_art(&side, &g)));
            }
        }
        for shape in ParticleShape::ALL {
            drawings.push(("particle", particle(*shape, scale, 1, kernel::Rgba::rgb(0xd2691e))));
        }
        for (name, canvas) in drawings {
            let (w, h) = (canvas.width as usize, canvas.height as usize);
            let pixels = canvas.into_rgba8();
            assert_eq!(pixels.len(), w * h * 4, "{name}");
            let a = alphas(&pixels);
            // (The smallest flake is a faint dot.)
            let solid = if name.starts_with("flake") || name == "particle" { 100 } else { 200 };
            assert!(a.iter().any(|v| *v > solid), "{name} at {scale}: something solid");
            assert!(a.iter().any(|v| *v > 10 && *v < 200), "{name} at {scale}: soft edges");
            assert!(a.contains(&0), "{name} at {scale}: transparent around");
            // The transparent pixels aren't black (filtering keeps the edges' color).
            let clear = pixels.chunks(4).find(|p| p[3] == 0).unwrap();
            assert!(clear[0] > 0 || clear[1] > 0 || clear[2] > 0, "{name}");
        }
    }
}

fn art_swag(shape: &SwagShape, g: &GarlandSpec) -> Canvas {
    swag(shape, g)
}

fn side_art(shape: &SideShape, g: &GarlandSpec) -> Canvas {
    side(shape, g)
}

#[test]
fn the_strand_follows_its_curve_and_the_bulbs_their_places() {
    let g = GarlandSpec::default();
    let scale = 2.0;
    let shape = SwagShape { width: 300.0, height: 32.0, scale, lights: 6, hang: 2.6 };
    let canvas = swag(&shape, &g);
    let w = canvas.width as usize;
    let pixels = canvas.into_rgba8();
    let alpha = |x: f32, y: f32| pixels[((y as usize) * w + x as usize) * 4 + 3];
    for x in [20.0, 75.0, 150.0, 230.0, 280.0] {
        assert!(alpha(x, shape.strand_y(x)) > 200, "the strand at {x}");
    }
    for k in 0..shape.lights {
        let p = shape.light(k);
        assert!(alpha(p.x, p.y) > 200, "bulb {k}");
    }
    // Under the strand and its bulbs: nothing.
    assert_eq!(alpha(150.0 + 37.0, 30.0), 0);
}

#[test]
fn mirrored_is_exactly_reversed() {
    let c = corner(2.0, &CornerSpec { style: CornerStyle::Holly, ..CornerSpec::default() });
    let w = c.width as usize;
    let straight = corner(2.0, &CornerSpec { style: CornerStyle::Holly, ..CornerSpec::default() })
        .into_rgba8();
    let mirrored = c.mirrored().into_rgba8();
    for y in 0..w {
        for x in 0..w {
            let a = &straight[(y * w + x) * 4..(y * w + x) * 4 + 4];
            let b = &mirrored[(y * w + (w - 1 - x)) * 4..(y * w + (w - 1 - x)) * 4 + 4];
            assert_eq!(a, b);
        }
    }
}

/// The glow's strips: as deep as asked, strongest at the window's edge, fading to nothing
/// inward (each pointing its own way); a vignette fades more slowly.
#[test]
fn the_glow_fades_inward_from_each_edge() {
    let spec = GlowSpec::default();
    let [top, bottom, left, right] = glow_strips(&spec, 40.0);
    assert_eq!((top.width, top.height, left.width, left.height), (4, 40, 40, 4));
    // The alphas across the strip, from the window's edge inward.
    let across = |c: Canvas, vertical: bool| {
        let w = c.width as usize;
        let a = alphas(&c.into_rgba8());
        (0..40).map(|i| if vertical { a[i * w + 1] } else { a[w + i] }).collect::<Vec<u8>>()
    };
    let t = across(top, true);
    assert!(t.windows(2).all(|w| w[0] >= w[1]) && t[0] > 200 && t[39] < 5, "{t:?}");
    let b = across(bottom, true);
    assert!(b.windows(2).all(|w| w[0] <= w[1]), "the bottom's from below");
    let l = across(left, false);
    assert!(l.windows(2).all(|w| w[0] >= w[1]), "the left's from the left");
    let r = across(right, false);
    assert!(r.windows(2).all(|w| w[0] <= w[1]), "the right's from the right");
    let vignette = GlowSpec { style: GlowStyle::Vignette, ..spec };
    let [v, ..] = glow_strips(&vignette, 40.0);
    assert!(across(v, true)[10] > t[10], "a vignette fades more slowly");
}
