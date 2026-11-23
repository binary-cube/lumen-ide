use super::*;
use crate::path::rounded;

/// The color the masks give pixel (`x`, `y`) over black (device pixels).
pub(crate) fn color_at(masks: &[Mask], x: u32, y: u32) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (image, r) in masks {
        let (rx, ry) = (r.min_x() as u32, r.min_y() as u32);
        if x < rx || y < ry || x >= rx + image.width || y >= ry + image.height {
            continue;
        }
        let tint = image.tint.expect("a mask");
        image.with_pixels(|cov| {
            let a = f32::from(cov[((y - ry) * image.width + (x - rx)) as usize]) / 255.0;
            for (o, v) in out.iter_mut().zip([tint.r, tint.g, tint.b]) {
                *o = v * a + *o * (1.0 - a);
            }
        });
    }
    out
}

/// Two boxes, the second over the first: where they meet, the second's color; outside
/// both, nothing; a tile past the scene, no masks.
#[test]
fn shapes_painted_in_order() {
    let (red, blue) = (Rgba::rgb(0xff0000), Rgba::rgb(0x0000ff));
    let a = rounded(Rect::new(0.0, 0.0, 60.0, 60.0), 0.0);
    let b = rounded(Rect::new(30.0, 30.0, 60.0, 60.0), 0.0);
    let shapes = [
        Shape { segs: &a, fill: Some(red), line: None },
        Shape { segs: &b, fill: Some(blue), line: None },
    ];
    let masks = tile((100.0, 100.0), shapes, 1.0, 0, 0);
    assert_eq!(color_at(&masks, 10, 10), [1.0, 0.0, 0.0]);
    assert_eq!(color_at(&masks, 45, 45), [0.0, 0.0, 1.0], "the second over the first");
    assert_eq!(color_at(&masks, 95, 5), [0.0, 0.0, 0.0]);
    assert_eq!(grid((100.0, 100.0), 1.0), (1, 1));
    assert_eq!(grid((1000.0, 100.0), 1.0), (2, 1));
    assert!(tile((100.0, 100.0), shapes, 1.0, 1, 0).is_empty(), "past the scene");
    assert!(tile((100.0, 100.0), [], 1.0, 0, 0).is_empty(), "nothing to paint");
}

/// A tile of many colors: one RGBA image (not a megabyte of weights per color), each shape
/// in its own color where it is.
#[test]
fn a_tile_of_many_colors_is_one_image() {
    let boxes: Vec<(Vec<Seg>, Rgba)> = (0..40u32)
        .map(|i| {
            let r = Rect::new(i as f32 * 12.0, 0.0, 10.0, 10.0);
            (rounded(r, 0.0), Rgba::rgb(0x010203 * (i + 1)))
        })
        .collect();
    let shapes = boxes.iter().map(|(segs, fill)| Shape { segs, fill: Some(*fill), line: None });
    let masks = tile((480.0, 10.0), shapes, 1.0, 0, 0);
    assert_eq!(masks.len(), 1);
    let (image, r) = &masks[0];
    assert!(image.tint.is_none(), "RGBA");
    assert_eq!((r.size.width, r.size.height), (480.0, 10.0));
    for (i, (_, fill)) in boxes.iter().enumerate() {
        let x = i as u32 * 12 + 5;
        let at = ((5 * image.width + x) * 4) as usize;
        let px = image.with_pixels(|p| [p[at], p[at + 1], p[at + 2], p[at + 3]]).unwrap();
        assert_eq!(px, fill.to_rgba8(), "box {i}");
    }
}

/// A flat shape (an area at zero, a bar of no height) covers nothing: no path to fill (it
/// would only be refused, a warning in the log each time); a line along it still has one.
#[test]
fn a_flat_shape_covers_nothing() {
    use kernel::Point;
    let flat = [
        Seg::Move(Point::new(0.0, 10.0)),
        Seg::Line(Point::new(50.0, 10.0)),
        Seg::Line(Point::new(100.0, 10.0)),
        Seg::Close,
    ];
    assert!(outline(&flat, None, 1.0, 0.0, 0.0).is_none());
    let line = Line { color: kernel::Rgba::WHITE, width: 2.0, dash: None };
    assert!(outline(&flat, Some(&line), 1.0, 0.0, 0.0).is_some());
}
