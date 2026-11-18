use super::*;

#[test]
fn every_target_is_marked_and_a_mark_comes_off() {
    let targets = ["var/old-wp".to_owned(), "build".to_owned()];
    let marks = marked(&PathMarks::default(), &targets, Some(PathMark::Excluded));
    assert!(marks.is_excluded("var/old-wp/a.php") && marks.is_excluded("build/x"));
    assert!(marked(&marks, &targets, None).is_empty());
}

#[test]
fn saving_lets_the_file_through_to_version_control_and_keeps_the_rest_out() {
    let root = std::env::temp_dir().join(format!("path-marks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    save(&root, "excluded var\n").unwrap();
    let dir = root.join(kernel::brand::PROJECT_DIR);
    assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), "excluded var\n");
    assert_eq!(std::fs::read_to_string(dir.join(".gitignore")).unwrap(), "*\n!path-marks\n");
    // Another module's line (the diagrams'): kept when the marks are saved again.
    std::fs::write(dir.join(".gitignore"), "*\n!diagrams/\n").unwrap();
    save(&root, "excluded build\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "*\n!diagrams/\n!path-marks\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}
