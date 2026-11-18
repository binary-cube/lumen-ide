use super::*;

/// Nothing said: a dialog's black 16 %, the drawer's dimming by the window's brightness
/// (about half under a dark theme, a quarter under a light one), not blurred.
#[test]
fn nothing_said_keeps_todays_look() {
    let dark = BackdropStyle::resolve(BackdropSpec::default(), Rgba::rgb(0x151618));
    assert_eq!(dark.tint(BackdropRole::Dialog), Rgba::BLACK.with_alpha(0.16));
    assert!((dark.opacity(BackdropRole::Drawer) - 0.526).abs() < 0.01);
    let light = BackdropStyle::resolve(BackdropSpec::default(), Rgba::rgb(0xf2f2f4));
    assert!((light.opacity(BackdropRole::Drawer) - 0.27).abs() < 0.01);
    assert_eq!(dark.kind, BackdropKind::Simple);
    assert_eq!(dark.blur_radius(2.0), None);
}

/// A theme's color and opacity go under every window; its glass blurs by its size.
#[test]
fn the_theme_decides_and_the_user_goes_over_it() {
    let theme = BackdropSpec {
        style: Some(BackdropKind::Glass),
        color: Some(Rgba::rgb(0x003300).with_alpha(0.5)),
        opacity: Some(0.3),
        blur: Some(3.0),
    };
    let style = BackdropStyle::resolve(theme, Rgba::rgb(0x010501));
    assert_eq!(style.tint(BackdropRole::Dialog), Rgba::rgb(0x003300).with_alpha(0.3));
    assert_eq!(style.opacity(BackdropRole::Drawer), 0.3);
    assert_eq!(style.blur_radius(2.0), Some(6.0));
    // No blur at all: as simple.
    let none = style.with_user(BackdropSpec { blur: Some(0.0), ..BackdropSpec::default() });
    assert_eq!(none.blur_radius(2.0), None);
    // The user's own: simple, the theme's color kept.
    let user = BackdropSpec { style: Some(BackdropKind::Simple), ..BackdropSpec::default() };
    let mine = style.with_user(user);
    assert_eq!(mine.kind, BackdropKind::Simple);
    assert_eq!(mine.color, Rgba::rgb(0x003300));
    assert_eq!(mine.theme_spec(), theme);
    // Back to the theme's.
    assert_eq!(mine.with_user(BackdropSpec::default()), style);
    // A renderer that can't blur: simple.
    assert_eq!(style.without_blur().blur_radius(1.0), None);
    // Too strong a color is capped.
    let wild = style.with_user(BackdropSpec { opacity: Some(2.0), ..BackdropSpec::default() });
    assert_eq!(wild.opacity(BackdropRole::Dialog), MAX_OPACITY);
}
