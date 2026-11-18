use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::fetch::Memory;
use pinned_downloads::samples::{Entry, ZipEntry, gz, tar, temp, xz, zip};

fn sha256(bytes: &[u8]) -> String {
    crate::install::hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

fn integrity(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        crate::install::base64(ring::digest::digest(&ring::digest::SHA512, bytes).as_ref())
    )
}

/// Node as the manifest describes it, its macOS archive served by `memory`.
fn node(memory: &Memory) -> Component {
    let archive = xz(&tar(&[
        Entry("node-v9-darwin-arm64/bin/node", b'0', 0o755, b"#!node"),
        Entry("node-v9-darwin-arm64/bin/npm", b'2', 0o777, b""),
        Entry("node-v9-darwin-arm64/LICENSE", b'0', 0o644, b"MIT"),
        Entry("node-v9-darwin-arm64/lib/node_modules/npm/index.js", b'0', 0o644, b"npm"),
    ]));
    let url = "https://nodejs.org/dist/v9/node-v9-darwin-arm64.tar.xz";
    memory.serve(url, archive.clone());
    let windows = zip(&[
        ZipEntry("node-v9-win-x64/node.exe", None, b"MZ", true),
        ZipEntry("node-v9-win-x64/LICENSE", None, b"MIT", true),
        ZipEntry("node-v9-win-x64/npm.cmd", None, b"@npm", true),
    ]);
    let windows_url = "https://nodejs.org/dist/v9/node-v9-win-x64.zip";
    memory.serve(windows_url, windows.clone());
    Component {
        name: "node".into(),
        role: Role::Node,
        version: "9.0.0".into(),
        license: "MIT".into(),
        platforms: BTreeMap::from([
            (
                "macos-arm64".to_owned(),
                Archive {
                    url: url.into(),
                    sha256: sha256(&archive),
                    size: archive.len() as u64,
                    strip: Some("node-v9-darwin-arm64".into()),
                    keep: vec!["bin/node".into(), "LICENSE".into()],
                    program: None,
                    bin: Some("bin/node".into()),
                },
            ),
            (
                "windows-x64".to_owned(),
                Archive {
                    url: windows_url.into(),
                    sha256: sha256(&windows),
                    size: windows.len() as u64,
                    strip: Some("node-v9-win-x64".into()),
                    keep: vec!["node.exe".into(), "LICENSE".into()],
                    program: None,
                    bin: Some("node.exe".into()),
                },
            ),
        ]),
        ..Component::default()
    }
}

/// A Node server: two packages (one inside the other), a schema referring to another
/// (inlined), a data file, something pruned.
fn server(memory: &Memory) -> Component {
    let a = gz(&tar(&[
        Entry("package/package.json", b'0', 0o644, br#"{"name": "a"}"#),
        Entry("package/bin/server.js", b'0', 0o755, b"require('b')"),
        Entry("package/test/huge.js", b'0', 0o644, b"pruned"),
    ]));
    let b = gz(&tar(&[Entry("package/index.js", b'0', 0o644, b"module.exports = 1")]));
    let mac_only = gz(&tar(&[Entry("package/fsevents.node", b'0', 0o644, b"bin")]));
    let schema = br#"{"$id": "https://json.schemastore.org/cargo.json", "properties": {"a": {"$ref": "https://json.schemastore.org/part.json"}}}"#;
    let part = br#"{"type": "string"}"#;
    let licence = b"MIT licence";
    let urls = [
        ("https://registry.npmjs.org/a/-/a-1.0.0.tgz", a.clone()),
        ("https://registry.npmjs.org/b/-/b-2.0.0.tgz", b.clone()),
        ("https://registry.npmjs.org/fsevents/-/fsevents-2.0.0.tgz", mac_only.clone()),
        ("https://raw.githubusercontent.com/x/y/abc/cargo.json", schema.to_vec()),
        ("https://raw.githubusercontent.com/x/y/abc/part.json", part.to_vec()),
        ("https://raw.githubusercontent.com/x/y/abc/LICENSE", licence.to_vec()),
    ];
    for (url, bytes) in &urls {
        memory.serve(url, bytes.clone());
    }
    let package = |path: &str, url: &str, bytes: &[u8], os: &[&str]| Package {
        path: path.into(),
        url: url.into(),
        integrity: integrity(bytes),
        size: bytes.len() as u64,
        os: os.iter().map(|s| (*s).to_owned()).collect(),
        cpu: Vec::new(),
    };
    let file = |path: &str, url: &str, bytes: &[u8]| DataFile {
        url: url.into(),
        sha256: sha256(bytes),
        path: path.into(),
        size: bytes.len() as u64,
    };
    Component {
        name: "the-server".into(),
        role: Role::Server,
        version: "1.0.0".into(),
        packages: vec![
            package("node_modules/a", urls[0].0, &a, &[]),
            package("node_modules/a/node_modules/b", urls[1].0, &b, &[]),
            package("node_modules/fsevents", urls[2].0, &mac_only, &["darwin"]),
        ],
        files: vec![
            file("schemas/parts/cargo.json", urls[3].0, schema),
            file("schemas/parts/part.json", urls[4].0, part),
            file("LICENSE", urls[5].0, licence),
        ],
        prune: vec!["node_modules/a/test".into()],
        inline: vec![Inline {
            schema: "schemas/parts/cargo.json".into(),
            out: "schemas/cargo.json".into(),
            urls: BTreeMap::from([(
                "https://json.schemastore.org/".to_owned(),
                "schemas/parts".to_owned(),
            )]),
        }],
        ..Component::default()
    }
}

fn run(root: &Path, c: &Component, platform: &str, memory: &Memory) -> Result<Found, FetchError> {
    install(root, c, platform, memory, &|_| {}, &|| {}, &AtomicBool::new(false))
}

#[test]
fn the_apps_manifest_is_read_and_every_source_is_allowed() {
    let text = include_str!("../../../../../modules/tools/lsp/data/downloads.toml");
    let manifest = Manifest::parse(text).unwrap();
    for name in [
        "node",
        "php",
        "vtsls",
        "vscode-langservers-extracted",
        "clangd",
        "lua-language-server",
        "zls",
        "bash-language-server",
        "yaml-language-server",
        "dockerfile-language-server-nodejs",
        "taplo",
        "phpactor",
    ] {
        assert!(manifest.named(name).is_some(), "{name}");
    }
    for platform in ["macos-arm64", "macos-x64", "linux-x64", "windows-x64"] {
        for c in &manifest.components {
            assert!(c.available(platform), "{} on {platform}", c.name);
            assert!(c.size(platform) > 0, "{} on {platform}", c.name);
        }
        assert!(manifest.of(Role::Node).unwrap().bin(platform).is_some());
    }
    assert_eq!(manifest.named("node").unwrap().folder(), "node/26.13.0");
    assert_eq!(manifest.named("vtsls").unwrap().folder(), "servers/vtsls/0.3.0");
    // Its recipe's fingerprint: stable, per platform.
    let clangd = manifest.named("clangd").unwrap();
    assert_eq!(clangd.fingerprint("macos-arm64"), clangd.fingerprint("macos-arm64"));
    assert_ne!(clangd.fingerprint("macos-arm64"), clangd.fingerprint("linux-x64"));
    // Another format, a source from elsewhere, a path leaving its folder: refused.
    assert!(Manifest::parse("format = 2").is_err());
    let elsewhere = text.replacen("https://nodejs.org/", "https://evil.example/", 1);
    assert!(Manifest::parse(&elsewhere).is_err());
    let escaping = text.replacen("path = \"node_modules/", "path = \"../node_modules/", 1);
    assert!(Manifest::parse(&escaping).is_err());
}

#[test]
fn a_package_for_another_system_is_left_out() {
    let only_mac = Package { os: vec!["darwin".into()], ..Package::default() };
    assert!(only_mac.applies("macos-arm64"));
    assert!(!only_mac.applies("windows-x64"));
    let not_windows =
        Package { os: vec!["!win32".into()], cpu: vec!["x64".into()], ..Package::default() };
    assert!(not_windows.applies("linux-x64"));
    assert!(!not_windows.applies("windows-x64"));
    assert!(!not_windows.applies("macos-arm64"));
}

#[test]
fn a_runtime_is_downloaded_checked_and_only_its_program_kept() {
    let root = temp("runtime");
    let memory = Memory::default();
    let c = node(&memory);
    let got = Mutex::new(Vec::new());
    let found = install(
        &root,
        &c,
        "macos-arm64",
        &memory,
        &|r| got.lock().unwrap().push(r),
        &|| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(found.dir, root.join("node/9.0.0"));
    assert!(found.current);
    assert_eq!(std::fs::read(found.dir.join("bin/node")).unwrap(), b"#!node");
    assert!(found.dir.join("LICENSE").is_file());
    assert!(!found.dir.join("bin/npm").exists() && !found.dir.join("lib").exists());
    assert_eq!(got.lock().unwrap().last(), Some(&c.size("macos-arm64")));
    // Its marker says what it is; nothing left beside it, the archive gone.
    let marker = crate::store::marker(&found.dir).unwrap();
    assert_eq!((marker.name.as_str(), marker.fingerprint), ("node", c.fingerprint("macos-arm64")));
    assert_eq!(std::fs::read_dir(root.join("node")).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(root.join(".downloads")).unwrap().count(), 0);
    assert_eq!(c.find(&root, "macos-arm64"), Some(found.clone()));
    // There already: nothing downloaded again.
    let asked = memory.asked().len();
    assert_eq!(run(&root, &c, "macos-arm64", &memory).unwrap().dir, found.dir);
    assert_eq!(memory.asked().len(), asked);
}

#[test]
fn windows_gets_its_zip_and_its_exe() {
    let root = temp("windows");
    let memory = Memory::default();
    let found = run(&root, &node(&memory), "windows-x64", &memory).unwrap();
    assert_eq!(std::fs::read(found.dir.join("node.exe")).unwrap(), b"MZ");
    assert!(!found.dir.join("npm.cmd").exists());
}

#[test]
fn a_node_server_is_assembled_from_its_lockfile() {
    let root = temp("npm");
    let memory = Memory::default();
    let c = server(&memory);
    let found = run(&root, &c, "linux-x64", &memory).unwrap();
    let dir = found.dir;
    assert_eq!(dir, root.join("servers/the-server/1.0.0"));
    assert!(dir.join("node_modules/a/bin/server.js").is_file());
    assert_eq!(
        std::fs::read_to_string(dir.join("node_modules/a/node_modules/b/index.js")).unwrap(),
        "module.exports = 1"
    );
    assert!(!dir.join("node_modules/a/test").exists(), "pruned");
    assert!(!dir.join("node_modules/fsevents").exists(), "another system's");
    assert!(dir.join("LICENSE").is_file());
    // The schema made self-contained, its parts gone.
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("schemas/cargo.json")).unwrap())
            .unwrap();
    assert!(schema.to_string().contains("x-bundled-part"), "{schema}");
    assert!(!dir.join("schemas/parts").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("node_modules/a/bin/server.js"))
            .unwrap()
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0);
    }
    // On macOS the package for it is there.
    let mac = temp("npm-mac");
    let found = run(&mac, &c, "macos-arm64", &memory).unwrap();
    assert!(found.dir.join("node_modules/fsevents/fsevents.node").is_file());
}

#[test]
fn a_download_that_does_not_match_installs_nothing() {
    let root = temp("tampered");
    let memory = Memory::default();
    let mut c = server(&memory);
    c.packages[1].integrity = integrity(b"something else");
    let error = run(&root, &c, "linux-x64", &memory).unwrap_err();
    assert!(matches!(error, FetchError::Mismatch(_)), "{error}");
    assert_eq!(c.find(&root, "linux-x64"), None);
    // Nothing half made beside where it would be.
    assert_eq!(std::fs::read_dir(root.join("servers/the-server")).unwrap().count(), 0);
}

#[test]
fn an_interrupted_download_is_resumed_and_a_stopped_one_cleaned() {
    let root = temp("interrupted");
    let memory = Memory::default();
    let c = node(&memory);
    let size = c.platforms["macos-arm64"].size;
    memory.cut_next_after(size / 2);
    let error = run(&root, &c, "macos-arm64", &memory).unwrap_err();
    assert!(error.offline(), "{error}");
    assert_eq!(c.find(&root, "macos-arm64"), None);
    // Its half kept, nothing else.
    let part = root.join(".downloads").join(format!("{}.part", c.platforms["macos-arm64"].sha256));
    assert_eq!(std::fs::metadata(&part).unwrap().len(), size / 2);
    assert_eq!(std::fs::read_dir(root.join("node")).unwrap().count(), 0);
    run(&root, &c, "macos-arm64", &memory).unwrap();
    assert_eq!(memory.asked_from().last().unwrap().1, size / 2);
    assert!(!part.exists());
    // Stopped: nothing installed, nothing left half made.
    let other = temp("stopped");
    let stop = AtomicBool::new(true);
    let error = install(&other, &server(&memory), "linux-x64", &memory, &|_| {}, &|| {}, &stop)
        .unwrap_err();
    assert_eq!(error, FetchError::Stopped);
    assert_eq!(std::fs::read_dir(other.join("servers/the-server")).unwrap().count(), 0);
}

#[test]
fn two_installers_of_one_component_download_it_once() {
    let root = temp("two");
    let memory = Arc::new(Memory::default());
    let c = node(&memory);
    // The first stalls half way, holding the component's lock.
    memory.stall_after(Some(c.platforms["macos-arm64"].size / 2));
    let waited = Arc::new(AtomicUsize::new(0));
    let first = {
        let (root, memory, c) = (root.clone(), Arc::clone(&memory), c.clone());
        std::thread::spawn(move || run(&root, &c, "macos-arm64", &memory))
    };
    while memory.asked().is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let second = {
        let (root, memory, c, waited) = (root, Arc::clone(&memory), c.clone(), Arc::clone(&waited));
        std::thread::spawn(move || {
            install(
                &root,
                &c,
                "macos-arm64",
                memory.as_ref(),
                &|_| {},
                &|| {
                    waited.fetch_add(1, Ordering::Relaxed);
                },
                &AtomicBool::new(false),
            )
        })
    };
    while waited.load(Ordering::Relaxed) == 0 {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // The first breaks off; the second one gets the lock, resumes and installs it.
    memory.stall_after(None);
    memory.release();
    assert!(first.join().unwrap().is_err());
    let found = second.join().unwrap().unwrap();
    assert!(found.current);
    assert_eq!(memory.asked().len(), 2, "{:?}", memory.asked_from());
    assert_eq!(memory.asked_from()[1].1, c.platforms["macos-arm64"].size / 2, "resumed");
}

#[test]
fn a_universal_program_is_thinned_to_the_platform() {
    fn fat(slices: &[(u32, &[u8])]) -> Vec<u8> {
        let mut out = vec![0xca, 0xfe, 0xba, 0xbe];
        out.extend_from_slice(&(slices.len() as u32).to_be_bytes());
        let mut offset = 8 + 20 * slices.len() as u32;
        let mut bodies = Vec::new();
        for (cpu, body) in slices {
            out.extend_from_slice(&cpu.to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&offset.to_be_bytes());
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes());
            offset += body.len() as u32;
            bodies.extend_from_slice(body);
        }
        out.extend_from_slice(&bodies);
        out
    }
    let program = fat(&[(0x0100_0007, b"x86 code"), (0x0100_000c, b"arm code")]);
    let archive = zip(&[ZipEntry("clangd_1/bin/clangd", Some(0o100755), &program, true)]);
    let memory = Memory::default();
    let url = "https://github.com/clangd/clangd/releases/download/1/clangd-mac-1.zip";
    memory.serve(url, archive.clone());
    let entry = Archive {
        url: url.into(),
        sha256: sha256(&archive),
        size: archive.len() as u64,
        strip: Some("clangd_1".into()),
        ..Archive::default()
    };
    let c = Component {
        name: "clangd".into(),
        version: "1".into(),
        platforms: BTreeMap::from([
            ("macos-arm64".to_owned(), entry.clone()),
            ("macos-x64".to_owned(), entry),
        ]),
        universal: vec!["bin/clangd".into()],
        ..Component::default()
    };
    let arm = run(&temp("thin-arm"), &c, "macos-arm64", &memory).unwrap();
    assert_eq!(std::fs::read(arm.dir.join("bin/clangd")).unwrap(), b"arm code");
    let x86 = run(&temp("thin-x86"), &c, "macos-x64", &memory).unwrap();
    assert_eq!(std::fs::read(x86.dir.join("bin/clangd")).unwrap(), b"x86 code");
}

#[test]
fn another_recipe_of_the_version_is_made_again() {
    let root = temp("recipe");
    let memory = Memory::default();
    let mut c = server(&memory);
    run(&root, &c, "linux-x64", &memory).unwrap();
    c.prune.push("node_modules/a/bin".into());
    assert!(!c.find(&root, "linux-x64").unwrap().current);
    let found = run(&root, &c, "linux-x64", &memory).unwrap();
    assert!(found.current && !found.dir.join("node_modules/a/bin").exists());
    assert_eq!(std::fs::read_dir(root.join("servers/the-server")).unwrap().count(), 1);
}

/// The real thing (only with `EDITOR_TEST_LSP_DOWNLOAD=<folder>`, never in the user's data
/// folder): every component of the app's manifest downloaded for this platform from its
/// pinned sources into the folder, timed, with a bundle manifest beside them so the real
/// servers' tests can run against it (`EDITOR_LSP_DIR=<folder>`).
#[test]
fn every_component_downloads_from_its_real_source() {
    let Some(root) = std::env::var_os("EDITOR_TEST_LSP_DOWNLOAD").map(std::path::PathBuf::from)
    else {
        eprintln!("EDITOR_TEST_LSP_DOWNLOAD not set: skipped");
        return;
    };
    let text = include_str!("../../../../../modules/tools/lsp/data/downloads.toml");
    let manifest = Manifest::parse(text).unwrap();
    let platform = crate::bundle::platform();
    let http = crate::fetch::Http::default();
    let mut bundle = format!("platform = \"{platform}\"\n");
    let mut servers = String::new();
    for c in &manifest.components {
        let started = std::time::Instant::now();
        let found = install(&root, c, platform, &http, &|_| {}, &|| {}, &AtomicBool::new(false))
            .unwrap_or_else(|e| panic!("{}: {e}", c.name));
        eprintln!(
            "{:<36} {:>9.1} MB downloaded {:>9.1} MB on disk in {:>6.1} s",
            c.name,
            c.size(platform) as f64 / 1_048_576.0,
            found.size as f64 / 1_048_576.0,
            started.elapsed().as_secs_f64()
        );
        let folder = c.folder();
        match c.role {
            Role::Node | Role::Php => {
                let key = if c.role == Role::Node { "node" } else { "php" };
                bundle.push_str(&format!(
                    "\n[{key}]\nversion = \"{}\"\npath = \"{folder}/{}\"\n",
                    c.version,
                    c.bin(platform).unwrap()
                ));
            }
            Role::Server => servers.push_str(&format!(
                "\n[servers.{}]\nversion = \"{}\"\npath = \"{folder}\"\n",
                c.name, c.version
            )),
        }
    }
    std::fs::write(root.join(crate::bundle::MANIFEST), bundle + &servers).unwrap();
}
