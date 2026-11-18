use super::*;

/// Halved: each pixel the average of its 2 × 2 block; BGRA read as RGBA; opaque.
#[test]
fn a_frame_is_shrunk_by_whole_blocks() {
    // 4 × 2, BGRA, a row padded to 20 bytes: left half blue-ish, right half red-ish.
    let px = |b: u8, g: u8, r: u8| [b, g, r, 0];
    let mut data = Vec::new();
    for _ in 0..2 {
        for p in [px(200, 0, 0), px(100, 0, 0), px(0, 0, 200), px(0, 0, 100)] {
            data.extend_from_slice(&p);
        }
        data.extend_from_slice(&[0; 4]);
    }
    let shrunk = shrink(&data, (4, 2, 20), true, 2);
    assert_eq!((shrunk.width, shrunk.height), (2, 1));
    assert_eq!(shrunk.rgba, [0, 0, 150, 255, 150, 0, 0, 255]);
    let same = shrink(&data, (4, 2, 20), true, 10);
    assert_eq!((same.width, same.height), (4, 2), "small enough: as it is");
}
