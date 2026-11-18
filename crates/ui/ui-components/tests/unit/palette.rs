use super::*;

#[test]
fn vivid_selections_become_translucent_quiet_ones_stay() {
    let dark = theming::Appearance::Dark;
    let light = theming::Appearance::Light;
    // Midnight's purple on a dark theme: translucent; Default Dark's deep blue: as is.
    assert!(readable_selection(Rgba::rgb(0x7c5cff), dark, 0.34).a < 0.5);
    assert_eq!(readable_selection(Rgba::rgb(0x214283), dark, 0.34).a, 1.0);
    // A saturated blue on a light theme: translucent; a pale one: as is.
    assert!(readable_selection(Rgba::rgb(0x0969da), light, 0.34).a < 0.5);
    assert_eq!(readable_selection(Rgba::rgb(0xdde3ff), light, 0.34).a, 1.0);
    // Already translucent: the theme's choice.
    let chosen = Rgba::rgb(0x7c5cff).with_alpha(0.5);
    assert_eq!(readable_selection(chosen, dark, 0.34), chosen);
}
