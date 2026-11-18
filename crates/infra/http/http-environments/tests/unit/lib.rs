//! The environments found beside a request file.

use super::*;

#[test]
fn the_environments_come_from_the_nearest_folder_with_them() {
    let root = std::env::temp_dir().join(format!("http-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let deep = root.join("api/users");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(root.join("api").join(PUBLIC), r#"{"dev": {"host": "localhost"}}"#).unwrap();
    std::fs::write(root.join("api").join(PRIVATE), r#"{"dev": {"token": "t"}}"#).unwrap();
    let file = deep.join("users.http");
    assert_eq!(folder_for(&file, Some(&root)), root.join("api"), "found above");
    assert_eq!(
        folder_for(&root.join("other/x.http"), Some(&root)),
        root.join("other"),
        "none: its own"
    );
    let environments = of(&file, Some(&root));
    assert_eq!(environments.variable(Some("dev"), "token").map(|v| v.value.as_str()), Some("t"));
    assert_eq!(
        environments.variable(Some("dev"), "host").map(|v| v.value.as_str()),
        Some("localhost")
    );
    let _ = std::fs::remove_dir_all(root);
}
