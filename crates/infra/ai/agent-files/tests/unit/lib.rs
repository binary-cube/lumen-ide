use super::*;

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agent-files-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Files listed newest first, by extension, a folder never taken for one; folders apart;
/// a file read bounded; a value read again only once its file changed.
#[test]
fn files_are_listed_and_kept_by_their_stamps() {
    let dir = temp();
    std::fs::write(dir.join("a.json"), "1").unwrap();
    std::fs::write(dir.join("b.txt"), "2").unwrap();
    std::fs::create_dir(dir.join("c.json")).unwrap();
    let found = files(&dir, "json", 100);
    assert_eq!(found.len(), 1);
    assert!(found[0].0.ends_with("a.json"));
    assert_eq!(folders(&dir, 100).len(), 1);
    assert_eq!(read_bounded(&dir.join("a.json"), 10).as_deref(), Some(&b"1"[..]));
    assert!(read_bounded(&dir.join("a.json"), 0).is_none(), "longer than allowed");
    assert!(read_bounded(&dir.join("c.json"), 10).is_none(), "a folder");
    let kept: Kept<String> = Kept::new(10);
    let file = dir.join("a.json");
    let at = stamp(&file).unwrap();
    let reads = std::cell::Cell::new(0);
    let read = || {
        reads.set(reads.get() + 1);
        Some("value".to_owned())
    };
    assert_eq!(kept.get(&file, at, read).as_deref(), Some("value"));
    assert_eq!(kept.get(&file, at, read).as_deref(), Some("value"));
    assert_eq!(reads.get(), 1, "the same stamp: not read again");
    kept.get(&file, (at.0, at.1 + 1), read);
    assert_eq!(reads.get(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

/// Titles as a list shows them: tags out, one line, cut with an ellipsis.
#[test]
fn titles_are_tidied() {
    assert_eq!(
        title_of("<user_input mode=\"act\">fix the\n  login</user_input>", 50),
        "fix the login"
    );
    assert_eq!(title_of("abcdefghij", 5), "abcd…");
    assert_eq!(title_of("\u{7}ok", 5), "ok");
}

/// Ids that can name a file and a command's argument; folders compared as written.
#[test]
fn ids_and_folders() {
    assert!(valid_id("1785502698506_ug05k") && valid_id("a-b"));
    assert!(!valid_id("") && !valid_id("../x") && !valid_id("a b"));
    assert!(same_folder(Path::new("/p/x/"), Path::new("/p/x")));
    assert!(!same_folder(Path::new("/p/x"), Path::new("/p/y")));
}
