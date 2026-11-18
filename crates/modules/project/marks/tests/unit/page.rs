use super::*;

const GB: u64 = 1 << 30;

fn held(entries: &[(&str, u64, u64)]) -> HashMap<String, Held> {
    entries
        .iter()
        .map(|(p, files, bytes)| ((*p).to_owned(), Held { files: *files, bytes: *bytes }))
        .collect()
}

#[test]
fn the_deepest_folder_holding_most_of_it_is_what_is_suggested() {
    let found =
        held(&[("var", 30_000, 27 * GB), ("var/old-wp", 25_000, 24 * GB), ("src", 900, GB / 100)]);
    let names: Vec<String> =
        suggestions(&found, &PathMarks::default()).into_iter().map(|(p, _)| p).collect();
    assert_eq!(names, ["var/old-wp"]);
}

#[test]
fn what_is_marked_and_libraries_are_not_suggested() {
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        ..Default::default()
    });
    let found = held(&[("vendor", 50_000, 2 * GB), ("old", 1, 3 * GB)]);
    let mut marks = PathMarks::default();
    marks.set("old", Some(PathMark::Normal));
    assert!(suggestions(&found, &marks).is_empty());
}

#[test]
fn folders_are_counted_with_what_is_marked_deep_down() {
    let root = std::env::temp_dir().join(format!("path-marks-count-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let deep = root.join("a/b/c/d");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("one.txt"), "12345").unwrap();
    std::fs::write(root.join("a/two.txt"), "1").unwrap();
    let mut marks = PathMarks::default();
    marks.set("a/b/c/d", Some(PathMark::Excluded));
    let held = count(&root, &marks, Instant::now() + Duration::from_secs(5));
    assert_eq!(held["a"], Held { files: 2, bytes: 6 });
    assert_eq!(held["a/b/c/d"], Held { files: 1, bytes: 5 });
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_folder_says_its_memory_only_when_the_index_holds_some() {
    let held = Some(Held { files: 2, bytes: 2048 });
    let plain = held_text(held);
    assert_eq!(described(held, None), plain);
    assert_eq!(described(held, Some(&0)), plain);
    let with = described(held, Some(&(5 * 1024 * 1024)));
    assert!(with.starts_with(&plain) && with.len() > plain.len(), "{with}");
}

#[test]
fn marks_written_by_hand_are_followed_and_our_own_change_nothing() {
    let dir = std::env::temp_dir().join(format!("marks-follow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("path-marks");
    let (marks, events) = (ProjectMarks::default(), kernel::EventBus::default());
    let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&told);
    let _heard = events.subscribe(move |_: &workspace::events::PathMarksChanged| {
        count.fetch_add(1, Ordering::SeqCst);
    });
    std::fs::write(&file, "excluded vendor\n").unwrap();
    assert!(follow_file(&file, &marks, &events));
    assert!(marks.get().is_excluded("vendor/a.php"));
    assert_eq!(told.load(Ordering::SeqCst), 1);
    assert!(!follow_file(&file, &marks, &events), "the same again: nothing to tell");
    assert_eq!(told.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}
