use super::*;

const SUM: &str = "6a55fc465f4f8f058d1f413e5002cdc9dde0a2849b8d93267a02cbb56b2cd4f4";

fn text() -> String {
    format!(
        r#"{{"version": "1.0.0-beta.2", "date": "2026-11-20", "files": [
            {{"platform": "macos-universal", "name": "lumen-1.0.0-beta.2-macos-universal.zip", "size": 10, "sha256": "{SUM}"}},
            {{"platform": "windows-x64", "name": "lumen-1.0.0-beta.2-windows-x64.zip", "size": 20, "sha256": "{}"}},
            {{"platform": "linux-x86_64", "name": "../escape.tar.gz", "size": 1, "sha256": "{SUM}"}},
            {{"platform": "macos-arm64", "name": "bad-sum.zip", "size": 1, "sha256": "xyz"}},
            {{"platform": "macos-x86_64", "name": "C:drive.zip", "size": 1, "sha256": "{SUM}"}}
        ]}}"#,
        SUM.to_uppercase()
    )
}

/// The manifest read: a file with a path in its name, or a sum that isn't one, left out;
/// sums kept lowercase.
#[test]
fn the_manifest_is_read_and_what_cant_be_trusted_left_out() {
    let manifest = Manifest::parse(&text()).unwrap();
    assert_eq!(manifest.version.to_string(), "1.0.0-beta.2");
    assert_eq!(manifest.date, "2026-11-20");
    let platforms: Vec<&str> = manifest.files.iter().map(|f| f.platform.as_str()).collect();
    assert_eq!(platforms, ["macos-universal", "windows-x64"]);
    assert_eq!(manifest.files[1].sha256, SUM);
    assert_eq!(Manifest::parse("not json"), None);
    assert_eq!(Manifest::parse(r#"{"version": "x", "date": "", "files": []}"#), None);
}

/// This system's file: the first platform of its list the manifest has.
#[test]
fn the_file_for_this_system_is_the_best_one_there() {
    let manifest = Manifest::parse(&text()).unwrap();
    let mac = manifest.file_for(&["macos-arm64", "macos-universal"]).unwrap();
    assert_eq!(mac.platform, "macos-universal", "no file of its own: the universal one");
    assert_eq!(manifest.file_for(&["linux-x86_64"]), None, "its file was refused");
}
