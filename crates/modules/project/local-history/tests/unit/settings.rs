use super::*;

#[test]
fn a_project_s_history_is_measured_by_its_versions() {
    let dir = std::env::temp_dir().join(format!("history-usage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("0123")).unwrap();
    std::fs::write(dir.join("0123/path"), "src/a.php").unwrap();
    std::fs::write(dir.join("0123/1-s-00-10.gz"), vec![0u8; 100]).unwrap();
    std::fs::write(dir.join("0123/2-s-00-10.gz"), vec![0u8; 50]).unwrap();
    let (bytes, versions) = usage(&dir);
    assert_eq!(versions, 2);
    assert_eq!(bytes, 150 + "src/a.php".len() as u64);
    assert_eq!(usage(&dir.join("none")), (0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the history holds is counted in the background: not known at once, then known, the
/// settings told (`poll`) and the app woken.
#[test]
fn the_history_is_counted_in_the_background() {
    let dir = std::env::temp_dir().join(format!("history-count-{}", std::process::id()));
    let file = dir.join("a.php");
    std::fs::create_dir_all(&file).unwrap();
    std::fs::write(file.join("1.gz"), b"x").unwrap();
    std::fs::write(file.join("2.gz"), b"yy").unwrap();
    let woken = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&woken);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || flag.store(true, Ordering::Release));
    assert_eq!(counted(&dir, Some(&wake)), None, "not counted yet");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !woken.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline, "the count didn't end");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(HistorySettings.poll(), "the settings are told");
    assert_eq!(counted(&dir, None), Some((3, 2)));
    // Another project's history asked about: only it is kept.
    let other = dir.join("elsewhere");
    let _ = counted(&other, None);
    assert!(!USAGE.lock().unwrap().contains_key(&dir), "the first one's figure goes");
    let _ = std::fs::remove_dir_all(&dir);
}
