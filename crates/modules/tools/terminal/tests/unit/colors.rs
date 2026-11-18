use super::*;

#[test]
fn the_theme_s_colors_by_default_the_classic_ones_on_its_background() {
    let palette = Palette::dark();
    let theme = TermColors::new(&palette, Colors::Theme);
    assert_eq!(theme.ansi, palette.terminal_ansi);
    let classic = TermColors::new(&palette, Colors::Classic);
    assert_eq!(classic.ansi[1], Rgba::rgb(0xcd0000), "xterm's red");
    assert_eq!(classic.background, palette.terminal_background, "the theme's background");
}

/// High contrast: every color reads well on the background (4.5:1), in a dark theme and in
/// a light one; the text is white / black.
#[test]
fn high_contrast_makes_every_color_readable() {
    let light = Palette { terminal_background: Rgba::rgb(0xfafafa), ..Palette::dark() };
    for palette in [Palette::dark(), light] {
        let colors = TermColors::new(&palette, Colors::HighContrast);
        for (i, c) in colors.ansi.iter().enumerate() {
            assert!(
                contrast(*c, colors.background) >= 4.4,
                "color {i}: {}",
                contrast(*c, colors.background)
            );
        }
        assert!(contrast(colors.foreground, colors.background) >= 7.0);
    }
}
