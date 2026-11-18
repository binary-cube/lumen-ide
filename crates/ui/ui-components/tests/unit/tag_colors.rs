use super::*;

/// The contrast between two colors (WCAG's ratio of their relative luminances).
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let channel = |c: f32| {
        if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let lum = |c: Rgba| 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn every_named_color_reads_on_dark_and_light_backgrounds() {
    let dark = Palette::dark();
    let mut light = Palette::dark();
    light.editor_background = Rgba::rgb(0xffffff);
    assert!(is_dark(&dark) && !is_dark(&light));
    for name in NAMES {
        let on_dark = named(name, &dark).unwrap();
        let on_light = named(name, &light).unwrap();
        assert!(contrast(on_dark, dark.editor_background) >= 3.0, "{name} on dark");
        assert!(contrast(on_light, light.editor_background) >= 3.0, "{name} on light");
    }
    assert_eq!(named("crimson", &dark), None);
    assert_eq!(named("#123456", &dark), Some(Rgba::rgb(0x123456)), "one picked, as it is");
}
