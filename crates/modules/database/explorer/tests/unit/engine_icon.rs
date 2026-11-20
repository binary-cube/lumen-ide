use super::*;
use std::sync::Arc;

fn none() -> Arc<dyn database::Driver> {
    Arc::new(crate::fake::FakeDriver::new(crate::fake::FakeDb::users(1), true))
}

/// A driver that names its icon is shown with it; one that doesn't, a server's or a file's,
/// with the database's one.
#[test]
fn a_driver_s_icon_is_its_own_else_its_shape_s() {
    let with = database::DriverFactory::new(
        "engine-icon-own",
        "Own",
        database::Kind::Relational,
        Some(1),
        none,
    )
    .with_icon("engine-own", IconTone::Blue);
    database::drivers::register(with);
    assert_eq!(of("engine-icon-own"), ("engine-own", IconTone::Blue));
    assert_eq!(of("engine-icon-unknown"), ("database", IconTone::Neutral));
}

/// The color: the theme's `icon.<name>` first, else the tone's terminal color, else the
/// neutral one given.
#[test]
fn an_icon_s_color_is_the_theme_s_then_its_tone_s() {
    let mut p = Palette::dark();
    let neutral = Rgba::rgb(0x123456);
    assert_eq!(color("engine-x", IconTone::Blue, &p, neutral), p.terminal_ansi[4]);
    assert_eq!(color("engine-x", IconTone::Cyan, &p, neutral), p.terminal_ansi[6]);
    assert_eq!(color("engine-x", IconTone::Neutral, &p, neutral), neutral);
    let own = Rgba::rgb(0xabcdef);
    p.icon_colors.insert("engine-x".into(), own);
    assert_eq!(color("engine-x", IconTone::Blue, &p, neutral), own);
}
