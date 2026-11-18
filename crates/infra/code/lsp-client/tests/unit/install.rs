use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use super::*;

/// A folder of the temporary folder for test `name` (never the app's data folder).
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lsp-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A tar entry: its header (ustar) and body, padded.
fn entry(out: &mut Vec<u8>, name: &str, kind: u8, body: &[u8]) {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..107].copy_from_slice(b"0000644");
    header[124..135].copy_from_slice(format!("{:011o}", body.len()).as_bytes());
    header[156] = kind;
    header[257..262].copy_from_slice(b"ustar");
    // The checksum, over the header with its field as spaces.
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(body);
    out.resize(out.len().div_ceil(512) * 512, 0);
}

/// A package's tarball as npm publishes it (everything under `package/`), gzipped.
fn tarball(name: &str, version: &str, extra: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    let json = format!("{{\"name\": \"{name}\", \"version\": \"{version}\"}}");
    entry(&mut tar, "package/package.json", b'0', json.as_bytes());
    entry(&mut tar, "package/lib/server.js", b'0', b"console.log('server')");
    for (path, kind, body) in extra {
        entry(&mut tar, path, *kind, body);
    }
    tar.extend_from_slice(&[0u8; 1024]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

fn integrity(bytes: &[u8]) -> String {
    format!("sha512-{}", base64(ring::digest::digest(&ring::digest::SHA512, bytes).as_ref()))
}

fn described(name: &str, version: &str, integrity: String) -> Install {
    Install {
        npm: name.into(),
        version: version.into(),
        integrity,
        program: None,
        entry: "lib/server.js".into(),
        licence_url: None,
        size: None,
    }
}

/// A registry in memory: the URLs asked for, and what it answers.
struct FakeRegistry {
    asked: Mutex<Vec<String>>,
    body: Vec<u8>,
}

impl Fetch for FakeRegistry {
    fn open(&self, url: &str, _from: u64) -> Result<crate::fetch::Body, crate::fetch::FetchError> {
        self.asked.lock().unwrap().push(url.to_owned());
        let total = self.body.len() as u64;
        Ok(crate::fetch::Body {
            from: 0,
            total: Some(total),
            reader: Box::new(std::io::Cursor::new(self.body.clone())),
        })
    }
}

#[test]
fn base64_is_the_standard_one() {
    for (plain, coded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(plain.as_bytes()), coded);
    }
}

#[test]
fn a_download_is_checked_against_its_integrity() {
    let bytes = b"the package".to_vec();
    assert!(verify(&bytes, &integrity(&bytes)));
    assert!(!verify(b"the packagf", &integrity(&bytes)));
    assert!(!verify(&bytes, "sha1-abc"));
    assert!(!verify(&bytes, ""));
}

#[test]
fn the_tarball_is_on_the_registry() {
    let plain = described("intelephense", "1.18.5", String::new());
    assert_eq!(
        tarball_url(&plain),
        "https://registry.npmjs.org/intelephense/-/intelephense-1.18.5.tgz"
    );
    let scoped = described("@scope/server", "2.0.0", String::new());
    assert_eq!(tarball_url(&scoped), "https://registry.npmjs.org/@scope/server/-/server-2.0.0.tgz");
}

#[test]
fn a_package_is_installed_checked_and_uninstalled() {
    let installs = temp("whole");
    let long = format!("package/{}/deep.txt", "d".repeat(120));
    let mut pax = Vec::new();
    let record = format!("path={long}\n");
    let length = record.len() + 4;
    pax.extend_from_slice(format!("{length} {record}").as_bytes());
    let body = tarball(
        "server-pkg",
        "1.2.3",
        &[
            ("package/stubs", b'5', b""),
            ("package/stubs/a.php", b'0', b"<?php"),
            ("ignored", b'x', &pax),
            ("package/ignored-name", b'0', b"long"),
            ("package/link", b'2', b""),
        ],
    );
    let registry = FakeRegistry { asked: Mutex::default(), body: body.clone() };
    let install_ = described("server-pkg", "1.2.3", integrity(&body));
    let got = Mutex::new(Vec::new());
    let progress = |received: u64, total: Option<u64>| got.lock().unwrap().push((received, total));
    let stop = AtomicBool::new(false);
    let installed = install(&installs, "server", &install_, &registry, &progress, &stop).unwrap();
    assert_eq!(
        registry.asked.lock().unwrap().as_slice(),
        ["https://registry.npmjs.org/server-pkg/-/server-pkg-1.2.3.tgz"]
    );
    assert_eq!(installed.version, "1.2.3");
    assert!(installed.ours);
    assert_eq!(installed.dir, version_dir(&installs, "server", "1.2.3"));
    assert!(installed.dir.join("lib/server.js").is_file());
    assert!(installed.dir.join("stubs/a.php").is_file());
    // The pax header's long path names the entry after it.
    assert!(installed.dir.join(long.trim_start_matches("package/")).is_file());
    assert!(!installed.dir.join("ignored-name").exists());
    assert!(!installed.dir.join("link").exists(), "links are never made");
    let total = body.len() as u64;
    assert_eq!(got.lock().unwrap().last(), Some(&(total, Some(total))));
    // Nothing left beside it: the temporary folder was moved.
    let beside: Vec<_> = std::fs::read_dir(installs.join("server")).unwrap().flatten().collect();
    assert_eq!(beside.len(), 1);
    assert_eq!(installed_in(&installs, "server", &install_), Some(installed));
    uninstall(&installs, "server").unwrap();
    assert_eq!(installed_in(&installs, "server", &install_), None);
    uninstall(&installs, "server").unwrap();
}

#[test]
fn a_tampered_download_installs_nothing() {
    let installs = temp("tampered");
    let body = tarball("server-pkg", "1.2.3", &[]);
    let pinned = integrity(&tarball("server-pkg", "1.2.4", &[]));
    let registry = FakeRegistry { asked: Mutex::default(), body };
    let install_ = described("server-pkg", "1.2.3", pinned);
    let stop = AtomicBool::new(false);
    let error = install(&installs, "server", &install_, &registry, &|_, _| {}, &stop).unwrap_err();
    assert!(error.contains("integrity"), "{error}");
    assert!(!installs.join("server").exists());
    assert_eq!(installed_in(&installs, "server", &install_), None);
}

#[test]
fn an_archive_leaving_its_folder_is_refused() {
    let into = temp("escape");
    let body = tarball("server-pkg", "1.0.0", &[("package/../../evil.txt", b'0', b"x")]);
    let stop = AtomicBool::new(false);
    assert!(unpack(&body, &into.join("out"), &stop).is_err());
    assert!(!into.join("evil.txt").exists());
    let absolute = tarball("server-pkg", "1.0.0", &[("/tmp/evil.txt", b'0', b"x")]);
    assert!(unpack(&absolute, &into.join("out2"), &stop).is_err());
    assert!(unpack(b"not gzip", &into.join("out3"), &stop).is_err());
}

#[test]
fn a_package_is_known_by_its_name_and_entry() {
    let dir = temp("known");
    let install_ = described("server-pkg", "1.0.0", String::new());
    assert_eq!(package_at(&dir, &install_), None);
    std::fs::write(dir.join("package.json"), r#"{"name": "other", "version": "1.0.0"}"#).unwrap();
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/server.js"), "").unwrap();
    assert_eq!(package_at(&dir, &install_), None, "another package");
    std::fs::write(dir.join("package.json"), r#"{"name": "server-pkg", "version": "1.0.9"}"#)
        .unwrap();
    let found = package_at(&dir, &install_).unwrap();
    assert_eq!((found.version.as_str(), found.ours), ("1.0.9", false));
    std::fs::remove_file(dir.join("lib/server.js")).unwrap();
    assert_eq!(package_at(&dir, &install_), None, "no entry");
}

#[test]
fn an_install_made_elsewhere_is_found() {
    let root = temp("existing");
    let package = root.join("server-pkg");
    std::fs::create_dir_all(package.join("lib")).unwrap();
    std::fs::write(package.join("package.json"), r#"{"name": "server-pkg", "version": "1.17.0"}"#)
        .unwrap();
    std::fs::write(package.join("lib/server.js"), "").unwrap();
    let install_ = described("server-pkg", "1.18.0", String::new());
    let found = find_existing(&install_, Some(&root)).unwrap();
    assert_eq!((found.dir, found.version.as_str(), found.ours), (package, "1.17.0", false));
    assert_eq!(find_existing(&install_, None), None);
    // An older install of the app's own still counts (until the pinned one is installed).
    let installs = temp("older");
    let older = version_dir(&installs, "server", "1.17.2");
    std::fs::create_dir_all(older.join("lib")).unwrap();
    std::fs::write(older.join("package.json"), r#"{"name": "server-pkg", "version": "1.17.2"}"#)
        .unwrap();
    std::fs::write(older.join("lib/server.js"), "").unwrap();
    let mine = installed_in(&installs, "server", &install_).unwrap();
    assert_eq!((mine.version.as_str(), mine.ours), ("1.17.2", true));
}
