use super::*;

#[test]
fn round_trip_keeps_other_keys() {
    let dir = std::env::temp_dir().join(format!("frameworks-{}", std::process::id()));
    let _ = std::fs::create_dir_all(dir.join(kernel::brand::PROJECT_DIR));
    let _ = std::fs::write(file(&dir), "[levels]\nphp = \"8.3\"\n");
    let store = TomlProjectFrameworksStore;
    let choices = vec![("laravel".to_owned(), true), ("htmx".to_owned(), false)];
    let result = store.save(&dir, &choices);
    let mut loaded = store.load(&dir);
    loaded.sort();
    let text = std::fs::read_to_string(file(&dir)).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.is_ok());
    assert_eq!(loaded, [("htmx".to_owned(), false), ("laravel".to_owned(), true)]);
    assert!(text.contains("php = \"8.3\""), "{text}");
}
