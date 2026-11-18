use std::path::Path;

use super::*;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[test]
fn the_downloads_manifest_is_up_to_date() {
    // Made again from packaging/lsp.toml and the lockfiles, the sizes from the manifest
    // itself (no network): the same text.
    generate(root(), true).unwrap();
}

#[test]
fn the_manifest_has_every_component_pinned() {
    let text = std::fs::read_to_string(root().join(OUT)).unwrap();
    let manifest: Table = toml::from_str(&text).unwrap();
    let components = manifest["component"].as_array().unwrap();
    let names: Vec<&str> = components.iter().map(|c| c["name"].as_str().unwrap()).collect();
    for name in
        ["node", "php", "vtsls", "vscode-langservers-extracted", "clangd", "taplo", "phpactor"]
    {
        assert!(names.contains(&name), "{name}: {names:?}");
    }
    let mut urls = Vec::new();
    collect_urls(&manifest["component"], &mut urls);
    assert!(urls.len() > 100, "{}", urls.len());
    for c in components {
        let c = c.as_table().unwrap();
        for p in c.get("packages").and_then(Value::as_array).into_iter().flatten() {
            assert!(p["integrity"].as_str().unwrap().starts_with("sha512-"));
            assert!(p["path"].as_str().unwrap().starts_with("node_modules/"));
            assert!(p["size"].as_integer().unwrap() > 0);
        }
        for (_, a) in c.get("platforms").and_then(Value::as_table).into_iter().flatten() {
            assert_eq!(a["sha256"].as_str().unwrap().len(), 64);
            assert!(a["size"].as_integer().unwrap() > 0);
        }
    }
    // Node: only its program and licence, under its archive's top folder.
    let node = components.iter().find(|c| c["name"].as_str() == Some("node")).unwrap();
    let mac = &node["platforms"]["macos-arm64"];
    assert_eq!(mac["strip"].as_str(), Some("node-v26.13.0-darwin-arm64"));
    assert_eq!(mac["bin"].as_str(), Some("bin/node"));
    assert_eq!(node["platforms"]["windows-x64"]["bin"].as_str(), Some("node.exe"));
}

#[test]
fn a_lockfile_gives_its_packages_without_the_development_ones() {
    let dir = std::env::temp_dir().join(format!("lsp-manifest-lock-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("pkg")).unwrap();
    std::fs::write(
        dir.join("pkg/package-lock.json"),
        r#"{"lockfileVersion": 3, "packages": {
            "": {"name": "root"},
            "node_modules/a": {"version": "1.0.0", "resolved": "https://registry.npmjs.org/a/-/a-1.0.0.tgz", "integrity": "sha512-AAAA"},
            "node_modules/a/node_modules/b": {"version": "2.0.0", "resolved": "https://registry.npmjs.org/b/-/b-2.0.0.tgz", "integrity": "sha512-BBBB", "os": ["darwin"]},
            "node_modules/dev": {"version": "1.0.0", "resolved": "https://registry.npmjs.org/dev/-/dev-1.0.0.tgz", "integrity": "sha512-CCCC", "dev": true}
        }}"#,
    )
    .unwrap();
    let packages = lockfile_packages(&dir, "test", "pkg").unwrap();
    let places: Vec<&str> = packages.iter().map(|p| p["path"].as_str().unwrap()).collect();
    assert_eq!(places, ["node_modules/a", "node_modules/a/node_modules/b"]);
    assert_eq!(packages[1]["os"].as_array().unwrap()[0].as_str(), Some("darwin"));
    // A package from elsewhere refuses the lockfile.
    std::fs::write(
        dir.join("pkg/package-lock.json"),
        r#"{"lockfileVersion": 3, "packages": {"node_modules/x": {"resolved": "https://example.com/x.tgz", "integrity": "sha512-A"}}}"#,
    )
    .unwrap();
    assert!(lockfile_packages(&dir, "test", "pkg").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
