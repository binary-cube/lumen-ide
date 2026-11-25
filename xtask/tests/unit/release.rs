use super::*;

/// A release file's platform is read from its name; other files have none.
#[test]
fn a_release_file_names_its_platform() {
    let v = "1.0.0-beta.1";
    assert_eq!(
        platform_of("app-1.0.0-beta.1-macos-universal.zip", "app", v).as_deref(),
        Some("macos-universal")
    );
    assert_eq!(
        platform_of("app-1.0.0-beta.1-windows-x64.zip", "app", v).as_deref(),
        Some("windows-x64")
    );
    assert_eq!(
        platform_of("app-1.0.0-beta.1-linux-x86_64.tar.gz", "app", v).as_deref(),
        Some("linux-x86_64")
    );
    assert_eq!(platform_of("app-0.9.0-windows-x64.zip", "app", v), None, "another version");
    assert_eq!(platform_of("SHA256SUMS", "app", v), None);
    assert_eq!(platform_of("app-1.0.0-beta.1-.zip", "app", v), None, "no platform");
    assert_eq!(platform_of("app-1.0.0-beta.1-a b.zip", "app", v), None, "not a name");
}

/// Hex both ways; anything but exactly the bytes asked is refused.
#[test]
fn hex_goes_both_ways() {
    assert_eq!(hex(&[0, 15, 255]), "000fff");
    assert_eq!(unhex::<3>(" 000fff\n"), Some([0, 15, 255]));
    assert_eq!(unhex::<3>("000ff"), None);
    assert_eq!(unhex::<3>("000fffaa"), None);
    assert_eq!(unhex::<2>("zz00"), None);
    assert_eq!(unhex::<2>("éé"), None, "never split inside a character");
}

/// A manifest signed with a key is checked by its public half only, and only as written:
/// another key, or one byte changed, fails.
#[test]
fn a_signed_manifest_is_checked_as_written() {
    let (secret, public) = new_key().unwrap();
    let assets = [Asset {
        platform: "windows-x64".into(),
        name: "app-1.0.0-windows-x64.zip".into(),
        size: 3,
        sha256: pinned_downloads::digest::sha256_hex(b"abc"),
    }];
    let text = manifest("1.0.0", "2026-11-18", &assets);
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["files"][0]["size"], 3);
    assert_eq!(
        doc["files"][0]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let signature = sign(&text, &secret).unwrap();
    assert!(verify(&text, &signature, &public));
    assert!(!verify(&text.replace("1.0.0", "1.0.1"), &signature, &public), "changed");
    let (_, other) = new_key().unwrap();
    assert!(!verify(&text, &signature, &other), "another key");
    assert!(!verify(&text, "00", &public), "not a signature");
    assert!(sign(&text, "nope").is_err());
    assert_eq!(sums(&assets), format!("{}  app-1.0.0-windows-x64.zip\n", assets[0].sha256));
}

/// A key's secret is its Ed25519 seed (RFC 8032), as `release-key` has always written it: the
/// RFC's first test vector signs and derives as published, and the app's check accepts it.
#[test]
fn a_seed_signs_as_rfc_8032_says() {
    let seed = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    let public = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    let signature = sign("", seed).unwrap();
    assert_eq!(
        signature,
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065\
         224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    );
    assert!(pinned_downloads::signature::verify(b"", &signature, public));
    let (secret, made) = new_key().unwrap();
    assert_eq!(secret.len(), 64);
    let signed = sign("text", &secret).unwrap();
    assert!(pinned_downloads::signature::verify(b"text", &signed, &made));
}

/// The manifest from a folder: the version's files only, sums and a signature checked against
/// the project's key; without the secret, unsigned unless a signature is required.
#[test]
fn the_manifest_is_written_from_the_release_files() {
    let root = std::env::temp_dir()
        .join(format!("release-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    let dist = root.join("dist");
    std::fs::create_dir_all(root.join("packaging")).unwrap();
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("app-1.0.0-macos-universal.zip"), b"mac").unwrap();
    std::fs::write(dist.join("app-1.0.0-windows-x64.zip"), b"win").unwrap();
    std::fs::write(dist.join("app-0.9.0-windows-x64.zip"), b"old").unwrap();
    // The installer: in the sums, and in the manifest as `<platform>-setup` (what an app it
    // installed updates through).
    std::fs::write(dist.join("app-1.0.0-windows-x64-setup.exe"), b"setup").unwrap();
    let secret_file = root.join("secret");
    make_key(&root, &secret_file).unwrap();
    assert!(make_key(&root, &root.join("again")).is_err(), "a key once");
    let secret = std::fs::read_to_string(&secret_file).unwrap();

    let public = std::fs::read_to_string(root.join(PUBLIC_KEY)).unwrap();
    let ask = |version, secret: Option<&str>, signed| {
        write_manifest(&ManifestAsk {
            public: &public,
            dist: &dist,
            id: "app",
            version,
            date: "2026-11-18",
            secret,
            signed,
        })
    };
    let done = ask("1.0.0", Some(&secret), true).unwrap();
    assert!(done.contains("signed"), "{done}");
    let text = std::fs::read_to_string(dist.join("latest.json")).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let platforms: Vec<&str> =
        doc["files"].as_array().unwrap().iter().map(|f| f["platform"].as_str().unwrap()).collect();
    assert_eq!(platforms, ["macos-universal", "windows-x64", "windows-x64-setup"]);
    let signature = std::fs::read_to_string(dist.join("latest.json.sig")).unwrap();
    assert!(verify(&text, &signature, &public));
    let sums = std::fs::read_to_string(dist.join("SHA256SUMS")).unwrap();
    assert_eq!(sums.lines().count(), 3, "{sums}");
    assert!(sums.contains("app-1.0.0-windows-x64-setup.exe"));
    assert!(done.contains("installer  app-1.0.0-windows-x64-setup.exe"), "{done}");
    assert!(is_installer("app-1.0.0-windows-x64-setup.exe", "app", "1.0.0"));
    assert!(!is_installer("app-1.0.0-windows-x64.zip", "app", "1.0.0"));
    assert!(!is_installer("app-0.9.0-windows-x64-setup.exe", "app", "1.0.0"), "another version's");

    assert!(ask("1.0.0", None, true).is_err(), "required");
    let (other, _) = new_key().unwrap();
    assert!(ask("1.0.0", Some(&other), true).is_err(), "not ours");
    assert!(ask("2.0.0", None, false).is_err(), "no files");
    std::fs::remove_dir_all(&root).unwrap();
}
