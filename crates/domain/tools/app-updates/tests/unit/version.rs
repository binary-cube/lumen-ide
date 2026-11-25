use super::*;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap_or_else(|| panic!("{text} is a version"))
}

/// Read as SemVer: a leading `v`, pre-release identifiers, build metadata left out; what
/// isn't a version refused.
#[test]
fn versions_are_read_as_semver_reads_them() {
    assert_eq!(v("v1.2.3"), Version { major: 1, minor: 2, patch: 3, pre: Vec::new() });
    assert_eq!(v("1.0.0-beta.1").pre, ["beta", "1"]);
    assert_eq!(v("1.0.0-beta.1+build.7"), v("1.0.0-beta.1"));
    assert_eq!(v("1.0.0-beta.1").to_string(), "1.0.0-beta.1");
    for bad in ["", "1", "1.2", "1.2.3.4", "01.2.3", "1.2.-3", "1.2.3-", "1.2.3-a..b", "x.y.z"] {
        assert_eq!(Version::parse(bad), None, "{bad:?}");
    }
    assert!(v("1.0.0-rc.1").is_pre() && !v("1.0.0").is_pre());
}

/// Ordered as SemVer orders them: the numbers, then a pre-release before its release,
/// numeric identifiers as numbers and below words, fewer identifiers first.
#[test]
fn versions_are_ordered_as_semver_orders_them() {
    let ordered = [
        "0.9.9",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.2.0",
        "10.0.0",
    ];
    for pair in ordered.windows(2) {
        assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
    }
    assert_eq!(v("1.0.0+a").cmp(&v("1.0.0+b")), std::cmp::Ordering::Equal);
}

/// The web engine's releases (`web-engine`, `web-engine-v<version>`) are never the app's.
#[test]
fn the_web_engines_tags_are_no_version() {
    assert!(Version::parse("web-engine").is_none());
    assert!(Version::parse("web-engine-v156.0.8078.100-1").is_none());
}
