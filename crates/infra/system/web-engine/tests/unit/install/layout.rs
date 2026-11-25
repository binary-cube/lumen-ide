use super::*;

/// How long a lock let go may still read as held (another test's process starting).
const LOCK_LET_GO: std::time::Duration = std::time::Duration::from_secs(2);

/// The app's version judging the starts.
const APP: &str = "1.0.0";

fn temp(name: &str) -> PathBuf {
    pinned_downloads::samples::temp(&format!("web-engine-{name}"))
}

/// An engine unpacked as a package lays it out.
pub(crate) fn unpack_engine(root: &Path, version: &str, protocol: u32) -> PathBuf {
    let dir = root.join(version);
    let program = program(&dir);
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::write(&program, b"host").unwrap();
    let engine = serde_json::to_vec(&EngineInfo { version: version.into(), protocol }).unwrap();
    std::fs::write(dir.join(ENGINE), engine).unwrap();
    dir
}

/// Chosen atomically; the one before kept until the new one worked; two failed starts give
/// the new one up and bring the one before back; another protocol is never chosen.
#[test]
fn a_version_is_chosen_kept_until_it_works_and_given_up_if_it_fails() {
    let root = temp("state");
    unpack_engine(&root, "156.0.0.1-1", 1);
    unpack_engine(&root, "156.0.0.1-2", 1);
    choose(&root, "156.0.0.1-1").unwrap();
    assert_eq!(chosen(&root, 1).unwrap().1.version, "156.0.0.1-1");
    assert!(chosen(&root, 2).is_none(), "another protocol");
    choose(&root, "156.0.0.1-2").unwrap();
    assert_eq!(read_state(&root).previous.as_deref(), Some("156.0.0.1-1"));
    assert!(!failed(&root, "156.0.0.1-2", APP).unwrap(), "once");
    assert!(failed(&root, "156.0.0.1-2", APP).unwrap(), "twice: given up");
    let state = read_state(&root);
    assert_eq!(state.current.as_deref(), Some("156.0.0.1-1"));
    assert_eq!(state.refused(APP), ["156.0.0.1-2".to_owned()]);
    assert!(state.refused("2.0.0").is_empty(), "a newer app tries it again");
    // A new one that works: the one before isn't needed.
    choose(&root, "156.0.0.1-2").unwrap();
    worked(&root, "156.0.0.1-2").unwrap();
    assert_eq!(read_state(&root).previous, None);
    // A temporary file left by a break doesn't change the choice.
    std::fs::write(root.join("state.json.1.tmp"), b"{\"current\":\"other\"}").unwrap();
    assert_eq!(read_state(&root).current.as_deref(), Some("156.0.0.1-2"));
    std::fs::remove_dir_all(root).unwrap();
}

/// Cleaning lets go the versions neither chosen nor kept, unless a host runs from them (or
/// it can't be told); what a break left in `.staging`; never the profile or the state; and
/// nothing at all while a download is under way.
#[test]
fn cleaning_lets_go_only_what_isnt_needed() {
    let root = temp("clean");
    for version in ["1.0.0.0-1", "1.0.0.0-2", "1.0.0.0-3", "1.0.0.0-4", "1.0.0.0-5"] {
        unpack_engine(&root, version, 1);
    }
    choose(&root, "1.0.0.0-3").unwrap();
    choose(&root, "1.0.0.0-4").unwrap();
    std::fs::create_dir_all(profile(&root).join("Local Storage")).unwrap();
    std::fs::create_dir_all(root.join(STAGING).join("x")).unwrap();
    // A host runs from -2; -5's hold can't be told (not a file).
    let lock = std::fs::File::create(root.join("1.0.0.0-2").join(IN_USE)).unwrap();
    lock.lock().unwrap();
    std::fs::create_dir_all(root.join("1.0.0.0-5").join(IN_USE)).unwrap();
    // A download under way (any instance's: it holds `.staging`): nothing is touched.
    let downloading = try_hold_staging(&root).unwrap().expect("free");
    assert_eq!(clean(&root), Cleaned::default(), "a download under way");
    assert!(root.join("1.0.0.0-1").exists() && root.join(STAGING).join("x").exists());
    drop(downloading);
    let cleaned = clean(&root);
    assert_eq!(cleaned.versions, vec!["1.0.0.0-1".to_owned()]);
    assert!(cleaned.staging);
    drop(lock);
    // A process another test starts meanwhile holds a copy of the lock's file between its
    // fork and its exec (a lock taken so stays while any copy is open): -2 then still reads
    // as in use, as it should, for that moment.
    let started = std::time::Instant::now();
    let cleaned = loop {
        let cleaned = clean(&root);
        if !cleaned.versions.is_empty() || started.elapsed() > LOCK_LET_GO {
            break cleaned;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(cleaned.versions, vec!["1.0.0.0-2".to_owned()]);
    for kept in ["1.0.0.0-3", "1.0.0.0-4", "1.0.0.0-5", PROFILE, STATE] {
        assert!(root.join(kept).exists(), "{kept} kept");
    }
    std::fs::remove_dir_all(root).unwrap();
}

/// A version moved into its place but not chosen yet (its download still holds `.staging`)
/// is never taken for one nobody chose, even by a clean of another window at that moment.
#[test]
fn a_version_being_put_in_place_is_never_cleaned() {
    let root = temp("clean-choose");
    unpack_engine(&root, "1.0.0.0-1", 1);
    choose(&root, "1.0.0.0-1").unwrap();
    worked(&root, "1.0.0.0-1").unwrap();
    let downloading = try_hold_staging(&root).unwrap().expect("free");
    // Moved into place, not chosen yet.
    unpack_engine(&root, "1.0.0.0-2", 1);
    let cleaning = {
        let root = root.clone();
        std::thread::spawn(move || clean(&root))
    };
    assert!(cleaning.join().unwrap().versions.is_empty(), "nothing while it's held");
    choose(&root, "1.0.0.0-2").unwrap();
    drop(downloading);
    assert!(clean(&root).versions.is_empty(), "both chosen: current and previous");
    assert!(root.join("1.0.0.0-2").exists() && root.join("1.0.0.0-1").exists());
    std::fs::remove_dir_all(root).unwrap();
}

/// A download broken off is kept by the clean of the next start (to be resumed) while it's
/// young, let go once it's older than [`PART_KEPT`]; what was being unpacked always goes.
#[test]
fn a_broken_download_is_kept_to_be_resumed() {
    let root = temp("clean-parts");
    let staging = root.join(STAGING);
    std::fs::create_dir_all(staging.join("1.0.0.0-2")).unwrap();
    std::fs::write(staging.join("1.0.0.0-2.engine.tar.xz.part"), b"half").unwrap();
    let old = staging.join("1.0.0.0-1.engine.tar.xz.part");
    std::fs::write(&old, b"old").unwrap();
    let long_ago = std::time::SystemTime::now() - PART_KEPT - Duration::from_secs(60);
    std::fs::File::options().write(true).open(&old).unwrap().set_modified(long_ago).unwrap();
    assert!(clean(&root).staging);
    assert!(staging.join("1.0.0.0-2.engine.tar.xz.part").exists(), "young: kept");
    assert!(!old.exists(), "old: let go");
    assert!(!staging.join("1.0.0.0-2").exists(), "what was being unpacked goes");
    std::fs::remove_dir_all(root).unwrap();
}

/// A state naming anything but a version (a path) chooses nothing.
#[test]
fn a_state_naming_no_version_chooses_nothing() {
    let root = temp("hostile-state");
    unpack_engine(&root, "156.0.0.1-1", 1);
    std::fs::create_dir_all(root.join("sub")).unwrap();
    for current in ["../156.0.0.1-1", "sub/../156.0.0.1-1", "/etc"] {
        update_state(&root, |s| s.current = Some(current.to_owned())).unwrap();
        assert!(chosen(&root, 1).is_none(), "{current}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

/// Changes from many threads at once (as from two windows, two instances) are all kept:
/// each reads and writes the state while holding its lock.
#[test]
fn changes_at_once_are_all_kept() {
    let root = temp("state-race");
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let root = root.clone();
            std::thread::spawn(move || {
                for i in 0..10 {
                    update_state(&root, |s| s.bad.push(format!("{n}.{i}"))).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(read_state(&root).bad.len(), 80);
    std::fs::remove_dir_all(root).unwrap();
}

/// A second download waits while one holds `.staging` (stopped meanwhile: it gives up), and
/// gets it once that one is done.
#[test]
fn one_download_at_a_time() {
    let root = temp("staging-hold");
    let first = try_hold_staging(&root).unwrap().expect("free");
    assert!(try_hold_staging(&root).unwrap().is_none(), "held");
    let stop = AtomicBool::new(true);
    assert!(hold_staging(&root, &stop).unwrap().is_none(), "stopped while waiting");
    let waiting = {
        let root = root.clone();
        std::thread::spawn(move || hold_staging(&root, &AtomicBool::new(false)).unwrap().is_some())
    };
    std::thread::sleep(Duration::from_millis(50));
    drop(first);
    assert!(waiting.join().unwrap(), "had it once let go");
    std::fs::remove_dir_all(root).unwrap();
}
