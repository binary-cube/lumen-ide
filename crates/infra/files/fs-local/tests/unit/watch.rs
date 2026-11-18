use super::*;

/// Held by the tests that start real watchers: the one counting registrations sees only its
/// own.
static REAL_WATCHERS: Mutex<()> = Mutex::new(());

/// Paths heard are kept once each and taken sorted; too many (a checkout) are let go: the
/// caller looks at everything again instead.
#[test]
fn heard_paths_are_kept_once_up_to_a_limit() {
    let heard = Heard::default();
    assert!(heard.add(PathBuf::from("/p/b")));
    assert!(heard.add(PathBuf::from("/p/a")));
    assert!(heard.add(PathBuf::from("/p/b")));
    assert_eq!(heard.take(), [PathBuf::from("/p/a"), PathBuf::from("/p/b")]);
    assert!(heard.take().is_empty());
    for i in 0..KEPT {
        assert!(heard.add(PathBuf::from(format!("/p/{i}"))));
    }
    assert!(!heard.add(PathBuf::from("/p/more")), "too many");
    assert!(heard.take().is_empty(), "let go");
}

/// Registrations take turns; a project closed before its turn is never registered, and the
/// next one still goes live.
#[test]
fn a_watcher_closed_before_its_turn_is_never_registered() {
    let _alone = REAL_WATCHERS.lock().unwrap_or_else(PoisonError::into_inner);
    let dir = std::env::temp_dir().join(format!("tree-watch-turns-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::create_dir_all(dir.join("b")).unwrap();
    let dir = dir.canonicalize().unwrap();
    // macOS's FSEvents can take ten seconds and more to start a stream while `fseventsd` is
    // busy (a build churning `target/`): the deadline only stops a watcher that never comes.
    let live = |w: &TreeWatcher| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        while std::time::Instant::now() < deadline {
            if w.take_rescan() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    };
    let turn = REGISTERING.lock().unwrap();
    let before = REGISTERED.load(Ordering::SeqCst);
    let closed = TreeWatcher::start(&dir.join("a"), WatchedDirs::default(), Arc::new(|| {}));
    drop(closed);
    let open = TreeWatcher::start(&dir.join("b"), WatchedDirs::default(), Arc::new(|| {})).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!open.take_rescan(), "not live before its turn");
    drop(turn);
    assert!(live(&open), "live once its turn comes");
    assert_eq!(REGISTERED.load(Ordering::SeqCst) - before, 1, "only the open one registered");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Too many paths, again and again (a long install): one look at everything, once the
/// events stop, not one per overflow.
#[test]
fn a_storm_of_paths_is_looked_at_once_when_it_ends() {
    let storm = Arc::new(Storm::new());
    let looks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&looks);
    let look: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    for _ in 0..50 {
        storm.heard(Arc::clone(&look));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(looks.load(Ordering::SeqCst), 0, "not while it lasts");
    let started = std::time::Instant::now();
    while looks.load(Ordering::SeqCst) == 0 && started.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(QUIET);
    assert_eq!(looks.load(Ordering::SeqCst), 1, "once, after it");
}

/// Our saves (a temporary file renamed over the file) are heard as the file written, never
/// as entries created or removed, however late the system reports them and with whatever
/// it gathered on the way (macOS: "created", "renamed", the folder "created" too); a file
/// made by another program, or put in our file's place since, still is one. The events are
/// given as macOS reports a save: through the system, a busy disk loses them without a word
/// (measured: 1 of 40 heard beside a build), which would test only the machine's load.
#[test]
fn our_saves_are_heard_as_written_not_as_new_entries() {
    use document::DocumentStorage as _;
    use notify::event::{CreateKind, DataChange, RenameMode};
    let dir = std::env::temp_dir().join(format!(
        "tree-watch-saves-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let file = dir.join("a.php");
    std::fs::write(&file, "<?php").unwrap();
    let (watcher, hear) = TreeWatcher::hearing(&dir, WatchedDirs::default(), Arc::new(|| {}));
    let event = |kind: EventKind, path: &Path| Ok(notify::Event::new(kind).add_path(path.into()));
    let disk = crate::LocalFileSystem;
    for i in 0..3 {
        disk.write(&file, format!("<?php // {i}").as_bytes()).unwrap();
    }
    let tmp = dir.join(crate::own_saves::temporary_name("a.php", 0));
    hear(event(EventKind::Create(CreateKind::File), &tmp));
    hear(event(EventKind::Modify(ModifyKind::Data(DataChange::Content)), &tmp));
    hear(event(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), &tmp));
    for _ in 0..3 {
        hear(event(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), &file));
    }
    // What the system gathered on the file and its folder since they were made.
    hear(event(EventKind::Create(CreateKind::File), &file));
    hear(event(EventKind::Create(CreateKind::Folder), &dir));
    assert_eq!(watcher.drain_written(), vec![file.clone()], "the file written, nothing else");
    assert_eq!(watcher.drain_changed(), Vec::<PathBuf>::new(), "no entry created or removed");
    // Another program's new file is an entry.
    let other = dir.join("b.php");
    std::fs::write(&other, "<?php").unwrap();
    hear(event(EventKind::Create(CreateKind::File), &other));
    assert_eq!(watcher.drain_changed(), vec![other]);
    // Another file put in ours' place since (another program's): an entry change — told
    // by the files' identities (inodes); Windows has none here, and within the short
    // window takes it as ours.
    if cfg!(unix) {
        std::fs::rename(&file, dir.join("aside.php")).unwrap();
        std::fs::write(&file, "<?php // theirs").unwrap();
        hear(event(EventKind::Create(CreateKind::File), &file));
        assert_eq!(watcher.drain_changed(), vec![file]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
