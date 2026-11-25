use super::*;

fn package(platform: &str, protocol: u32, version: &str) -> Package {
    Package {
        version: version.into(),
        protocol,
        platform: platform.into(),
        url: format!("https://example.test/web-engine-{platform}-{version}.tar.xz"),
        size: 5,
        sha256: "ab".into(),
    }
}

/// The newest of each system for each protocol stays: a protocol's bump keeps the older
/// protocol's newest package for the apps that still speak it.
#[test]
fn the_merge_keeps_the_newest_of_each_protocol() {
    let previous = [
        package("macos-arm64", 1, "156.0.8078.100-1"),
        package("macos-arm64", 1, "156.0.8078.100-2"),
        package("linux-x64", 1, "156.0.8078.100-2"),
    ];
    let new =
        [package("macos-arm64", 2, "157.0.1.1-1"), package("linux-x64", 1, "156.0.8078.100-3")];
    let merged = merged(&previous, &new);
    let kept: Vec<(&str, u32, &str)> =
        merged.iter().map(|p| (p.platform.as_str(), p.protocol, p.version.as_str())).collect();
    assert_eq!(
        kept,
        [
            ("linux-x64", 1, "156.0.8078.100-3"),
            ("macos-arm64", 1, "156.0.8078.100-2"),
            ("macos-arm64", 2, "157.0.1.1-1"),
        ]
    );
}

/// A version compares by its numbers (`-10` after `-9`), and a new package of the same
/// version replaces the previous one (a rebuilt draft).
#[test]
fn the_merge_compares_versions_by_number() {
    let previous = [package("windows-x64", 1, "156.0.8078.100-10")];
    let merged_older = merged(&previous, &[package("windows-x64", 1, "156.0.8078.100-9")]);
    assert_eq!(merged_older[0].version, "156.0.8078.100-10");
    let mut rebuilt = package("windows-x64", 1, "156.0.8078.100-10");
    rebuilt.sha256 = "cd".into();
    assert_eq!(merged(&previous, std::slice::from_ref(&rebuilt))[0].sha256, "cd");
}

/// The list's text: each archive at the release's address, the minimum only when there is
/// one, a newline at the end (signed as written).
#[test]
fn the_list_names_each_package_at_the_release() {
    let described = Description {
        version: "156.0.8078.100-1".into(),
        protocol: 1,
        platform: "macos-arm64".into(),
        name: "web-engine-macos-arm64-156.0.8078.100-1.tar.xz".into(),
        size: 5,
        sha256: "ab".repeat(32),
    };
    let listed =
        listed(&described, "https://github.com/o/r/releases/download/web-engine/").unwrap();
    assert_eq!(
        listed.url,
        "https://github.com/o/r/releases/download/web-engine/web-engine-macos-arm64-156.0.8078.100-1.tar.xz"
    );
    let without = text(&List { minimum: None, packages: vec![listed.clone()] }).unwrap();
    assert!(without.ends_with('\n'));
    assert!(!without.contains("minimum"));
    let with = text(&List { minimum: Some("156.0.8078.100-1".into()), packages: vec![listed] });
    let doc: serde_json::Value = serde_json::from_str(&with.unwrap()).unwrap();
    assert_eq!(doc["minimum"], "156.0.8078.100-1");
    assert_eq!(doc["packages"][0]["platform"], "macos-arm64");
}

/// A description the app would pass over is refused, never listed: no SHA-256, an empty
/// archive, a name that isn't plain, a system no engine is made for.
#[test]
fn a_package_the_app_would_refuse_is_not_listed() {
    let good = Description {
        version: "156.0.8078.100-1".into(),
        protocol: 1,
        platform: "linux-x64".into(),
        name: "web-engine-linux-x64-156.0.8078.100-1.tar.xz".into(),
        size: 5,
        sha256: "ab".repeat(32),
    };
    let base = "https://example.test/web-engine";
    assert!(listed(&good, base).is_ok());
    let refused = [
        Description { sha256: "ab".into(), ..good.clone() },
        Description { size: 0, ..good.clone() },
        Description { name: "../x.tar.xz".into(), ..good.clone() },
        Description { name: "sub/x.tar.xz".into(), ..good.clone() },
        Description { version: "156".into(), ..good.clone() },
        Description { platform: "linux-riscv".into(), ..good },
    ];
    for description in refused {
        assert!(listed(&description, base).is_err(), "{description:?}");
    }
}

/// A release is complete only with every system's package, each archive as the list says.
#[test]
fn a_release_needs_every_system_and_its_archives() {
    let dir = std::env::temp_dir().join(format!("web-engine-list-{:032x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let version = "156.0.8078.100-1";
    let mut packages = Vec::new();
    for platform in platforms() {
        let mut listed = package(platform, 1, version);
        let bytes = format!("archive of {platform}");
        let name = format!("web-engine-{platform}-{version}.tar.xz");
        std::fs::write(dir.join(&name), &bytes).unwrap();
        listed.size = bytes.len() as u64;
        listed.sha256 = pinned_downloads::digest::sha256_hex(bytes.as_bytes());
        packages.push(listed);
    }
    // An older package of another protocol: listed, not part of this release.
    packages.push(package("linux-x64", 0, "155.0.1.1-1"));
    let list = List { minimum: None, packages };
    assert_eq!(check_release(&list, &dir, version, &platforms()).unwrap().len(), 4);

    let missing = List { packages: list.packages[1..].to_vec(), ..list.clone() };
    assert!(
        check_release(&missing, &dir, version, &platforms()).unwrap_err().contains("linux-x64")
    );
    std::fs::write(dir.join(format!("web-engine-windows-x64-{version}.tar.xz")), "changed!")
        .unwrap();
    assert!(check_release(&list, &dir, version, &platforms()).is_err(), "an archive changed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Versions as the app reads them.
#[test]
fn versions_are_read_as_the_app_reads_them() {
    assert!(Version::parse("156.0.8078.100-2") < Version::parse("156.0.8078.100-10"));
    assert!(Version::parse("156.0.8078-2").is_none());
    assert!(Version::parse("156.0.8078.100").is_none());
}
