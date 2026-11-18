use super::*;
use crate::Quad;
use kernel::Rgba;

/// A small radius is reduced less than a large one; the gaussian's weights sum to 1 and
/// fall off from the middle.
#[test]
fn plans_follow_the_radius() {
    let small = BlurPlan::for_radius(8.0);
    let large = BlurPlan::for_radius(40.0);
    assert_eq!(small.factor, 2);
    assert!(large.factor >= 4 && large.factor <= 8);
    for plan in [small, large, BlurPlan::for_radius(200.0), BlurPlan::for_radius(0.0)] {
        assert!(plan.taps >= 1 && plan.taps <= MAX_TAPS, "{plan:?}");
        let w = plan.weights();
        let total = w[0] + 2.0 * w[1..].iter().sum::<f32>();
        assert!((total - 1.0).abs() < 1e-5);
        assert!(w.windows(2).all(|p| p[0] >= p[1]));
    }
    let bounds = PixelRect::new(0, 0, 101, 40);
    assert_eq!(small.reduced(bounds), (51, 20));
}

/// The layers that blur, with the pixels their blur covers (within their clip and the
/// surface); none when nothing blurs.
#[test]
fn blur_layers_are_found_with_their_bounds() {
    let mut scene = Scene::default();
    scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 50.0, 50.0), Rgba::WHITE));
    let surface = PixelRect::new(0, 0, 200, 100);
    let mut out = Vec::new();
    blur_layers(&scene, surface, &mut out);
    assert!(out.is_empty());
    let layer = scene.push_layer(Some(Rect::new(0.0, 0.0, 150.0, 100.0)));
    layer.backdrop_blur(Rect::new(-10.0, 0.0, 100.0, 40.0), 16.0, 1.0);
    layer.backdrop_blur(Rect::new(0.0, 60.0, 300.0, 40.0), 16.0, 1.0);
    // Faded out or too small a radius: nothing.
    layer.backdrop_blur(Rect::new(0.0, 0.0, 10.0, 10.0), 16.0, 0.0);
    assert_eq!(layer.blurs.len(), 2);
    blur_layers(&scene, surface, &mut out);
    assert_eq!(out, [BlurLayer { layer: 1, bounds: PixelRect::new(0, 0, 150, 100), radius: 16.0 }]);
}

/// A rounded blur's mask: whole inside, nothing past its rounded corner, half on its edge.
#[test]
fn a_rounded_blur_covers_inside_its_corners() {
    let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
    assert_eq!(rounded_coverage(&rect, 20.0, 50.0, 30.0), 1.0, "inside");
    assert_eq!(rounded_coverage(&rect, 20.0, 1.0, 1.0), 0.0, "past the corner's curve");
    assert_eq!(rounded_coverage(&rect, 0.0, 0.5, 0.5), 1.0, "square: its corner is in");
    assert!((rounded_coverage(&rect, 20.0, 50.0, 0.0) - 0.5).abs() < 1e-4, "on the edge");
    // A radius larger than the rectangle allows: a pill, still whole in the middle.
    assert_eq!(rounded_coverage(&rect, 500.0, 50.0, 30.0), 1.0);
}
