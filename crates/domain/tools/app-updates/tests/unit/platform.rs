use super::*;

/// Each system's builds, best first; a system without releases has none.
#[test]
fn each_system_takes_its_builds() {
    assert_eq!(preferred("macos", "aarch64"), ["macos-arm64", "macos-universal"]);
    assert_eq!(preferred("macos", "x86_64"), ["macos-x86_64", "macos-universal"]);
    assert_eq!(preferred("windows", "x86_64"), ["windows-x64"]);
    assert_eq!(preferred("linux", "x86_64"), ["linux-x86_64"]);
    assert!(preferred("linux", "aarch64").is_empty() && preferred("freebsd", "x86_64").is_empty());
}
