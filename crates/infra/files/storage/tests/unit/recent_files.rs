use super::*;

/// Saved relative to the project (those outside it left out), read back absolute, in order.
#[test]
fn recent_files_round_trip_relative_to_the_project() {
    let root = std::env::temp_dir().join(format!("recent-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(load(&root).is_empty(), "never saved");
    let files = [root.join("src/a.php"), PathBuf::from("/elsewhere/b.php"), root.join("c.css")];
    save(&root, &files).unwrap();
    assert_eq!(load(&root), [root.join("src/a.php"), root.join("c.css")]);
    let _ = std::fs::remove_dir_all(&root);
}
