use std::path::{Path, PathBuf};

use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lsp-bundle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_folder_is_looked_for_beside_the_app_in_its_bundle_and_in_target() {
    let found = candidates(Path::new("/opt/editor/editor"));
    assert_eq!(found[0], Path::new("/opt/editor/lsp"));
    let mac = candidates(Path::new("/Applications/E.app/Contents/MacOS/e"));
    assert!(mac.contains(&PathBuf::from("/Applications/E.app/Contents/Resources/lsp")));
    let dev = candidates(Path::new("/w/target/debug/e"));
    assert!(dev.contains(&PathBuf::from("/w/target/lsp")));
    let cross = candidates(Path::new("/w/target/x86_64-pc-windows-gnu/release/e.exe"));
    assert!(cross.contains(&PathBuf::from("/w/target/lsp")));
    // A test's binary doesn't find it by chance.
    let test = candidates(Path::new("/w/target/debug/deps/app-123"));
    assert!(!test.contains(&PathBuf::from("/w/target/lsp")));
}

#[test]
fn a_manifest_says_where_node_and_the_servers_are() {
    let dir = temp_dir("manifest");
    std::fs::create_dir_all(dir.join("node/22.0.0/bin")).unwrap();
    std::fs::write(dir.join("node/22.0.0/bin/node"), "").unwrap();
    std::fs::create_dir_all(dir.join("servers/vtsls/0.2.9")).unwrap();
    std::fs::write(
        dir.join(MANIFEST),
        r#"platform = "macos-arm64"
[node]
version = "22.0.0"
path = "node/22.0.0/bin/node"
license = "MIT"
[servers.vtsls]
version = "0.2.9"
path = "servers/vtsls/0.2.9"
[servers.gone]
version = "1"
path = "servers/gone/1"
"#,
    )
    .unwrap();
    let bundle = Bundle::open(&dir).unwrap();
    assert_eq!(bundle.manifest.platform, "macos-arm64");
    assert_eq!(bundle.node(), Some(dir.join("node/22.0.0/bin/node")));
    assert_eq!(bundle.server("vtsls"), Some((dir.join("servers/vtsls/0.2.9"), "0.2.9")));
    // Listed, not there.
    assert_eq!(bundle.server("gone"), None);
    assert_eq!(bundle.server("other"), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_manifest_no_bundle() {
    let dir = temp_dir("empty");
    assert_eq!(Bundle::open(&dir), None);
    std::fs::write(dir.join(MANIFEST), "not = [toml").unwrap();
    assert_eq!(Bundle::open(&dir), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_platform_is_named() {
    assert!(
        ["macos-arm64", "macos-x64", "windows-x64", "windows-arm64", "linux-x64", "linux-arm64"]
            .contains(&platform())
    );
}
