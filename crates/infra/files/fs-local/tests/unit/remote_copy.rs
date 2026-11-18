use super::*;

#[test]
fn the_local_copy_lists_writes_and_removes_files() {
    let root = std::env::temp_dir().join(format!("remote-copy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let copy = LocalCopy { root: root.clone(), skip: vec![".data".into()] };
    copy.write("app/a.php", b"<?php").unwrap();
    copy.write(".data/x", b"skipped").unwrap();
    let listed: Vec<String> = copy.list().into_iter().map(|e| e.path).collect();
    assert_eq!(listed, ["app/a.php"]);
    assert_eq!(copy.read("app/a.php").unwrap(), b"<?php");
    assert_eq!(copy.stat("app/a.php").map(|e| e.size), Some(5));
    copy.remove("app/a.php").unwrap();
    assert!(copy.stat("app/a.php").is_none());
    let _ = std::fs::remove_dir_all(root);
}

/// A link in the copy leads somewhere that isn't the project: never listed (so never
/// uploaded), never read from nor written through.
#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    let base = std::env::temp_dir().join(format!("remote-copy-links-{}", std::process::id()));
    let (root, secret) = (base.join("project"), base.join("private"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&secret).unwrap();
    std::fs::write(secret.join("id_rsa"), "key").unwrap();
    std::fs::write(root.join("index.php"), "<?php").unwrap();
    std::os::unix::fs::symlink(&secret, root.join("shared")).unwrap();
    std::os::unix::fs::symlink(secret.join("id_rsa"), root.join("key")).unwrap();
    let copy = LocalCopy { root, skip: Vec::new() };
    let listed: Vec<String> = copy.list().into_iter().map(|e| e.path).collect();
    assert_eq!(listed, ["index.php"]);
    assert!(copy.read("shared/id_rsa").is_err());
    assert!(copy.read("key").is_err());
    assert!(copy.write("shared/planted", b"x").is_err());
    assert!(!secret.join("planted").exists());
    let _ = std::fs::remove_dir_all(base);
}
