//! The ring's rasterizer (coverage at its edges, the arc at 0, ½ and a whole turn, round
//! caps) and its image kept until what it shows changes.

use super::*;

const ARC: Rgba = Rgba::new(1.0, 0.0, 0.0, 1.0);
const TRACK: Rgba = Rgba::new(0.0, 0.0, 1.0, 0.25);

/// A 48 px ring: its stroke's middle at 20 px, 4 px wide (18..22 from the center).
fn spec(track: Option<Rgba>) -> RingSpec {
    RingSpec { size: 48, radius: 20.0, stroke: 4.0, color: ARC, track }
}

/// The pixel whose center is nearest `(dx, dy)` from the image's center.
fn pixel(pixels: &[u8], size: u32, dx: f32, dy: f32) -> [u8; 4] {
    let mid = size as f32 / 2.0;
    let (x, y) = ((mid + dx - 0.5).round() as usize, (mid + dy - 0.5).round() as usize);
    let i = (y * size as usize + x) * 4;
    pixels[i..i + 4].try_into().unwrap()
}

#[test]
fn coverage_ramps_over_one_pixel_at_the_edge() {
    assert_eq!(coverage(-3.0), 1.0, "well inside");
    assert_eq!(coverage(-0.5), 1.0);
    assert_eq!(coverage(0.0), 0.5, "the center on the edge: half covered");
    assert_eq!(coverage(0.25), 0.25);
    assert_eq!(coverage(0.5), 0.0);
    assert_eq!(coverage(4.0), 0.0, "well outside");
    // A whole ring's edges: the stroke's middle inside, its edges half, beyond nothing.
    let d = |r: f32| arc_distance(0.0, -r, 20.0, 4.0, 0.0, 1.0);
    assert_eq!(coverage(d(20.0)), 1.0);
    assert!((coverage(d(22.0)) - 0.5).abs() < 1e-4);
    assert!((coverage(d(18.0)) - 0.5).abs() < 1e-4);
    assert_eq!(coverage(d(23.0)), 0.0);
    assert_eq!(coverage(d(10.0)), 0.0);
}

#[test]
fn the_edges_are_smooth_not_stepped() {
    // Along a radius through the stroke the alpha rises and falls through partial values.
    let px = rasterize(&RingSpec { radius: 20.3, ..spec(None) }, 0.0, 1.0);
    let alphas: Vec<u8> = (14..28).map(|r| pixel(&px, 48, 0.5, -(r as f32) + 0.5)[3]).collect();
    assert!(alphas.iter().any(|&a| a > 0 && a < 255), "partial coverage: {alphas:?}");
    assert_eq!(alphas.iter().copied().max(), Some(255));
    assert_eq!(pixel(&px, 48, 0.5, 0.5)[3], 0, "nothing at the center");
    assert_eq!(pixel(&px, 48, 23.5, 23.5)[3], 0, "nothing in the corner");
}

#[test]
fn progress_none_half_and_whole() {
    let s = spec(Some(TRACK));
    let (top, right, bottom, left) = ((0.5, -19.5), (19.5, 0.5), (0.5, 19.5), (-19.5, 0.5));
    let at = |px: &[u8], (x, y): (f32, f32)| pixel(px, 48, x, y);
    let track = TRACK.to_rgba8();
    let arc = ARC.to_rgba8();
    // None: only the faint track, all around.
    let none = rasterize(&s, 0.0, 0.0);
    for p in [top, right, bottom, left] {
        assert_eq!(at(&none, p), track, "{p:?}");
    }
    // Half: clockwise from the top, the right side is the arc's, the left the track's.
    let half = rasterize(&s, 0.0, 0.5);
    assert_eq!(at(&half, right), arc);
    assert_eq!(at(&half, left), track);
    // Whole: the arc all around, no seam at the top.
    let whole = rasterize(&s, 0.0, 1.0);
    for p in [top, right, bottom, left, (-0.5, -19.5)] {
        assert_eq!(at(&whole, p), arc, "{p:?}");
    }
    // A transparent pixel keeps the arc's color (no dark fringe when filtered).
    assert_eq!(at(&whole, (0.5, 0.5)), [255, 0, 0, 0]);
}

#[test]
fn the_ends_are_round_caps() {
    // A quarter from the top: it ends at 3 o'clock, (20, 0).
    let d = |x: f32, y: f32| arc_distance(x, y, 20.0, 4.0, 0.0, 0.25);
    // Past the end, along the tangent, within half the stroke: the cap covers it.
    assert!(d(20.0, 1.5) < 0.0);
    assert!((d(20.0, 2.0)).abs() < 1e-4, "the cap's edge at half the stroke");
    // A square cap's corner: outside the round one.
    assert!(d(21.8, 1.8) > 0.0);
    // Far past the end: nothing.
    assert!(d(0.0, 20.0) > 10.0);
    // Inside the span, the band; before the start (just left of the top), the start's cap.
    assert!(d(14.14, -14.14) < -1.0);
    assert!(d(-1.5, -20.0) < 0.0 && d(-3.0, -20.0) > 0.0);
    // None: nothing anywhere.
    assert_eq!(arc_distance(0.0, -20.0, 20.0, 4.0, 0.0, 0.0), f32::INFINITY);
    // Drawn: a pixel just past the end is partly covered, one beyond the cap not.
    let px = rasterize(&spec(None), 0.0, 0.25);
    assert!(pixel(&px, 48, 20.5, 1.5)[3] > 0);
    assert_eq!(pixel(&px, 48, 20.5, 4.5)[3], 0);
}

#[test]
fn a_start_turns_the_arc() {
    // From 3 o'clock, a quarter: the bottom-right covered, the top-right not.
    let px = rasterize(&spec(None), 0.25, 0.25);
    assert_eq!(pixel(&px, 48, 14.5, 14.5)[3], 255);
    assert_eq!(pixel(&px, 48, 14.5, -13.5)[3], 0);
}

#[test]
fn the_image_is_kept_until_what_it_shows_changes() {
    let s = spec(Some(TRACK));
    let mut ring = ProgressRing::default();
    assert!(ring.kept().is_none());
    let first = ring.image(&s, 0.0, 0.5).id;
    assert_eq!(ring.image(&s, 0.0, 0.5).id, first, "the same: not rasterized again");
    assert_eq!(ring.image(&s, 0.0, 0.5 + 0.3 / f32::from(STEPS)).id, first, "less than a step");
    let kept = ring.image(&s, 0.0, 0.5).clone();
    let next = ring.image(&s, 0.0, 0.5 + 1.0 / f32::from(STEPS)).id;
    assert_ne!(next, first, "a step: another image");
    assert!(kept.released(), "the last one's texture freed at once");
    assert!(!ring.kept().unwrap().released());
    assert_ne!(
        ring.image(&RingSpec { color: TRACK, ..s }, 0.0, 0.5 + 1.0 / f32::from(STEPS)).id,
        next
    );
    assert_ne!(ring.image(&RingSpec { size: 50, ..s }, 0.0, 0.5).id, next);
    // A start a whole turn on is the same start.
    let a = ring.image(&s, 0.1, 0.5).id;
    assert_eq!(ring.image(&s, 1.1, 0.5).id, a);
    let image = ring.kept().unwrap();
    assert_eq!((image.width, image.height), (48, 48));
    assert_eq!(ring.memory(), 48 * 48 * 4);
    // Drawn centered, on whole pixels, the same image every frame.
    let mut layer = Layer::default();
    ring.paint(&mut layer, Point::new(100.3, 50.6), &s, (0.1, 0.5), 0.5);
    ring.paint(&mut layer, Point::new(100.3, 50.6), &s, (0.1, 0.5), 0.5);
    assert_eq!(layer.images.len(), 2);
    assert_eq!(layer.images[0].image.id, a);
    assert_eq!(layer.images[1].image.id, a);
    assert_eq!(layer.images[0].bounds, Rect::new(76.0, 27.0, 48.0, 48.0));
    assert_eq!(layer.images[0].opacity, 0.5);
    ring.purge();
    assert!(ring.kept().is_none());
    assert_eq!(ring.memory(), 0);
}
