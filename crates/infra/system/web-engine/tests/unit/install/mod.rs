use std::sync::atomic::AtomicBool;

use pinned_downloads::fetch::Memory;
use pinned_downloads::samples::{Entry, tar, xz};

use super::*;

fn sha256(bytes: &[u8]) -> String {
    pinned_downloads::digest::hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

/// A package of `version` as the packaging makes it (`whole`: with its engine.json).
fn package(version: &str, whole: bool) -> (Package, Vec<u8>) {
    let engine = format!(r#"{{"version":"{version}","protocol":1}}"#);
    let program = layout::program(Path::new(""));
    let program = program.to_string_lossy().replace('\\', "/");
    let mut entries = vec![Entry(&program, b'0', 0o755, b"host")];
    if whole {
        entries.push(Entry("engine.json", b'0', 0o644, engine.as_bytes()));
    }
    let bytes = xz(&tar(&entries));
    let url =
        format!("https://github.com/o/r/releases/download/web-engine/engine-{version}.tar.xz");
    let package = Package {
        version: version.into(),
        protocol: 1,
        platform: "macos-arm64".into(),
        url,
        size: bytes.len() as u64,
        sha256: sha256(&bytes),
    };
    (package, bytes)
}

fn install_from(memory: &Memory, root: &Path, package: &Package) -> Result<PathBuf, InstallError> {
    let going = Going {
        fetch: memory,
        progress: &|_, _| {},
        unpacking: &|| {},
        stop: &AtomicBool::new(false),
    };
    install(going, root, package)
}

/// A package downloaded, checked, unpacked into its place and chosen, nothing left in
/// `.staging` but its lock; its program executable.
#[test]
fn a_package_is_installed_whole_and_chosen() {
    let root = pinned_downloads::samples::temp("web-engine-install");
    let (package, bytes) = package("156.0.0.1-1", true);
    let memory = Memory::default();
    memory.serve(&package.url, bytes);
    let dir = install_from(&memory, &root, &package).unwrap();
    assert_eq!(dir, root.join("156.0.0.1-1"));
    assert_eq!(layout::chosen(&root, 1).unwrap().0, dir);
    let staging: Vec<_> = std::fs::read_dir(root.join(layout::STAGING))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name() != layout::STAGING_LOCK)
        .collect();
    assert!(staging.is_empty(), "nothing left but its lock: {staging:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(layout::program(&dir)).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "its program runs");
    }
    // Asked again: there already, nothing downloaded.
    memory.set_offline(true);
    assert_eq!(install_from(&memory, &root, &package).unwrap(), dir);
    std::fs::remove_dir_all(root).unwrap();
}

/// What isn't the pinned package, or isn't whole, is never installed nor chosen.
#[test]
fn a_package_not_pinned_or_not_whole_is_refused() {
    let root = pinned_downloads::samples::temp("web-engine-refused");
    let (mut package, bytes) = package("156.0.0.1-1", true);
    let memory = Memory::default();
    memory.serve(&package.url, bytes);
    package.sha256 = "1".repeat(64);
    let error = install_from(&memory, &root, &package).unwrap_err();
    assert!(matches!(error, InstallError::Fetch(FetchError::Mismatch(_))), "{error}");
    let (package, bytes) = super::tests::package("156.0.0.1-2", false);
    memory.serve(&package.url, bytes);
    let error = install_from(&memory, &root, &package).unwrap_err();
    assert!(matches!(error, InstallError::Incomplete(_)), "{error}");
    assert!(layout::chosen(&root, 1).is_none());
    assert!(!root.join("156.0.0.1-2").exists());
    std::fs::remove_dir_all(root).unwrap();
}

/// The list's place: the project's rolling release, its signature beside it, from the
/// allowed hosts only.
#[test]
fn the_list_comes_from_the_projects_release() {
    let (list, signature) = manifest_urls("owner/name");
    assert_eq!(list, "https://github.com/owner/name/releases/download/web-engine/web-engine.json");
    assert!(signature.ends_with(".json.sig"));
    assert!(fetch::allowed(&list, &HOSTS));
}

/// A package whose address names no plain archive (one that could leave `.staging`) is
/// refused before anything is downloaded.
#[test]
fn a_package_named_outside_its_folder_is_refused() {
    let root = pinned_downloads::samples::temp("web-engine-named");
    let (mut package, bytes) = package("156.0.0.1-1", true);
    package.url = "https://github.com/o/r/releases/download/web-engine/..".into();
    let memory = Memory::default();
    memory.serve(&package.url, bytes);
    let error = install_from(&memory, &root, &package).unwrap_err();
    assert!(matches!(error, InstallError::Manifest(_)), "{error}");
    assert!(memory.asked().is_empty(), "nothing downloaded");
    std::fs::remove_dir_all(root).unwrap();
}

/// A clean while a download is under way (another window's, another instance's) leaves
/// its `.part` alone; broken off, the download resumes from it next time.
#[test]
fn a_clean_never_takes_a_download_under_way() {
    let root = pinned_downloads::samples::temp("web-engine-clean-race");
    let (package, bytes) = package("156.0.0.1-1", true);
    let memory = std::sync::Arc::new(Memory::default());
    memory.serve(&package.url, bytes);
    memory.stall_after(Some(8));
    let downloading = {
        let (memory, root, package) = (memory.clone(), root.clone(), package.clone());
        std::thread::spawn(move || install_from(&memory, &root, &package))
    };
    let part = root
        .join(layout::STAGING)
        .join(format!("156.0.0.1-1.{}.part", "engine-156.0.0.1-1.tar.xz"));
    let since = std::time::Instant::now();
    while std::fs::metadata(&part).map_or(0, |m| m.len()) < 8 {
        assert!(since.elapsed() < std::time::Duration::from_secs(10), "it downloads");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let cleaned = layout::clean(&root);
    assert!(!cleaned.staging && part.exists(), "under way: kept");
    memory.release();
    assert!(downloading.join().unwrap().is_err(), "broken off");
    assert!(part.exists(), "kept to resume");
    memory.stall_after(None);
    memory.forget_asked();
    install_from(&memory, &root, &package).unwrap();
    assert_eq!(memory.asked_from()[0].1, 8, "resumed");
    std::fs::remove_dir_all(root).unwrap();
}

/// A download stopped by the user is resumed at the next run: the clean of the next start
/// keeps what came.
#[test]
fn a_stopped_download_resumes_after_a_restart() {
    let root = pinned_downloads::samples::temp("web-engine-restart");
    let (package, bytes) = package("156.0.0.1-1", true);
    let memory = std::sync::Arc::new(Memory::default());
    memory.serve(&package.url, bytes);
    memory.stall_after(Some(8));
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let downloading = {
        let (memory, root, package, stop) =
            (memory.clone(), root.clone(), package.clone(), stop.clone());
        std::thread::spawn(move || {
            let going =
                Going { fetch: &*memory, progress: &|_, _| {}, unpacking: &|| {}, stop: &stop };
            install(going, &root, &package)
        })
    };
    let part = root
        .join(layout::STAGING)
        .join(format!("156.0.0.1-1.{}.part", "engine-156.0.0.1-1.tar.xz"));
    let since = std::time::Instant::now();
    while std::fs::metadata(&part).map_or(0, |m| m.len()) < 8 {
        assert!(since.elapsed() < std::time::Duration::from_secs(10), "it downloads");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    memory.release();
    assert!(downloading.join().unwrap().is_err(), "stopped");
    // The next run: its start cleans first, then the download is asked again.
    layout::clean(&root);
    assert!(part.exists(), "kept to resume");
    memory.stall_after(None);
    memory.forget_asked();
    install_from(&memory, &root, &package).unwrap();
    assert_eq!(memory.asked_from()[0].1, 8, "resumed");
    std::fs::remove_dir_all(root).unwrap();
}
