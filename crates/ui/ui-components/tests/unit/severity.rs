//! The severities' colors read against both themes' backgrounds (icons at least 3:1), they
//! differ from one another, each has its own icon; the heat ramp grows in one direction.

use super::*;

fn light() -> Palette {
    Palette { editor_background: Rgba::WHITE, ..Palette::dark() }
}

#[test]
fn each_severity_reads_on_both_backgrounds() {
    for palette in [Palette::dark(), light()] {
        for level in Level::ALL {
            let c = contrast(level.color(&palette), palette.editor_background);
            assert!(c >= 3.0, "{level:?} on {:?}: {c}", palette.is_dark());
        }
        let colors: Vec<Rgba> = Level::ALL.iter().map(|l| l.color(&palette)).collect();
        assert_ne!(colors[0], colors[1]);
        assert_ne!(colors[1], colors[2]);
    }
    let icons: Vec<&str> = Level::ALL.iter().map(|l| l.icon()).collect();
    assert_eq!(icons, ["info", "alert", "octagon-alert"]);
    assert!(Level::Critical > Level::Warning && Level::Warning > Level::Info);
}

#[test]
fn the_heat_ramp_grows_in_one_direction() {
    let dark = Palette::dark();
    let lums: Vec<f32> = (0..=10).map(|i| luminance(dark.heat(i as f32 / 10.0))).collect();
    assert!(lums.windows(2).all(|w| w[1] > w[0]), "brighter on dark: {lums:?}");
    let light = light();
    let lums: Vec<f32> = (0..=10).map(|i| luminance(light.heat(i as f32 / 10.0))).collect();
    assert!(lums.windows(2).all(|w| w[1] < w[0]), "deeper on light: {lums:?}");
    // Out of range and not a number: clamped.
    assert_eq!(dark.heat(2.0), dark.heat(1.0));
    assert_eq!(dark.heat(f32::NAN), dark.heat(0.0));
    // The hottest stands out from the background.
    assert!(contrast(dark.heat(1.0), dark.editor_background) >= 3.0);
    assert!(contrast(light.heat(1.0), light.editor_background) >= 3.0);
}

#[test]
fn a_color_is_moved_just_enough_to_read() {
    let dark = Palette::dark();
    let bg = dark.editor_background;
    let faint = bg.mix(dark.text, 0.3);
    let moved = readable(faint, dark.text, bg, 4.5);
    assert!(contrast(moved, bg) >= 4.5);
    assert!(contrast(moved, bg) < contrast(dark.text, bg), "not all the way");
    assert_eq!(readable(dark.text, dark.text_muted, bg, 4.5), dark.text, "already reads: as it is");
}
