use super::{Size, sane, utf16_of, whole_pixels};

/// What an app asks is brought within a page's bounds.
#[test]
fn page_sizes_are_kept_sane() {
    let asked = Size { width: 0, height: 1_000_000, scale: f32::INFINITY };
    assert_eq!(sane(asked), Size { width: 1, height: 16_384, scale: 1.0 });
    assert_eq!(sane(Size { width: 800, height: 600, scale: 40.0 }).scale, 8.0);
}

/// A trackpad's fractions add up to whole pixels, none lost.
#[test]
fn wheel_fractions_add_up() {
    let (whole, rest) = whole_pixels((0.0, 0.0), (0.4, -0.6));
    assert_eq!(whole, (0, 0));
    let (whole, rest) = whole_pixels(rest, (0.7, -0.6));
    assert_eq!(whole, (1, -1));
    assert!((rest.0 - 0.1).abs() < 1e-5 && (rest.1 + 0.2).abs() < 1e-5);
}

/// A wheel's delta that isn't a number (or past any) moves nothing and leaves nothing: what
/// comes after it still scrolls.
#[test]
fn a_wheel_delta_not_a_number_is_let_go() {
    let (whole, rest) = whole_pixels((0.5, 0.5), (f32::NAN, f32::INFINITY));
    assert_eq!((whole, rest), ((0, 0), (0.0, 0.0)));
    let (whole, _) = whole_pixels(rest, (2.0, -3.0));
    assert_eq!(whole, (2, -3));
}

/// A character as UTF-16: one unit, two past the BMP (an emoji), one zero for what isn't one.
#[test]
fn characters_past_the_bmp_go_as_two_units() {
    assert_eq!(utf16_of('a' as u32), ([0x61, 0], 1));
    assert_eq!(utf16_of('😀' as u32), ([0xD83D, 0xDE00], 2));
    assert_eq!(utf16_of(0xD800), ([0, 0], 1), "a lone surrogate");
}
