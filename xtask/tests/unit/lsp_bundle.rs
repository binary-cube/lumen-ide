use std::path::Path;

use super::*;

#[test]
fn the_host_is_a_known_platform() {
    assert!(PLATFORMS.contains(&host()));
}

#[test]
fn an_unknown_platform_is_refused() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = std::env::temp_dir().join(format!("lsp-bundle-test-{}", std::process::id()));
    let error = bundle(root, Some("amiga"), Some(&out)).unwrap_err();
    assert!(error.contains("unknown platform"), "{error}");
}

#[test]
fn the_manifest_pins_every_platform_from_the_official_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("packaging/lsp.toml")).unwrap()).unwrap();
    let node = manifest["node"].as_table().unwrap();
    for platform in PLATFORMS {
        let entry = node["platforms"][platform].as_table().unwrap();
        assert!(entry["url"].as_str().unwrap().starts_with("https://nodejs.org/dist/"));
        assert_eq!(entry["sha256"].as_str().unwrap().len(), 64);
    }
    // Every native server is pinned for every platform, from its GitHub releases; every
    // other one's packages are locked, from npm's registry only.
    for server in manifest["servers"].as_array().unwrap() {
        if let Some(platforms) = server.get("platforms").and_then(toml::Value::as_table) {
            for platform in PLATFORMS {
                let entry = platforms[platform].as_table().unwrap();
                assert!(github_release(entry["url"].as_str().unwrap()), "{platform}");
                assert_eq!(entry["sha256"].as_str().unwrap().len(), 64);
            }
            assert!(server.get("packages").is_none());
            continue;
        }
        // A server that is only its files (phpactor's PHAR): pinned below.
        if server.get("packages").is_none() {
            assert!(server.get("files").is_some(), "{server:?}");
            continue;
        }
        let lock = std::fs::read_to_string(
            root.join(server["packages"].as_str().unwrap()).join("package-lock.json"),
        )
        .unwrap();
        assert!(
            lock.lines()
                .filter(|l| l.contains("\"resolved\""))
                .all(|l| l.contains("https://registry.npmjs.org/"))
        );
        assert!(lock.contains("\"integrity\": \"sha512-"));
    }
}

#[test]
fn sizes_are_summed() {
    let dir = std::env::temp_dir().join(format!("lsp-bundle-size-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::write(dir.join("a/x"), [0u8; 10]).unwrap();
    std::fs::write(dir.join("y"), [0u8; 5]).unwrap();
    assert_eq!(size(&dir), 15);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pruned_paths_match_names_and_stars() {
    assert!(pruned("node_modules/typescript/bin", "node_modules/typescript/bin"));
    assert!(pruned("a/*/test", "a/html/test"));
    assert!(!pruned("a/*/test", "a/html/test/x.js"));
    assert!(!pruned("a/*/test", "a/html"));
    assert!(!pruned(
        "node_modules/typescript/lib/tsc.js",
        "node_modules/typescript/lib/typescript.js"
    ));
}

#[test]
fn data_files_stay_inside_the_servers_folder() {
    assert!(safe_relative("schemas/schemastore/package.json"));
    for bad in ["", "../x.json", "schemas/../../x", "/etc/passwd", "a//b", "C:/x", "a\\b"] {
        assert!(!safe_relative(bad), "{bad}");
    }
}

#[test]
fn data_files_are_pinned_on_github() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("packaging/lsp.toml")).unwrap()).unwrap();
    let files: Vec<&toml::Table> = manifest["servers"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s.get("files").and_then(toml::Value::as_array).into_iter().flatten())
        .map(|f| f.as_table().unwrap())
        .collect();
    assert!(files.len() >= 8, "the JSON schemas");
    for file in files {
        let url = file["url"].as_str().unwrap();
        assert!(
            url.starts_with("https://raw.githubusercontent.com/") || github_release(url),
            "{url}"
        );
        assert_eq!(file["sha256"].as_str().unwrap().len(), 64);
        assert!(safe_relative(file["path"].as_str().unwrap()));
    }
}

#[test]
fn a_tree_is_copied_without_what_is_pruned() {
    let dir = std::env::temp_dir().join(format!("lsp-bundle-prune-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let from = dir.join("from");
    std::fs::create_dir_all(from.join("lib/de")).unwrap();
    std::fs::create_dir_all(from.join("bin")).unwrap();
    std::fs::write(from.join("lib/de/messages.json"), "{}").unwrap();
    std::fs::write(from.join("lib/main.js"), "").unwrap();
    std::fs::write(from.join("bin/tool"), "").unwrap();
    std::fs::write(from.join(".installed"), "").unwrap();
    copy_tree(&from, &dir.join("to"), "", &["bin", "lib/*/messages.json"]).unwrap();
    assert!(dir.join("to/lib/main.js").is_file());
    assert!(!dir.join("to/bin").exists());
    assert!(!dir.join("to/lib/de").exists(), "a folder left empty isn't made");
    assert!(!dir.join("to/.installed").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_native_servers_are_bundled() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("packaging/lsp.toml")).unwrap()).unwrap();
    let native: Vec<&str> = manifest["servers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s.get("platforms").is_some())
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(native, ["clangd", "lua-language-server", "zls", "taplo"]);
}

#[test]
fn only_github_releases_are_native_sources() {
    assert!(github_release(
        "https://github.com/clangd/clangd/releases/download/23.1.0/clangd-mac-23.1.0.zip"
    ));
    for bad in [
        "http://github.com/clangd/clangd/releases/download/23.1.0/a.zip",
        "https://github.com/clangd/clangd/archive/refs/tags/a.zip",
        "https://example.com/clangd/clangd/releases/download/1/a.zip",
        "https://github.com/clangd/releases/download/1",
        "https://github.com/a/b/releases/download/../../x/a.zip",
    ] {
        assert!(!github_release(bad), "{bad}");
    }
}

/// A universal Mach-O program with `slices` (cputype, bytes), 32-bit offsets.
fn fat(slices: &[(u32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xcafe_babe_u32.to_be_bytes());
    out.extend_from_slice(&(slices.len() as u32).to_be_bytes());
    let mut offset = 8 + 20 * slices.len() as u32;
    for (cpu, bytes) in slices {
        for word in [*cpu, 0, offset, bytes.len() as u32, 0] {
            out.extend_from_slice(&word.to_be_bytes());
        }
        offset += bytes.len() as u32;
    }
    for (_, bytes) in slices {
        out.extend_from_slice(bytes);
    }
    out
}

#[test]
fn a_universal_program_is_thinned_to_one_architecture() {
    let program = fat(&[(0x0100_0007, b"intel"), (0x0100_000c, b"apple silicon")]);
    assert_eq!(thin(&program, MachArch::Arm64).unwrap(), Some(&b"apple silicon"[..]));
    assert_eq!(thin(&program, MachArch::X86_64).unwrap(), Some(&b"intel"[..]));
    // A thin program (or anything else) is left as it is.
    assert_eq!(thin(b"\xcf\xfa\xed\xfe....", MachArch::Arm64).unwrap(), None);
    assert_eq!(thin(b"", MachArch::Arm64).unwrap(), None);
    // One without the architecture, or cut short, is an error.
    assert!(thin(&fat(&[(0x0100_0007, b"intel")]), MachArch::Arm64).is_err());
    let mut short = fat(&[(0x0100_000c, b"apple silicon")]);
    short.truncate(short.len() - 3);
    assert!(thin(&short, MachArch::Arm64).is_err());
    assert_eq!(macos_arch("macos-arm64"), Some(MachArch::Arm64));
    assert_eq!(macos_arch("windows-x64"), None);
}

#[test]
fn a_universal_file_is_thinned_in_place_keeping_its_permissions() {
    let dir = std::env::temp_dir().join(format!("lsp-bundle-thin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tool");
    std::fs::write(&path, fat(&[(0x0100_0007, b"intel"), (0x0100_000c, b"arm")])).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    thin_file(&path, MachArch::Arm64).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"arm");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn debug_information_is_stripped_only_by_a_tool_for_the_platform() {
    assert!(strip_tools("macos-arm64").is_empty(), "macOS programs are left as they come");
    assert!(!strip_tools("linux-x64").is_empty());
    assert!(!strip_tools("windows-x64").is_empty());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("packaging/lsp.toml")).unwrap()).unwrap();
    for server in manifest["servers"].as_array().unwrap() {
        for (platform, files) in
            server.get("strip-debug").and_then(toml::Value::as_table).into_iter().flatten()
        {
            assert!(PLATFORMS.contains(&platform.as_str()), "{platform}");
            for file in files.as_array().unwrap() {
                assert!(safe_relative(file.as_str().unwrap()));
            }
        }
    }
}

#[test]
fn a_single_gzipped_program_is_told_from_an_archive() {
    assert!(single_gzip("taplo-darwin-aarch64.gz"));
    assert!(!single_gzip("lua-language-server-3.19.2-darwin-arm64.tar.gz"));
    assert!(!single_gzip("zls-x86_64-linux.tar.xz"));
    assert!(!single_gzip("taplo-windows-x86_64.zip"));
}

#[test]
fn a_gzipped_program_is_unpacked_executable() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("lsp-gunzip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("tool.gz");
    let mut encoder =
        flate2::write::GzEncoder::new(std::fs::File::create(&archive).unwrap(), Default::default());
    encoder.write_all(b"#!/bin/sh\necho hi\n").unwrap();
    encoder.finish().unwrap();
    gunzip(&archive, &dir.join("tool")).unwrap();
    assert_eq!(std::fs::read(dir.join("tool")).unwrap(), b"#!/bin/sh\necho hi\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("tool")).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "executable");
    }
    assert!(gunzip(&dir.join("missing.gz"), &dir.join("x")).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn urls_resolve_against_a_schemas_id() {
    let base = "https://json.schemastore.org/pyproject.json";
    assert_eq!(
        resolve_url(base, "partial-black.json"),
        "https://json.schemastore.org/partial-black.json"
    );
    assert_eq!(resolve_url(base, "./ruff.json"), "https://json.schemastore.org/ruff.json");
    assert_eq!(
        resolve_url(base, "https://www.schemastore.org/x.json"),
        "https://www.schemastore.org/x.json"
    );
    assert_eq!(resolve_url("", "x.json"), "x.json");
}

#[test]
fn a_schema_is_made_self_contained() {
    use serde_json::json;
    let root = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": "https://store.test/cargo.json",
        "definitions": {"Edition": {"enum": ["2021", {"$ref": "data, not a schema"}]}},
        "properties": {
            "edition": {"$ref": "#/definitions/Edition"},
            "lints": {"$ref": "https://other.test/lints.json"},
            "clippy": {"$ref": "https://other.test/lints.json#/definitions/Level"},
            "tool": {"$ref": "tool.json"},
            "again": {"$ref": "https://store.test/cargo.json#/definitions/Edition"}
        }
    });
    let lints = json!({
        "$id": "https://other.test/lints.json",
        "$schema": "http://json-schema.org/draft-07/schema#",
        "definitions": {"Level": {"enum": ["warn", "deny"]}},
        "additionalProperties": {"$ref": "#/definitions/Level"},
        "properties": {"tool": {"$ref": "https://store.test/tool.json"}}
    });
    let tool = json!({"$id": "https://store.test/tool.json", "properties": {"me": {"$ref": "#"}}});
    let load = |url: &str| match url {
        "https://other.test/lints.json" => Some(lints.clone()),
        "https://store.test/tool.json" => Some(tool.clone()),
        _ => None,
    };
    let made = inline_schema(root, &load).unwrap();
    let props = &made["properties"];
    assert_eq!(props["edition"]["$ref"], "#/definitions/Edition");
    assert_eq!(props["lints"]["$ref"], "#/definitions/x-bundled-lints");
    assert_eq!(props["clippy"]["$ref"], "#/definitions/x-bundled-lints/definitions/Level");
    assert_eq!(props["tool"]["$ref"], "#/definitions/x-bundled-tool");
    assert_eq!(props["again"]["$ref"], "#/definitions/Edition");
    // Data is left alone.
    assert_eq!(made["definitions"]["Edition"]["enum"][1], json!({"$ref": "data, not a schema"}));
    let inlined = &made["definitions"]["x-bundled-lints"];
    assert!(inlined.get("$id").is_none() && inlined.get("$schema").is_none());
    assert_eq!(
        inlined["additionalProperties"]["$ref"],
        "#/definitions/x-bundled-lints/definitions/Level"
    );
    // Inlined once, wherever it's referred to from.
    assert_eq!(inlined["properties"]["tool"]["$ref"], "#/definitions/x-bundled-tool");
    assert_eq!(
        made["definitions"]["x-bundled-tool"]["properties"]["me"]["$ref"],
        "#/definitions/x-bundled-tool"
    );
    assert_eq!(made["definitions"].as_object().unwrap().len(), 3);
    // What isn't bundled is said: the server would fetch it.
    let missing = json!({"$id": "https://a.test/a.json", "items": {"$ref": "b.json"}});
    let error = inline_schema(missing, &|_| None).unwrap_err();
    assert!(error.contains("https://a.test/b.json"), "{error}");
}

#[test]
fn php_is_pinned_for_every_platform_from_static_php_clis_builds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("packaging/lsp.toml")).unwrap()).unwrap();
    let php = manifest["php"].as_table().unwrap();
    let version = php["version"].as_str().unwrap();
    for platform in PLATFORMS {
        let entry = php["platforms"][platform].as_table().unwrap();
        let url = entry["url"].as_str().unwrap();
        assert!(static_php(url), "{url}");
        // The same version everywhere.
        assert!(url.contains(&format!("php-{version}-cli-")), "{url}");
        assert_eq!(entry["sha256"].as_str().unwrap().len(), 64);
    }
    // Its licences, pinned on GitHub.
    for file in php["files"].as_array().unwrap() {
        assert!(file["url"].as_str().unwrap().starts_with("https://raw.githubusercontent.com/"));
        assert_eq!(file["sha256"].as_str().unwrap().len(), 64);
    }
    // phpactor: its release's PHAR.
    let phpactor = manifest["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"].as_str() == Some("phpactor"))
        .unwrap();
    let phar = &phpactor["files"][0];
    assert!(github_release(phar["url"].as_str().unwrap()));
    assert_eq!(phar["path"].as_str(), Some("phpactor.phar"));
}

#[test]
fn php_comes_only_from_static_php_clis_builds() {
    assert!(static_php(
        "https://dl.static-php.dev/static-php-cli/minimal/php-8.4.22-cli-macos-aarch64.tar.gz"
    ));
    assert!(static_php(
        "https://dl.static-php.dev/static-php-cli/windows/spc-min/php-8.4.22-cli-win.zip"
    ));
    for bad in [
        "http://dl.static-php.dev/static-php-cli/minimal/php-8.4.22-cli-linux-x86_64.tar.gz",
        "https://dl.static-php.dev/static-php-cli/../evil/php-1.tar.gz",
        "https://dl.static-php.dev/static-php-cli/minimal/spc.tar.gz",
        "https://example.com/static-php-cli/minimal/php-8.4.22-cli-linux-x86_64.tar.gz",
    ] {
        assert!(!static_php(bad), "{bad}");
    }
}
