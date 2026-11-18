use super::*;

#[test]
fn limits_are_kept_and_names_read() {
    let wild = CaretSpec {
        width: Some(9.0),
        glow: Some(3.0),
        trail_ms: Some(9000),
        ..CaretSpec::default()
    };
    let look = wild.look();
    assert_eq!((look.width, look.glow, look.trail_ms), (4.0, 1.0, 1000));
    assert_eq!(CaretShape::parse("underline"), Some(CaretShape::Underline));
    assert_eq!(Blink::parse("smooth"), Some(Blink::Smooth));
    assert_eq!(Motion::parse("glide"), Some(Motion::Glide));
    assert_eq!(Blink::parse("sometimes"), None);
}
