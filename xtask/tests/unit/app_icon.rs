use super::*;

#[test]
fn ico_lists_its_images_then_holds_them() {
    let images = vec![(16, vec![1, 2, 3]), (256, vec![4, 5])];
    let file = ico(&images);
    // The header: an icon, two images.
    assert_eq!(&file[..6], &[0, 0, 1, 0, 2, 0]);
    // The first entry: 16 × 16, 32 bits, 3 bytes after the header and both entries.
    assert_eq!(&file[6..10], &[16, 16, 0, 0]);
    assert_eq!(&file[12..14], &32u16.to_le_bytes());
    assert_eq!(&file[14..18], &3u32.to_le_bytes());
    assert_eq!(&file[18..22], &38u32.to_le_bytes());
    // 256 is written as 0; its bytes follow the first image's.
    assert_eq!(&file[22..24], &[0, 0]);
    assert_eq!(&file[34..38], &41u32.to_le_bytes());
    assert_eq!(&file[38..], &[1, 2, 3, 4, 5]);
}

#[test]
fn the_committed_icon_holds_every_windows_size() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let file = std::fs::read(root.join("assets/app-icon.ico")).unwrap();
    let count = usize::from(u16::from_le_bytes([file[4], file[5]]));
    let sizes: Vec<u32> = (0..count)
        .map(|i| match file[6 + 16 * i] {
            0 => 256,
            side => u32::from(side),
        })
        .collect();
    assert_eq!(sizes, ICO_SIZES, "regenerate it: cargo xtask icon");
}

/// An opaque disc filling a transparent square (a stand-in for the master).
fn disc(side: u32) -> Rgba32FImage {
    let c = side as f32 / 2.0;
    ImageBuffer::from_fn(side, side, |x, y| {
        let inside = (x as f32 + 0.5 - c).hypot(y as f32 + 0.5 - c) < c;
        if inside { Rgba([0.5, 0.25, 0.75, 1.0]) } else { Rgba([0.0; 4]) }
    })
}

#[test]
fn every_size_keeps_the_corners_transparent_and_the_centre_opaque() {
    let master = disc(1024);
    for size in [16, 32, 64, 256] {
        for fill in [MAC_FILL, FLAT_FILL] {
            let image = icon(&master, size, fill);
            assert_eq!(image.dimensions(), (size, size));
            assert!(image.get_pixel(0, 0)[3] < 0.01, "{size} {fill}: corner");
            assert!(image.get_pixel(size / 2, size / 2)[3] > 0.99, "{size} {fill}: centre");
            assert!(image.pixels().all(|p| (0..3).all(|c| p[c] <= p[3])), "premultiplied");
        }
    }
}
