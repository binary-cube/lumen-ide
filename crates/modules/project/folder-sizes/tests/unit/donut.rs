use super::*;

/// The slice under a point: from the top, clockwise; the hole and outside, none.
#[test]
fn a_slice_is_found_where_it_is_drawn() {
    let shares = [1.0, 1.0, 2.0];
    assert_eq!(hit(&shares, 10.0, -80.0, 100.0), Some(0), "just right of the top");
    assert_eq!(hit(&shares, 80.0, 10.0, 100.0), Some(1), "right, a little down");
    assert_eq!(hit(&shares, -80.0, 0.0, 100.0), Some(2), "left");
    assert_eq!(hit(&shares, 0.0, 0.0, 100.0), None, "the hole");
    assert_eq!(hit(&shares, 0.0, 120.0, 100.0), None, "outside");
}

/// Drawn: its pixels in the ring colored, the hole and the corners clear.
#[test]
fn the_ring_is_drawn_in_its_colors() {
    let ring = Ring { shares: vec![1.0], colors: vec![Rgba::rgb(0xff0000)], lit: None, size: 40 };
    let image = ring.image();
    let alpha = |x: u32, y: u32| image.with_pixels(|p| p[((y * 40 + x) * 4 + 3) as usize]).unwrap();
    assert_eq!(alpha(20, 20), 0, "the hole");
    assert_eq!(alpha(0, 0), 0, "a corner");
    assert_eq!(alpha(20, 2), 255, "on the ring");
}
