use super::*;

#[test]
fn round_trip_keeps_other_keys() {
    let dir = std::env::temp_dir().join(format!("levels-{}", std::process::id()));
    let _ = std::fs::create_dir_all(dir.join(kernel::brand::PROJECT_DIR));
    let _ = std::fs::write(file(&dir), "[other]\nkeep = true\n");
    let saved = SavedLevels {
        levels: vec![("php".into(), "8.3".into())],
        files: vec![("legacy/a.php".into(), "php".into(), "7.4".into())],
    };
    let store = TomlProjectLevelsStore;
    let result = store.save(&dir, &saved);
    let loaded = store.load(&dir);
    let text = std::fs::read_to_string(file(&dir)).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.is_ok());
    assert_eq!(loaded, saved);
    assert!(text.contains("keep = true"), "{text}");
}
