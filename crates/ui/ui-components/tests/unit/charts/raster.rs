use super::*;

/// A slice is a closed path from the middle round its arc, in quarter-turn pieces at most.
#[test]
fn a_slice_is_a_closed_arc() {
    let segs = slice(Point::new(50.0, 50.0), (40.0, 0.0), 0.0, std::f32::consts::PI);
    assert_eq!(segs.first(), Some(&Seg::Move(Point::new(50.0, 50.0))));
    assert_eq!(segs.last(), Some(&Seg::Close));
    let cubics = segs.iter().filter(|s| matches!(s, Seg::Cubic(..))).count();
    assert_eq!(cubics, 2, "half a turn: two quarters");
    let Some(Seg::Cubic(_, _, end)) = segs.iter().rev().nth(1) else { panic!() };
    assert!((end.x - 50.0).abs() < 1e-3 && (end.y - 90.0).abs() < 1e-3, "ends at the bottom");
}

/// A ring's piece: round its outer arc, across to the inner one, back round it the other
/// way (a whole ring keeps its hole), closed.
#[test]
fn a_ring_s_piece_goes_round_and_back() {
    let segs = slice(Point::new(50.0, 50.0), (40.0, 20.0), 0.0, std::f32::consts::PI);
    assert_eq!(segs.first(), Some(&Seg::Move(Point::new(50.0, 10.0))), "the outer arc's top");
    let cubics = segs.iter().filter(|s| matches!(s, Seg::Cubic(..))).count();
    assert_eq!(cubics, 4, "two quarters out, two back");
    assert!(segs.contains(&Seg::Line(Point::new(50.0, 70.0))), "across to the inner arc");
    let Some(Seg::Cubic(_, _, end)) = segs.iter().rev().nth(1) else { panic!() };
    assert!((end.x - 50.0).abs() < 1e-3 && (end.y - 30.0).abs() < 1e-3, "back at the top");
}

/// A set of masks frees its textures as soon as nothing holds it: not while a fade still
/// draws it, at once when the last holder lets it go.
#[test]
fn masks_free_their_textures_once_nobody_holds_them() {
    let image = ImageData::mask(4, 4, vec![255; 16], Rgba::WHITE);
    let piece = Piece { slice: None, image: image.clone(), at: Rect::new(0.0, 0.0, 4.0, 4.0) };
    let masks = Arc::new(Masks(vec![piece]));
    let fading = Arc::clone(&masks);
    drop(masks);
    assert!(!image.released(), "still drawn by the fade");
    drop(fading);
    assert!(image.released());
}
