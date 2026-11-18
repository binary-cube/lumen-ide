use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lsp-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Component `name` at `version` installed under `root/group` (its marker, a file).
fn installed(root: &Path, group: &str, name: &str, version: &str, fingerprint: &str) -> PathBuf {
    let dir = path_in(root, group).join(version);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("file"), "x").unwrap();
    let marker = Marker {
        name: name.into(),
        version: version.into(),
        fingerprint: fingerprint.into(),
        size: 1,
        platform: "macos-arm64".into(),
    };
    write_marker(&dir, &marker).unwrap();
    dir
}

fn age(path: &Path, days: u64) {
    let then = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    let mut options = std::fs::File::options();
    options.write(true);
    // Windows opens a folder only with FILE_FLAG_BACKUP_SEMANTICS.
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(&mut options, 0x0200_0000);
    options.open(path).or_else(|_| std::fs::File::open(path)).unwrap().set_modified(then).unwrap();
}

#[test]
fn the_pinned_version_first_else_the_newest_other() {
    let root = temp("find");
    assert_eq!(find(&root, "node", "node", "26.13.0", "fp"), None);
    installed(&root, "node", "node", "24.1.0", "old");
    installed(&root, "node", "node", "25.0.0", "old");
    let found = find(&root, "node", "node", "26.13.0", "fp").unwrap();
    assert_eq!((found.version.as_str(), found.current), ("25.0.0", false));
    let pinned = installed(&root, "node", "node", "26.13.0", "fp");
    let found = find(&root, "node", "node", "26.13.0", "fp").unwrap();
    assert_eq!((found.dir, found.current), (pinned.clone(), true));
    // Another recipe of the version: there, not current (made again).
    assert!(!find(&root, "node", "node", "26.13.0", "another").unwrap().current);
    // A folder without its marker (half removed, half made) is nothing.
    std::fs::remove_file(pinned.join(MARKER)).unwrap();
    assert_eq!(find(&root, "node", "node", "26.13.0", "fp").unwrap().version, "25.0.0");
}

#[test]
fn a_lock_is_held_by_one_at_a_time() {
    let root = temp("lock");
    let first = Lock::acquire(&root, "vtsls", &AtomicBool::new(false), &|| {}).unwrap();
    assert!(Lock::try_acquire(&root, "vtsls").is_none());
    assert!(Lock::try_acquire(&root, "node").is_some(), "another component's isn't");
    let waited = Arc::new(AtomicBool::new(false));
    let told = Arc::clone(&waited);
    let at = root.clone();
    let second = std::thread::spawn(move || {
        let _lock = Lock::acquire(&at, "vtsls", &AtomicBool::new(false), &|| {
            told.store(true, Ordering::Relaxed);
        })
        .unwrap();
        std::time::Instant::now()
    });
    std::thread::sleep(Duration::from_millis(300));
    let released = std::time::Instant::now();
    drop(first);
    let got = second.join().unwrap();
    assert!(waited.load(Ordering::Relaxed), "the second one said it waits");
    assert!(got >= released, "the second one got it only once the first let go");
    // Given up when stopped.
    let _held = Lock::acquire(&root, "php", &AtomicBool::new(false), &|| {}).unwrap();
    let error = Lock::acquire(&root, "php", &AtomicBool::new(true), &|| {}).unwrap_err();
    assert_eq!(error, "stopped");
}

#[test]
fn the_cleanup_keeps_what_is_used_here_and_by_other_versions_lately() {
    let root = temp("collect");
    let ours = installed(&root, "servers/vtsls", "vtsls", "0.3.0", "fp");
    let old = installed(&root, "servers/vtsls", "vtsls", "0.2.0", "fp");
    let theirs = installed(&root, "node", "node", "27.0.0", "fp");
    let forgotten = installed(&root, "node", "node", "20.0.0", "fp");
    let gone_server = installed(&root, "servers/retired", "retired", "1.0.0", "fp");
    // Another app version uses node 27, seen now; one seen 40 days ago used node 20.
    write_refs(&root, "0.9.0-aaaa", &BTreeSet::from(["node/27.0.0".to_owned()])).unwrap();
    write_refs(&root, "0.1.0-bbbb", &BTreeSet::from(["node/20.0.0".to_owned()])).unwrap();
    age(&root.join("refs/0.1.0-bbbb.toml"), 40);
    // A temporary folder left by a crash two days ago, one being made now; a part of a week
    // and a day, one of yesterday.
    let crashed = root.join("servers/vtsls/.tmp-0.3.0-dead");
    let making = root.join("servers/vtsls/.tmp-0.3.0-live");
    std::fs::create_dir_all(&crashed).unwrap();
    std::fs::create_dir_all(&making).unwrap();
    age(&crashed, 2);
    std::fs::create_dir_all(root.join(".downloads")).unwrap();
    std::fs::write(root.join(".downloads/old.part"), "x").unwrap();
    std::fs::write(root.join(".downloads/new.part"), "x").unwrap();
    age(&root.join(".downloads/old.part"), 8);
    // A component being installed by another process is never touched.
    let busy = installed(&root, "php", "php", "8.3.0", "fp");
    let lock = Lock::acquire(&root, "php", &AtomicBool::new(false), &|| {}).unwrap();
    let keep = BTreeSet::from(["servers/vtsls/0.3.0".to_owned()]);
    let removed = collect(&root, &keep, "0.8.7-cccc", SystemTime::now());
    drop(lock);
    assert!(ours.is_dir() && theirs.is_dir() && busy.is_dir() && making.is_dir());
    assert!(!old.exists() && !forgotten.exists() && !gone_server.exists() && !crashed.exists());
    assert!(!root.join("servers/retired").exists(), "a server's empty folder goes too");
    assert!(!root.join("refs/0.1.0-bbbb.toml").exists());
    assert!(root.join("refs/0.9.0-aaaa.toml").exists());
    assert!(!root.join(".downloads/old.part").exists());
    assert!(root.join(".downloads/new.part").exists());
    assert!(removed.contains(&old), "{removed:?}");
}

#[test]
fn a_folder_goes_marker_first() {
    let root = temp("remove");
    let dir = installed(&root, "node", "node", "1.0.0", "fp");
    remove_dir(&dir).unwrap();
    assert!(!dir.exists());
    remove_dir(&dir).unwrap();
}
