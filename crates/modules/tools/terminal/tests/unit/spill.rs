use super::*;

/// A folder of the test's own.
fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("spill-{name}-{:032x}", kernel::id::new_uuid()))
}

/// Written and taken back the same; the file holds nothing readable and goes once read; a
/// token never names a path elsewhere.
#[test]
fn a_history_comes_back_the_same_and_is_unreadable_on_disk() {
    let root = temp("round");
    let store = DiskSpill::new(root.join(kernel::brand::PROJECT_DIR).join(FOLDER), true);
    let history = "drwxr-xr-x  src/module.rs  a line of history\n".repeat(2_000);
    let token = store.write(history.as_bytes()).unwrap();
    let file = store.dir().join(format!("{}.bin", token.0));
    let on_disk = std::fs::read(&file).unwrap();
    assert!(on_disk.len() < history.len() / 4, "compressed: {} bytes", on_disk.len());
    assert!(!on_disk.windows(9).any(|w| w == b"src/modul"), "sealed");
    assert_eq!(store.take(&token).as_deref(), Some(history.as_bytes()));
    assert!(!file.exists(), "read once, then gone");
    assert_eq!(store.take(&token), None);
    // Never in a commit.
    let data = root.join(kernel::brand::PROJECT_DIR);
    assert_eq!(std::fs::read_to_string(data.join(".gitignore")).unwrap(), "*\n");
    assert_eq!(std::fs::read_to_string(data.join(FOLDER).join(".gitignore")).unwrap(), "*\n");
    // Removed unread.
    let token = store.write(b"x").unwrap();
    store.remove(&token);
    assert_eq!(store.take(&token), None);
    assert_eq!(store.take(&SpillToken("../../etc/passwd".into())), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// A lock file made long enough ago for a run left behind.
fn aged(file: &Path) {
    let old = std::time::SystemTime::now() - LEFT_AFTER * 2;
    std::fs::File::options().write(true).open(file).unwrap().set_modified(old).unwrap();
}

/// Another store's folder no app holds (its lock file there, free and old) is deleted the
/// first time a store writes; one still held, one just made, one without a lock file yet (an
/// app making it), and files that aren't stores' stay.
#[test]
fn folders_left_by_runs_that_ended_are_deleted() {
    let base = temp("left");
    let folder = |name: &str| {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    };
    let left = folder("run-0000");
    std::fs::write(left.join(LOCK_FILE), b"").unwrap();
    aged(&left.join(LOCK_FILE));
    std::fs::write(left.join("x.bin"), b"old").unwrap();
    let held = folder("run-1111");
    let lock = std::fs::File::create(held.join(LOCK_FILE)).unwrap();
    lock.try_lock().unwrap();
    aged(&held.join(LOCK_FILE));
    let fresh = folder("run-2222");
    std::fs::write(fresh.join(LOCK_FILE), b"").unwrap();
    let making = folder("run-3333");
    std::fs::write(base.join("notes.txt"), b"keep").unwrap();
    let store = DiskSpill::new(base.clone(), false);
    assert!(store.write(b"history").is_some());
    assert!(!left.exists(), "nobody holds it: gone");
    assert!(held.exists(), "another app holds it");
    assert!(fresh.exists(), "just made: maybe not locked yet");
    assert!(making.exists(), "no lock file yet: being made");
    assert!(base.join("notes.txt").exists());
    assert!(store.dir().join(LOCK_FILE).exists());
    drop(lock);
    let _ = std::fs::remove_dir_all(&base);
}

/// A folder that couldn't be made is tried again later (not given up for the run); a
/// store let go deletes its folder.
#[test]
fn a_store_retries_and_cleans_up_after_itself() {
    let base = temp("retry");
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    std::fs::write(&base, b"a file where the folder goes").unwrap();
    let store = DiskSpill::new(base.clone(), false);
    assert!(store.ready(), "not tried yet: ready");
    assert!(store.write(b"history").is_none(), "can't be made");
    assert!(!store.ready(), "failed lately: not ready (nothing encoded for it)");
    std::fs::remove_file(&base).unwrap();
    assert!(store.write(b"history").is_none(), "not tried again at once");
    *store.ready.lock().unwrap() = Ready::Failed(std::time::Instant::now() - RETRY_AFTER);
    assert!(store.ready());
    assert!(store.write(b"history").is_some(), "tried again later");
    let dir = store.dir();
    assert!(dir.exists());
    drop(store);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while dir.exists() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!dir.exists(), "let go: its folder deleted (on a worker)");
    let _ = std::fs::remove_dir_all(&base);
}

/// A project closed: its store let go (a new one made if it opens again).
#[test]
fn a_closed_project_s_store_is_let_go() {
    let root = temp("release");
    let first = store(Some(&root));
    assert!(Arc::ptr_eq(&first, &store(Some(&root))), "one per project");
    release(&root);
    assert!(!Arc::ptr_eq(&first, &store(Some(&root))), "let go");
    release(&root);
}

/// The project's data folder's `.gitignore`, written by another module with what it lets
/// through: kept as it is.
#[test]
fn another_module_s_ignore_lines_are_kept() {
    let root = temp("ignore");
    let data = root.join(kernel::brand::PROJECT_DIR);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join(".gitignore"), "*\n!diagrams/\n!diagrams/**\n").unwrap();
    let store = DiskSpill::new(data.join(FOLDER), true);
    assert!(store.write(b"history").is_some());
    let kept = std::fs::read_to_string(data.join(".gitignore")).unwrap();
    assert_eq!(kept, "*\n!diagrams/\n!diagrams/**\n");
    assert_eq!(std::fs::read_to_string(data.join(FOLDER).join(".gitignore")).unwrap(), "*\n");
    let _ = std::fs::remove_dir_all(&root);
}
