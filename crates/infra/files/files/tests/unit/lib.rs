use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The content replaces the old one; no temporary file is left behind; the folder is made.
#[test]
fn writes_atomically_and_cleans_up() {
    let dir = scratch("atomic");
    let path = dir.join("sub").join("a.toml");
    write_atomic(&path, b"one").unwrap();
    write_atomic(&path, b"two").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"two");
    let left: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
    assert_eq!(left.len(), 1, "only the file itself");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Targets with the same stem don't share a temporary file.
#[test]
fn same_stem_targets_dont_collide() {
    let dir = scratch("stem");
    let (a, b) = (dir.join("x.toml"), dir.join("x.json"));
    let tmp_a = temporary(&dir, &a);
    let tmp_b = temporary(&dir, &b);
    assert_ne!(tmp_a, tmp_b);
    assert_ne!(temporary(&dir, &a), tmp_a, "a new name per write");
}

/// A private file is the user's alone from the start.
#[cfg(unix)]
#[test]
fn private_files_are_only_the_users() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("private");
    let path = dir.join("secrets");
    write_private(&path, b"s").unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a killed write left is removed once old enough; a young one (a write going on),
/// the files themselves and look-alikes stay.
#[test]
fn temporaries_left_by_a_killed_write_are_removed() {
    let dir = scratch("left");
    std::fs::create_dir_all(&dir).unwrap();
    for name in [".search.bin.88072-17.tmp", "search.bin", ".notes.tmp", "a.1-2.tmp"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    assert_eq!(remove_left_temporaries(&dir, std::time::Duration::from_secs(3600)), 0, "young");
    assert_eq!(remove_left_temporaries(&dir, std::time::Duration::ZERO), 1);
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, [".notes.tmp", "a.1-2.tmp", "search.bin"]);
    let _ = std::fs::remove_dir_all(&dir);
}
