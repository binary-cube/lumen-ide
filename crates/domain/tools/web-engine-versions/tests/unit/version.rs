use super::*;

/// Numbers compared as numbers, ours after Chromium's; anything else isn't a version.
#[test]
fn versions_compare_part_by_part() {
    let v = |t: &str| Version::parse(t).unwrap();
    assert!(v("156.0.8078.100-2") < v("156.0.8078.100-10"));
    assert!(v("156.0.8078.100-10") < v("157.0.0.0-1"));
    assert_eq!(v("156.0.8078.100-2").to_string(), "156.0.8078.100-2");
    for other in ["156.0.8078-1", "156.0.8078.100", "1.2.3.4.5-1", "a.0.0.0-1", "1.0.0.0-x"] {
        assert!(Version::parse(other).is_none(), "{other}");
    }
}

/// A version has one spelling: a sign, a leading zero, an empty part or a number too big
/// isn't one; every version read writes back as it was read.
#[test]
fn a_version_has_one_spelling() {
    for other in [
        "156.0.0.1-+5",
        "+156.0.0.1-5",
        "156.0.0.01-5",
        "156.0.0.1-05",
        "156..0.1-5",
        "156.0.0.1-",
        "156.0.0.1-99999999999",
        "156.0.0.1- 5",
    ] {
        assert!(Version::parse(other).is_none(), "{other}");
    }
    for text in ["0.0.0.0-0", "156.0.8078.100-10", "4294967295.0.0.0-1"] {
        assert_eq!(Version::parse(text).map(|v| v.to_string()).as_deref(), Some(text));
    }
}
