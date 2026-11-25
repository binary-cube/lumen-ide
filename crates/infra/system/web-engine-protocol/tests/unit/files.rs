use super::*;
use crate::{PROTOCOL, popup_pixels};

/// A page's pixels' file is named so the host knows it; anything else (a path, another
/// file) isn't one.
#[test]
fn a_pages_pixels_are_named_as_the_host_expects() {
    let name = pixels_name("1f2e");
    assert_eq!(name, "frames-1f2e.bgra");
    assert!(is_pixels_name(&name));
    let popup = popup_pixels(std::path::Path::new(&name));
    assert!(is_pixels_name(&popup.to_string_lossy()));
    for other in
        ["frames-.bgra", "frames-x.png", "x-1.bgra", "frames-../a.bgra", "frames-a\\b.bgra"]
    {
        assert!(!is_pixels_name(other), "{other}");
    }
}

/// A package's description becomes its list entry under the release's address.
#[test]
fn a_description_is_listed_under_its_release() {
    let described = PackageDescription {
        version: "156.0.0.1-1".into(),
        protocol: PROTOCOL,
        platform: "macos-arm64".into(),
        name: "engine.tar.xz".into(),
        size: 9,
        sha256: "ab".into(),
    };
    let entry = described.listed("https://github.com/o/r/releases/download/web-engine/");
    assert_eq!(entry.url, "https://github.com/o/r/releases/download/web-engine/engine.tar.xz");
    assert_eq!((entry.version.as_str(), entry.size), ("156.0.0.1-1", 9));
}

/// The shared files read as their makers write them: a list with no minimum, an engine's
/// own description; a list written leaves out a minimum it hasn't.
#[test]
fn the_shared_files_read_as_written() {
    let engine: EngineInfo =
        serde_json::from_str(r#"{"version":"156.0.0.1-1","protocol":2}"#).unwrap();
    assert_eq!(engine, EngineInfo { version: "156.0.0.1-1".into(), protocol: 2 });
    let list: PackageList = serde_json::from_str(r#"{"packages":[]}"#).unwrap();
    assert_eq!(list, PackageList::default());
    assert_eq!(serde_json::to_string(&list).unwrap(), r#"{"packages":[]}"#);
}
