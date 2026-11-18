use super::*;

fn built(dir: &Path, events: &EventBus) -> Paths {
    let paths = Paths::new(dir, events, Arc::new(|| {}), Default::default());
    assert!(paths.ensure());
    assert!(paths.first_build());
    let started = Instant::now();
    while !paths.take_built() {
        assert!(started.elapsed() < Duration::from_secs(10), "the index wasn't built");
        std::thread::sleep(Duration::from_millis(5));
    }
    paths
}

/// One index answers the path search and the filtered listings; it rebuilds only when the
/// project's events say it's out of date, and forgets everything when cleared.
#[test]
fn paths_listings_and_rebuilds() {
    // The folders the languages' modules register (Composer's, npm's, the builds').
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    let dir = std::env::temp_dir().join(format!("project-paths-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("vendor/x")).unwrap();
    std::fs::write(dir.join("src/User.php"), "").unwrap();
    std::fs::write(dir.join("src/app.js"), "").unwrap();
    std::fs::write(dir.join("vendor/x/Lib.php"), "").unwrap();
    let events = EventBus::new();
    let paths = built(&dir, &events);
    assert!(!paths.first_build());
    assert!(!paths.take_built(), "taken once");

    let found = paths.find(&PathQuery { text: "user", folders: 3, files: 3 });
    assert_eq!(found.files.first().map(|f| f.path.as_str()), Some("src/User.php"));
    let filter = workspace::FileFilter::code(&["php"]);
    let all = paths.peek().unwrap();
    let php: Vec<&String> = all.iter().filter(|f| filter.accepts(f)).collect();
    assert_eq!(php, ["src/User.php"], "the project's own PHP, no vendor");

    // Up to date: no rebuild; out of date (an entry made, a batch of changes ended): one.
    assert!(!paths.ensure());
    events.publish(&EntriesChanged { paths: vec![dir.join("src/New.php")] });
    assert!(paths.ensure());
    while paths.first_build() || !paths.take_built() {
        std::thread::sleep(Duration::from_millis(5));
    }
    events.publish(&BulkChangeEnded { id: 1 });
    assert!(paths.ensure());
    // Reindexed from scratch: forgotten (built again when asked).
    events.publish(&ReindexRequested { root: dir.clone() });
    assert!(paths.peek().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
