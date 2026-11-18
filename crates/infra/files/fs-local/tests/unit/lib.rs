use super::*;

#[test]
fn lists_files_and_directories() {
    let dir = std::env::temp_dir().join(format!("fs-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.php"), "<?php").unwrap();

    let mut entries = LocalFileSystem.read_dir(&dir).unwrap();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        entries,
        vec![DirEntry::new("a.php", NodeKind::File), DirEntry::new("sub", NodeKind::Directory)]
    );
    assert_eq!(LocalFileSystem.read_file(&dir.join("a.php")).unwrap(), b"<?php");
    assert_eq!(LocalFileSystem.kind(&dir.join("a.php")), Some(NodeKind::File));
    assert_eq!(LocalFileSystem.kind(&dir.join("sub")), Some(NodeKind::Directory));
    assert_eq!(LocalFileSystem.kind(&dir.join("none")), None);
    assert_eq!(LocalFileSystem.stamp(&dir.join("a.php")).map(|(len, _)| len), Some(5));
    assert_eq!(LocalFileSystem.stamp(&dir.join("none")), None);
    assert!(LocalFileSystem.canonical(&dir.join("sub/../a.php")).unwrap().ends_with("a.php"));

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn atomic_write_replaces_contents_and_leaves_no_temp_file() {
    let dir = std::env::temp_dir().join(format!("save-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("x.php");
    std::fs::write(&file, "old").unwrap();

    DocumentStorage::write(&LocalFileSystem, &file, b"new").unwrap();

    assert_eq!(std::fs::read(&file).unwrap(), b"new");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn folders_are_copied_and_moved_whole() {
    let dir = std::env::temp_dir().join(format!("fs-copy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/sub")).unwrap();
    std::fs::write(dir.join("src/a.txt"), "a").unwrap();
    std::fs::write(dir.join("src/sub/b.txt"), "b").unwrap();
    assert_eq!(measure(&dir.join("src")), (2, 2));
    let mut seen = 0;
    copy_recursive_with(&dir.join("src"), &dir.join("copy"), &mut |b| seen += b, &|| false)
        .unwrap();
    assert_eq!(seen, 2);
    assert_eq!(std::fs::read_to_string(dir.join("copy/sub/b.txt")).unwrap(), "b");
    assert!(dir.join("src/a.txt").exists(), "a copy leaves the original");
    move_entry(&dir.join("copy"), &dir.join("moved")).unwrap();
    assert!(!dir.join("copy").exists() && dir.join("moved/a.txt").exists());
    assert!(move_entry(&dir.join("src"), &dir.join("moved")).is_err(), "not over an existing one");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link to a parent folder is copied and measured as a link: never followed (it would
/// never end).
#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    let dir = std::env::temp_dir().join(format!("fs-local-links-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let from = dir.join("from");
    std::fs::create_dir_all(from.join("sub")).unwrap();
    std::fs::write(from.join("sub/a.txt"), "abc").unwrap();
    std::os::unix::fs::symlink("..", from.join("sub/up")).unwrap();
    let (files, _) = measure(&from);
    assert_eq!(files, 2, "the file and the link");
    let to = dir.join("to");
    copy_recursive(&from, &to).unwrap();
    assert_eq!(std::fs::read_to_string(to.join("sub/a.txt")).unwrap(), "abc");
    assert!(std::fs::symlink_metadata(to.join("sub/up")).unwrap().file_type().is_symlink());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Saving through a link writes the file it points to: the link stays a link.
#[cfg(unix)]
#[test]
fn saving_through_a_link_keeps_the_link() {
    let dir = std::env::temp_dir().join(format!("fs-local-save-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("real.php"), "old").unwrap();
    std::os::unix::fs::symlink("real.php", dir.join("link.php")).unwrap();
    LocalFileSystem.write(&dir.join("link.php"), b"new").unwrap();
    assert!(std::fs::symlink_metadata(dir.join("link.php")).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(dir.join("real.php")).unwrap(), "new");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A private file (`.env`, a key) stays private through a save: its permissions are the new
/// file's from the moment it exists.
#[cfg(unix)]
#[test]
fn a_private_file_stays_private_when_saved() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("fs-local-private-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(".env");
    std::fs::write(&file, "A=1").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    LocalFileSystem.write(&file, b"A=2").unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "A=2");
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(left.len(), 1, "no temporary file left");
    let _ = std::fs::remove_dir_all(dir);
}

/// Only the case changes: the same entry, renamed (on a disk ignoring case it "exists").
#[test]
fn a_change_of_case_is_a_rename() {
    let dir = std::env::temp_dir().join(format!("fs-local-case-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (from, to) = (dir.join("userservice.php"), dir.join("UserService.php"));
    std::fs::write(&from, "<?php").unwrap();
    assert!(!is_taken(&from, &to));
    move_entry(&from, &to).unwrap();
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["UserService.php"]);
    std::fs::write(dir.join("other.php"), "").unwrap();
    assert!(is_taken(&to, &dir.join("other.php")), "another file is taken");
    let _ = std::fs::remove_dir_all(dir);
}

/// A file without write permission (the read-only attribute on Windows) is read-only; a
/// writable one, a folder or a missing file isn't.
#[test]
fn read_only_files_are_found() {
    let dir = std::env::temp_dir().join(format!("fs-local-ro-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("locked.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(!LocalFileSystem.read_only(&file));
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions.clone()).unwrap();
    // Root writes anything (a container's tests): nothing to check there.
    let writable_anyway = std::fs::OpenOptions::new().append(true).open(&file).is_ok();
    assert!(writable_anyway || LocalFileSystem.read_only(&file));
    assert!(!LocalFileSystem.read_only(&dir));
    assert!(!LocalFileSystem.read_only(&dir.join("missing.txt")));
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&file, permissions).unwrap();
    assert!(!LocalFileSystem.read_only(&file));
    let _ = std::fs::remove_dir_all(dir);
}
