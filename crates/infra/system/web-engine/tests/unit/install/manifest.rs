use ring::signature::{Ed25519KeyPair, KeyPair};
use web_engine_versions::Wanted;

use super::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A list signed by a key made here: its bytes, signature and public key.
pub(crate) fn signed(text: &str) -> (Vec<u8>, String, String) {
    let key = Ed25519KeyPair::from_seed_unchecked(&[3u8; 32]).unwrap();
    let signature = hex(key.sign(text.as_bytes()).as_ref());
    (text.as_bytes().to_vec(), signature, hex(key.public_key().as_ref()))
}

const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn package(version: &str, protocol: u32, platform: &str) -> String {
    format!(
        r#"{{"version":"{version}","protocol":{protocol},"platform":"{platform}","url":"https://github.com/o/r/releases/download/web-engine/e-{version}.tar.xz","size":10,"sha256":"{SHA}"}}"#
    )
}

fn list() -> String {
    format!(
        r#"{{"minimum":"156.0.8078.100-2","packages":[{},{},{},{}]}}"#,
        package("156.0.8078.100-2", 1, "macos-arm64"),
        package("156.0.8078.100-10", 1, "macos-arm64"),
        package("157.0.1.1-1", 2, "macos-arm64"),
        package("157.0.1.1-1", 1, "linux-x64"),
    )
}

fn wanted<'a>(refused: &'a [String], current: Option<&'a str>) -> Wanted<'a> {
    Wanted { platform: "macos-arm64", protocol: 1, refused, current }
}

/// Trusted only when the project signed it; the newest for the system and the protocol is
/// picked (numbers compared as numbers), one that failed here left out.
#[test]
fn a_signed_list_gives_the_newest_package_that_fits() {
    let (bytes, signature, public) = signed(&list());
    let manifest = Manifest::read(&bytes, &signature, &public).unwrap();
    let Pick::Install(chosen) = manifest.pick(wanted(&[], None)) else { panic!("one fits") };
    assert_eq!(chosen.version, "156.0.8078.100-10", "10 after 2, the other protocol left out");
    let refused = ["156.0.8078.100-10".to_owned()];
    let Pick::Install(chosen) = manifest.pick(wanted(&refused, None)) else { panic!("one fits") };
    assert_eq!(chosen.version, "156.0.8078.100-2");
    let other = Wanted { platform: "windows-x64", ..wanted(&[], None) };
    assert_eq!(manifest.pick(other), Pick::Nothing);
    // Another byte, another key: refused.
    let mut changed = bytes.clone();
    changed[3] ^= 1;
    assert_eq!(Manifest::read(&changed, &signature, &public), Err(ManifestError::Unsigned));
    let other = hex(Ed25519KeyPair::from_seed_unchecked(&[4u8; 32]).unwrap().public_key().as_ref());
    assert_eq!(Manifest::read(&bytes, &signature, &other), Err(ManifestError::Unsigned));
}

/// Signed but wrong (a version, a checksum, an archive's name that could leave its folder,
/// a minimum that isn't a version): refused whole.
#[test]
fn a_signed_list_that_isnt_well_formed_is_refused() {
    let bad_url = package("156.0.0.1-1", 1, "p").replace("e-156.0.0.1-1.tar.xz", "..");
    let lists = [
        r#"{"packages":[{"version":"x","protocol":1,"platform":"p","url":"https://github.com/a/b","size":1,"sha256":"00"}]}"#.to_owned(),
        format!(r#"{{"packages":[{bad_url}]}}"#),
        format!(r#"{{"minimum":"soon","packages":[{}]}}"#, package("156.0.0.1-1", 1, "p")),
    ];
    for list in lists {
        let (bytes, signature, public) = signed(&list);
        let read = Manifest::read(&bytes, &signature, &public);
        assert!(matches!(read, Err(ManifestError::Malformed(_))), "{list}");
    }
}

/// The minimum: nothing below it installed; one installed below it replaced; an update
/// never goes back to an older version.
#[test]
fn the_minimum_is_enforced_and_nothing_goes_back() {
    let (bytes, signature, public) = signed(&list());
    let manifest = Manifest::read(&bytes, &signature, &public).unwrap();
    let refused = ["156.0.8078.100-10".to_owned()];
    let Pick::Install(replacing) = manifest.pick(wanted(&[], Some("156.0.8078.100-1"))) else {
        panic!("below the minimum: replaced")
    };
    assert_eq!(replacing.version, "156.0.8078.100-10");
    let newer = manifest.pick(wanted(&refused, Some("156.0.8078.100-5")));
    assert_eq!(newer, Pick::Current { below_minimum: false }, "never back to -2");
    let Pick::Install(safe) = manifest.pick(wanted(&refused, Some("156.0.8078.100-1"))) else {
        panic!("the safe one fits")
    };
    assert_eq!(safe.version, "156.0.8078.100-2");
    let none_safe = ["156.0.8078.100-10".to_owned(), "156.0.8078.100-2".to_owned()];
    let kept = manifest.pick(wanted(&none_safe, Some("156.0.8078.100-1")));
    assert_eq!(kept, Pick::Current { below_minimum: true }, "kept, said unsafe");
}
