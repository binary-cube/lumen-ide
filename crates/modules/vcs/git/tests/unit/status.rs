use super::*;

/// Asked, a read is made in the background (once, however many times it was asked), and the
/// project's tab sees what it found.
#[test]
fn the_changes_are_read_once_when_asked() {
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&woken);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    let changes = Arc::new(Changes { wake: Some(wake), ..Changes::default() });
    let repo: Arc<dyn Repository> = Arc::new(crate::fake::FakeRepository::default());
    // Nothing asked: nothing read.
    changes.start(Arc::clone(&repo));
    assert!(!changes.lock().reading);
    changes.stale();
    changes.stale();
    changes.start(Arc::clone(&repo));
    changes.start(repo);
    let started = std::time::Instant::now();
    while changes.files().0 == 0 && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let (generation, files) = changes.files();
    assert_eq!(generation, 1, "one read");
    assert_eq!(files.len(), 2);
    let read = changes.lock();
    assert!(!read.stale && !read.reading);
}

/// While a batch of changes runs, nothing is read; at its end, one read. The project's
/// entry goes with its subscriptions.
#[test]
fn a_batch_of_changes_is_read_once_at_its_end() {
    let root = std::env::temp_dir();
    let events = EventBus::default();
    let id = ProjectId::new();
    let context = ProjectContext {
        marks: Default::default(),
        id,
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: None,
        language_of: Arc::new(|_: &std::path::Path| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let subscriptions = ChangesService.attach(&context);
    let changes = changes(id).unwrap();
    let repo: Arc<dyn Repository> = Arc::new(crate::fake::FakeRepository::default());
    events.publish(&workspace::events::BulkChangeStarted { id: 1 });
    changes.start(Arc::clone(&repo));
    assert!(!changes.lock().reading, "held");
    events.publish(&workspace::events::BulkChangeEnded { id: 1 });
    changes.start(repo);
    let started = std::time::Instant::now();
    while changes.files().0 == 0 && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(changes.files().0, 1);
    drop(subscriptions);
    assert!(super::changes(id).is_none());
}

/// A file open in a three-part merge is marked resolved once it's saved without conflict
/// markers — saved with some left, it stays as it is.
#[test]
fn a_merged_file_saved_without_markers_is_marked_resolved() {
    let root = std::env::temp_dir().join(format!("git-resolve-on-save-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("a.php");
    let events = EventBus::default();
    let id = ProjectId::new();
    let context = ProjectContext {
        marks: Default::default(),
        id,
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: None,
        language_of: Arc::new(|_: &std::path::Path| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let _subscriptions = ChangesService.attach(&context);
    let changes = changes(id).unwrap();
    let fake = Arc::new(crate::fake::FakeRepository {
        root: Some(root.clone()),
        conflicted: std::sync::Mutex::new(vec!["a.php".into()]),
        ..Default::default()
    });
    let repo: Arc<dyn Repository> = Arc::clone(&fake) as Arc<dyn Repository>;
    let resolved = || fake.resolved.lock().unwrap().clone();
    let wait = |done: &dyn Fn() -> bool| {
        let started = std::time::Instant::now();
        while !done() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
    };

    // Saved with a conflict left: nothing marked (and it's no longer followed).
    std::fs::write(&path, "<<<<<<< HEAD\na\n=======\nb\n>>>>>>> x\n").unwrap();
    changes.resolve_on_save(path.clone(), Arc::clone(&repo));
    events.publish(&document::events::FileSaved { path: path.clone() });
    wait(&|| changes.lock().resolving.is_empty());
    std::thread::sleep(Duration::from_millis(50));
    assert!(resolved().is_empty());

    // The merge's result: marked resolved.
    std::fs::write(&path, "a\nb\n").unwrap();
    changes.resolve_on_save(path.clone(), repo);
    events.publish(&document::events::FileSaved { path: path.clone() });
    wait(&|| !resolved().is_empty());
    assert_eq!(resolved(), [vec!["a.php".to_owned()]]);
    // Another save of it isn't followed any more.
    events.publish(&document::events::FileSaved { path });
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(resolved().len(), 1);
    let _ = std::fs::remove_dir_all(root);
}
