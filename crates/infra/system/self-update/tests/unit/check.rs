use super::*;
use pinned_downloads::fetch::Memory;
use ring::signature::{Ed25519KeyPair, KeyPair};

const BASE: &str = "https://github.com/o/r/releases/download";
const LIST: &str = "https://api.github.com/repos/o/r/releases?per_page=30";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A key made from `seed` (the same seed, the same key).
fn key(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

/// A GitHub serving release `tag` (`version` in its manifest), signed by `key`.
pub(crate) fn serve(memory: &Memory, key: &Ed25519KeyPair, tag: &str, version: &str) -> String {
    let manifest = format!(
        r#"{{"version": "{version}", "date": "2026-11-20", "files": [
          {{"platform": "macos-universal", "name": "app-{version}-macos-universal.zip", "size": 3, "sha256": "{0}"}},
          {{"platform": "windows-x64", "name": "app-{version}-windows-x64.zip", "size": 3, "sha256": "{0}"}}
        ]}}"#,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let url = |name: &str| format!("{BASE}/{tag}/{name}");
    let names = [
        "latest.json".to_owned(),
        "latest.json.sig".to_owned(),
        format!("app-{version}-macos-universal.zip"),
        format!("app-{version}-windows-x64.zip"),
    ];
    let assets: Vec<serde_json::Value> = names
        .iter()
        .map(|n| serde_json::json!({"name": n, "browser_download_url": url(n), "size": 3}))
        .collect();
    let list = serde_json::json!([
        {"tag_name": tag, "name": format!("App {version}"), "prerelease": version.contains('-'),
         "body": "## What's new\n- tabs", "assets": assets},
        {"tag_name": "v0.1.0", "assets": []}
    ]);
    memory.serve(LIST, list.to_string().into_bytes());
    memory.serve(&url("latest.json"), manifest.clone().into_bytes());
    memory.serve(&url("latest.json.sig"), hex(key.sign(manifest.as_bytes()).as_ref()).into_bytes());
    hex(key.public_key().as_ref())
}

fn ask<'a>(current: &'a Version, platforms: &'a [&'a str]) -> Ask<'a> {
    Ask { repository: "o/r", current, prereleases: true, skipped: None, platforms }
}

/// A newer release with a manifest its key signed: offered, this system's file found in it
/// (the universal Mac build for an Apple Silicon one without its own).
#[test]
fn a_signed_newer_release_is_offered_with_this_system_s_file() {
    let memory = Memory::default();
    let key = key(3);
    let public = serve(&memory, &key, "v1.0.0-beta.2", "1.0.0-beta.2");
    let current = Version::parse("1.0.0-beta.1").unwrap();
    let found =
        check_signed_by(&memory, ask(&current, &["macos-arm64", "macos-universal"]), &public)
            .unwrap()
            .expect("offered");
    assert_eq!(found.release.version.to_string(), "1.0.0-beta.2");
    assert_eq!(found.file.platform, "macos-universal");
    assert_eq!(found.url, format!("{BASE}/v1.0.0-beta.2/app-1.0.0-beta.2-macos-universal.zip"));
    // Up to date: nothing; no build for this system: said.
    let newest = Version::parse("1.0.0-beta.2").unwrap();
    assert_eq!(check_signed_by(&memory, ask(&newest, &["windows-x64"]), &public), Ok(None));
    assert!(matches!(
        check_signed_by(&memory, ask(&current, &["linux-x86_64"]), &public),
        Err(CheckError::Failed(_))
    ));
}

/// A manifest signed by another key, or for another version than its release's, is never
/// trusted; no network is said as such.
#[test]
fn what_doesnt_check_out_is_refused() {
    let memory = Memory::default();
    let current = Version::parse("1.0.0-beta.1").unwrap();
    let platforms = ["windows-x64"];
    serve(&memory, &key(3), "v1.0.0", "1.0.0");
    let ours = hex(key(4).public_key().as_ref());
    let refused = check_signed_by(&memory, ask(&current, &platforms), &ours).unwrap_err();
    assert!(refused.to_string().contains("isn't signed"), "{refused}");
    let key = key(5);
    let public = serve(&memory, &key, "v1.0.0", "1.0.1");
    let other = check_signed_by(&memory, ask(&current, &platforms), &public).unwrap_err();
    assert!(other.to_string().contains("is for 1.0.1"), "{other}");
    memory.set_offline(true);
    assert!(matches!(
        check_signed_by(&memory, ask(&current, &platforms), &public),
        Err(CheckError::Offline(_))
    ));
}

/// The real repository (`EDITOR_UPDATES_REPO=owner/name`) asked as an old version would
/// ask it: its newest release found, its manifest's signature checked with the key built
/// in, this system's file chosen. Run by hand (it goes to GitHub).
#[test]
#[ignore = "asks GitHub, given by hand"]
fn the_real_repository_offers_its_newest_release() {
    let Ok(repository) = std::env::var("EDITOR_UPDATES_REPO") else { return };
    let current = Version::parse("0.0.1").unwrap();
    let platforms = app_updates::platform::preferred(std::env::consts::OS, std::env::consts::ARCH);
    let ask = Ask {
        repository: &repository,
        current: &current,
        prereleases: true,
        skipped: None,
        platforms,
    };
    let found =
        check(&pinned_downloads::fetch::Http::new(crate::HOSTS), ask).unwrap().expect("a release");
    println!(
        "{} · {} · {} bytes · {}",
        found.release.tag, found.file.name, found.file.size, found.url
    );
    assert!(found.file.size > 0 && found.url.starts_with("https://github.com/"));
}

/// The same release's file downloaded from where GitHub sends it, checked against the signed
/// manifest, unpacked, its app found (`EDITOR_UPDATES_REPO`). Run by hand.
#[test]
#[ignore = "downloads from GitHub, given by hand"]
fn the_real_release_downloads_and_unpacks() {
    let Ok(repository) = std::env::var("EDITOR_UPDATES_REPO") else { return };
    let current = Version::parse("0.0.1").unwrap();
    let platforms = app_updates::platform::preferred(std::env::consts::OS, std::env::consts::ARCH);
    let ask = Ask {
        repository: &repository,
        current: &current,
        prereleases: true,
        skipped: None,
        platforms,
    };
    let http = pinned_downloads::fetch::Http::new(crate::HOSTS);
    let found = check(&http, ask).unwrap().expect("a release");
    let root = std::env::temp_dir()
        .join(format!("self-update-real-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    let stop = std::sync::atomic::AtomicBool::new(false);
    let staged =
        crate::stage::stage(&http, &found, &root, &crate::stage::app_name(), &|_, _| {}, &stop)
            .unwrap();
    println!("staged {} at {}", staged.version, staged.app.display());
    assert!(staged.app.exists());
    std::fs::remove_dir_all(&root).unwrap();
}

/// The project's key built in is a key.
#[test]
fn the_project_s_key_is_one() {
    assert!(pinned_downloads::signature::unhex::<32>(crate::PUBLIC_KEY).is_some());
}
