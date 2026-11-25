use super::*;

const OFFERS: [Offer<'static>; 5] = [
    Offer { version: "156.0.8078.100-2", protocol: 1, platform: "macos-arm64" },
    Offer { version: "156.0.8078.100-10", protocol: 1, platform: "macos-arm64" },
    Offer { version: "157.0.1.1-1", protocol: 2, platform: "macos-arm64" },
    Offer { version: "157.0.1.1-1", protocol: 1, platform: "linux-x64" },
    Offer { version: "not-a-version", protocol: 1, platform: "macos-arm64" },
];

fn wanted<'a>(refused: &'a [String], current: Option<&'a str>) -> Wanted<'a> {
    Wanted { platform: "macos-arm64", protocol: 1, refused, current }
}

/// The newest for the system and the protocol (numbers as numbers), one that failed here
/// left out; nothing for another system.
#[test]
fn the_newest_that_fits_is_taken() {
    let listed = Listed { offers: &OFFERS, minimum: None };
    assert_eq!(choose(listed, wanted(&[], None)), Choice::Take(1), "10 after 2");
    let refused = ["156.0.8078.100-10".to_owned()];
    assert_eq!(choose(listed, wanted(&refused, None)), Choice::Take(0));
    let other = Wanted { platform: "windows-x64", ..wanted(&[], None) };
    assert_eq!(choose(listed, other), Choice::Nothing);
}

/// An update never goes back: the one installed stays when nothing newer fits (newer than
/// the list's, or the newest refused).
#[test]
fn an_update_never_goes_back() {
    let listed = Listed { offers: &OFFERS, minimum: None };
    let up_to_date = Choice::Current { below_minimum: false };
    assert_eq!(choose(listed, wanted(&[], Some("156.0.8078.100-10"))), up_to_date);
    assert_eq!(choose(listed, wanted(&[], Some("158.0.0.0-1"))), up_to_date, "newer than any");
    let refused = ["156.0.8078.100-10".to_owned()];
    assert_eq!(choose(listed, wanted(&refused, Some("156.0.8078.100-5"))), up_to_date);
    assert_eq!(choose(listed, wanted(&[], Some("156.0.8078.100-5"))), Choice::Take(1));
}

/// Nothing below the minimum is installed; one installed below it is replaced by what is
/// at least the minimum, and said unsafe when nothing can.
#[test]
fn the_minimum_is_enforced() {
    let listed = Listed { offers: &OFFERS, minimum: Some("156.0.8078.100-3") };
    let refused = ["156.0.8078.100-10".to_owned()];
    assert_eq!(choose(listed, wanted(&refused, None)), Choice::Nothing, "only -2: below it");
    assert_eq!(choose(listed, wanted(&[], Some("156.0.8078.100-2"))), Choice::Take(1));
    assert_eq!(
        choose(listed, wanted(&refused, Some("156.0.8078.100-2"))),
        Choice::Current { below_minimum: true }
    );
}

/// A package counts only with a version, a SHA-256, a size and a plain archive name.
#[test]
fn a_package_must_be_well_formed() {
    let sha = "a".repeat(64);
    let url = "https://github.com/o/r/releases/download/web-engine/engine.tar.xz";
    assert!(well_formed("156.0.0.1-1", &sha, 10, url));
    assert!(!well_formed("x", &sha, 10, url));
    assert!(!well_formed("156.0.0.01-1", &sha, 10, url), "not its one spelling");
    assert!(!well_formed("156.0.0.1-1", &"g".repeat(64), 10, url));
    assert!(!well_formed("156.0.0.1-1", &sha, 0, url));
    assert_eq!(archive_name(url), Some("engine.tar.xz"));
    for bad in [
        "https://h/a/",
        "https://h/..",
        "https://h/a\\b.zip",
        "https://h/a.zip?x=1",
        "https://h/.x",
        "https://h/a*.zip",
        "https://h/a<b.zip",
        "https://h/a>b.zip",
        "https://h/a|b.zip",
        "https://h/a\"b.zip",
        "none",
    ] {
        assert_eq!(archive_name(bad), None, "{bad}");
    }
}
