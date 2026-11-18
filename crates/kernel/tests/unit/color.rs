use super::*;

#[test]
fn parses_hex_forms() {
    assert_eq!(Rgba::from_hex("#ff0000"), Some(Rgba::rgb(0xff0000)));
    assert_eq!(Rgba::from_hex("#f00"), Some(Rgba::rgb(0xff0000)));
    assert_eq!(Rgba::from_hex("#00000080").map(|c| c.to_rgba8()[3]), Some(128));
    assert_eq!(Rgba::from_hex("ff0000"), None);
    assert_eq!(Rgba::from_hex("#ff00"), None);
    assert_eq!(Rgba::from_hex("#gg0000"), None);
}

#[test]
fn rgb_round_trips_through_rgba8() {
    assert_eq!(Rgba::rgb(0x1e1f22).to_rgba8(), [0x1e, 0x1f, 0x22, 0xff]);
}
