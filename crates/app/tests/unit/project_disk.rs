use super::*;

/// A file read once is kept until the project's events tell of a change; then read again.
#[test]
fn a_file_is_read_again_only_after_a_change() {
    let reads = Arc::new(DiskReads::default());
    let events = kernel::EventBus::default();
    let _subscriptions = reads.follow(&events);
    let count = std::cell::Cell::new(0);
    let read = |_: &Path| {
        count.set(count.get() + 1);
        Some(format!("v{}", count.get()))
    };
    let path = Path::new("/project/inc.php");
    assert_eq!(reads.file(path, read).as_deref(), Some("v1"));
    assert_eq!(reads.file(path, read).as_deref(), Some("v1"), "kept: not read again");
    assert_eq!(count.get(), 1);
    events.publish(&workspace::events::FilesWritten { paths: vec![PathBuf::from("/project/x")] });
    assert_eq!(reads.file(path, read).as_deref(), Some("v2"), "a change: read again");
    // A folder's entries, the same way.
    let listed = std::cell::Cell::new(0);
    let list = |_: &Path| {
        listed.set(listed.get() + 1);
        vec![("a.php".to_owned(), false)]
    };
    reads.dir(Path::new("/project"), list);
    reads.dir(Path::new("/project"), list);
    assert_eq!(listed.get(), 1);
    events.publish(&workspace::events::EntriesRescan { root: PathBuf::from("/project") });
    reads.dir(Path::new("/project"), list);
    assert_eq!(listed.get(), 2);
    // Purged: read again.
    reads.purge();
    assert_eq!(reads.file(path, read).as_deref(), Some("v3"));
}

/// Under the project, what's read is kept; outside it (no events tell its changes), read
/// each time.
#[test]
fn only_the_project_s_files_are_kept() {
    let dir = std::env::temp_dir().join(format!("project-disk-{}", uuid::Uuid::new_v4()));
    let (inside, outside) = (dir.join("project"), dir.join("elsewhere"));
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let (a, b) = (inside.join("a.php"), outside.join("b.php"));
    std::fs::write(&a, "a1").unwrap();
    std::fs::write(&b, "b1").unwrap();
    let reads = DiskReads::default();
    let none = |_: &Path| None;
    let read = |path: &Path| {
        with_project_kept(Some(&inside), &[], &[], &none, Some(&reads), |p| (p.read)(path))
    };
    assert_eq!(read(&a).as_deref(), Some("a1"));
    assert_eq!(read(&b).as_deref(), Some("b1"));
    std::fs::write(&a, "a2").unwrap();
    std::fs::write(&b, "b2").unwrap();
    let (now_a, now_b) = (read(&a), read(&b));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(now_a.as_deref(), Some("a1"), "kept until an event tells");
    assert_eq!(now_b.as_deref(), Some("b2"), "outside: read each time");
}
