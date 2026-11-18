use super::*;

#[test]
fn names_read_and_written() {
    for kind in [BackdropKind::Simple, BackdropKind::Glass] {
        assert_eq!(BackdropKind::parse(kind.id()), Some(kind));
    }
    assert_eq!(BackdropKind::parse("frosted"), None);
}

#[test]
fn what_a_theme_doesnt_say_comes_from_under() {
    let base = BackdropSpec {
        style: Some(BackdropKind::Glass),
        opacity: Some(0.3),
        ..BackdropSpec::default()
    };
    let top = BackdropSpec { opacity: Some(0.2), ..BackdropSpec::default() };
    let spec = top.over(base);
    assert_eq!(spec.style, Some(BackdropKind::Glass));
    assert_eq!(spec.opacity, Some(0.2));
    assert!(spec.color.is_none() && !spec.is_empty());
    assert!(BackdropSpec::default().is_empty());
}
