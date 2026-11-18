use super::*;

#[test]
fn decodes_svg_and_png() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="20"><rect width="10" height="20" fill="red"/></svg>"#;
    let d = decode_svg(svg).unwrap();
    assert_eq!(d.original, (10, 20));
    assert_eq!(d.pixels.len() as u32, d.width * d.height * 4);
    assert_eq!(&d.pixels[..4], &[255, 0, 0, 255]);

    let mut png = Vec::new();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 4]))
        .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let d = decode(&png).unwrap();
    assert_eq!((d.width, d.height, d.format), (3, 2, "PNG"));
    assert_eq!(&d.pixels[..4], &[1, 2, 3, 4]);
    assert!(is_image(Path::new("a/B.JPG")) && !is_image(Path::new("a.php")));
}

#[test]
fn saves_transformed_file() {
    let dir = std::env::temp_dir().join(format!("image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.png");
    image::RgbaImage::from_fn(3, 2, |x, _| image::Rgba([x as u8, 0, 0, 255])).save(&path).unwrap();
    save_transformed(&path, Transform::default().rotate(true)).unwrap();
    let d = load(&path).unwrap();
    assert_eq!((d.width, d.height), (2, 3));
    assert!(!dir.join("a.saving-tmp").exists());
    assert!(save_transformed(&dir.join("x.svg"), Transform::default()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn transforms_compose() {
    let t = Transform::default();
    assert!(t.rotate(true).rotate(false).is_identity());
    assert!(t.flip_horizontal().flip_horizontal().is_identity());
    assert!(t.flip_vertical().flip_vertical().is_identity());
    assert!(t.rotate(true).flip_horizontal().rotate(true).flip_horizontal().is_identity());
    // 2×1: [A B] rotated clockwise → 1×2: [A; B].
    let px = [1, 0, 0, 255, 2, 0, 0, 255];
    let (w, h, out) = t.rotate(true).apply(2, 1, &px);
    assert_eq!((w, h, out[0], out[4]), (1, 2, 1, 2));
    let (_, _, out) = t.flip_horizontal().apply(2, 1, &px);
    assert_eq!((out[0], out[4]), (2, 1));
    // Vertically flipping a column reverses the rows.
    let (_, _, out) = t.flip_vertical().apply(1, 2, &px);
    assert_eq!((out[0], out[4]), (2, 1));
}

#[test]
fn probes_headers_and_decodes_at_the_shown_size() {
    let dir = std::env::temp_dir().join(format!("image-fit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.png");
    image::RgbaImage::from_pixel(1200, 600, image::Rgba([9, 8, 7, 255])).save(&path).unwrap();
    assert_eq!(probe(&path).unwrap(), ((1200, 600), "PNG"));
    let d = load_fitting(&path, 300).unwrap();
    assert_eq!((d.width, d.height, d.original), (300, 150, (1200, 600)));
    assert_eq!(&d.pixels[..4], &[9, 8, 7, 255]);
    let (w, h, small) = shrink(d.width, d.height, &d.pixels, 100);
    assert_eq!((w, h, small.len()), (100, 50, 100 * 50 * 4));
    let (w, h, _) = Transform::default().rotate(true).apply_owned(d.width, d.height, d.pixels);
    assert_eq!((w, h), (150, 300));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A fresh folder of the test's own under the system temp directory.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("image-decode-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An EXIF chunk (little-endian TIFF) holding only an orientation.
fn exif_orientation(value: u16) -> Vec<u8> {
    let mut exif = b"II*\0".to_vec();
    exif.extend(8u32.to_le_bytes());
    exif.extend(1u16.to_le_bytes());
    exif.extend(0x0112u16.to_le_bytes());
    exif.extend(3u16.to_le_bytes());
    exif.extend(1u32.to_le_bytes());
    exif.extend(value.to_le_bytes());
    exif.extend([0, 0]);
    exif.extend(0u32.to_le_bytes());
    exif
}

/// A JPEG 32 × 16 stored, red on the left and blue on the right, tagged as turned a
/// quarter clockwise (EXIF orientation 6): shown 16 × 32, red on top.
fn write_turned_jpeg(path: &Path) {
    use image::ImageEncoder;
    let pixels = image::RgbImage::from_fn(32, 16, |x, _| {
        if x < 16 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) }
    });
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95);
    encoder.set_exif_metadata(exif_orientation(6)).unwrap();
    encoder.write_image(&pixels, 32, 16, image::ExtendedColorType::Rgb8).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The orientation a file says it has.
fn orientation_of(path: &Path) -> image::metadata::Orientation {
    use image::ImageDecoder;
    let reader = image::ImageReader::open(path).unwrap().with_guessed_format().unwrap();
    reader.into_decoder().unwrap().orientation().unwrap()
}

#[test]
fn a_photo_is_shown_as_its_exif_orientation_says() {
    let dir = temp_dir("exif");
    let path = dir.join("turned.jpg");
    write_turned_jpeg(&path);
    assert_eq!(probe(&path).unwrap(), ((16, 32), "JPEG"));
    let d = load(&path).unwrap();
    assert_eq!((d.width, d.height, d.original), (16, 32, (16, 32)));
    let top = &d.pixels[(8 * 4)..(8 * 4 + 3)];
    assert!(top[0] > 200 && top[2] < 60, "red on top: {top:?}");
    let d = load_for_width(&path, 8, 1.0).unwrap();
    assert_eq!((d.width, d.height), (8, 16));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_photo_keeps_what_it_showed_and_no_orientation() {
    let dir = temp_dir("exif-save");
    let path = dir.join("turned.jpg");
    write_turned_jpeg(&path);
    // Shown 16 × 32 (red on top), turned clockwise again: 32 × 16, red on the right.
    save_transformed(&path, Transform::default().rotate(true)).unwrap();
    assert_eq!(orientation_of(&path), image::metadata::Orientation::NoTransforms);
    assert_eq!(probe(&path).unwrap().0, (32, 16));
    let d = load(&path).unwrap();
    let right = &d.pixels[((8 * 32 + 30) * 4)..((8 * 32 + 30) * 4 + 3)];
    assert!(right[0] > 200 && right[2] < 60, "red on the right: {right:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_holding_several_pictures_are_not_saved_as_one() {
    use image::ImageEncoder;
    let dir = temp_dir("several");
    // An animated GIF: refused, left as it was.
    let gif = dir.join("anim.gif");
    {
        let file = std::fs::File::create(&gif).unwrap();
        let mut encoder = image::codecs::gif::GifEncoder::new(file);
        let frame = |c| image::Frame::new(image::RgbaImage::from_pixel(4, 2, image::Rgba(c)));
        encoder.encode_frames([frame([255, 0, 0, 255]), frame([0, 255, 0, 255])]).unwrap();
    }
    let before = std::fs::read(&gif).unwrap();
    let error = save_transformed(&gif, Transform::default().rotate(true)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(std::fs::read(&gif).unwrap(), before);
    // An icon of two sizes: refused.
    let ico = dir.join("two.ico");
    {
        let png = |side: u32| {
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(
                    &vec![200; (side * side * 4) as usize],
                    side,
                    side,
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
            bytes
        };
        let (small, big) = (png(16), png(32));
        let frames = [
            image::codecs::ico::IcoFrame::with_encoded(
                &small,
                16,
                16,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap(),
            image::codecs::ico::IcoFrame::with_encoded(
                &big,
                32,
                32,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap(),
        ];
        let file = std::fs::File::create(&ico).unwrap();
        image::codecs::ico::IcoEncoder::new(file).encode_images(&frames).unwrap();
    }
    let error = save_transformed(&ico, Transform::default().rotate(true)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    // A still GIF is saved as before.
    let still = dir.join("still.gif");
    image::RgbaImage::from_pixel(4, 2, image::Rgba([1, 2, 3, 255])).save(&still).unwrap();
    save_transformed(&still, Transform::default().rotate(true)).unwrap();
    assert_eq!(probe(&still).unwrap().0, (2, 4));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_svg_is_drawn_at_the_side_asked_and_its_texts_are_set() {
    let dir = temp_dir("svg-side");
    let path = dir.join("a.svg");
    std::fs::write(
        &path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="20"><rect width="10" height="20" fill="red"/></svg>"#,
    )
    .unwrap();
    let d = load_svg_at_side(&path, 400).unwrap();
    assert_eq!((d.width, d.height, d.original), (200, 400, (10, 20)));
    assert_eq!(load_svg_at_side(&path, 100_000).unwrap().height, MAX_SIDE);
    // A text: set with the system's fonts (where it has any).
    assert!(has_text(b"<svg><text>a</text></svg>") && has_text(b"<svg:text>"));
    assert!(!has_text(b"<svg><rect/></svg>"));
    let text = br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40"><text x="4" y="30" font-family="sans-serif" font-size="28" fill="black">Hello</text></svg>"#;
    let d = decode_svg_fitting(text, 512).unwrap();
    let inked = d.pixels.chunks(4).filter(|p| p[3] > 0).count();
    if !system_fonts().is_empty() {
        assert!(inked > 50, "the text is drawn: {inked}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
