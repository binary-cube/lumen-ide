use super::*;

/// A save's temporary names are recognized (ours, another instance's); look-alikes aren't.
#[test]
fn a_save_s_temporary_file_is_known_by_its_name() {
    let ours = temporary_name("index.php", 7);
    assert!(is_temporary(Path::new(&format!("/p/{ours}"))));
    assert!(is_temporary(Path::new("/p/.a.php.saving-123-4.tmp")), "another instance's");
    assert!(!is_temporary(Path::new("/p/a.php")));
    assert!(!is_temporary(Path::new("/p/.saving-notes.tmp")));
    assert!(!is_temporary(Path::new("/p/.a.saving-x.tmp")));
    assert!(!is_temporary(Path::new("/p/a.saving-1-2.tmp")), "not hidden");
}

/// A file a save replaced is ours while it's there; gone since, or never saved: as heard.
#[test]
fn a_file_replaced_by_a_save_is_written_not_a_new_entry() {
    let dir = std::env::temp_dir().join(format!(
        "own-saves-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.php");
    std::fs::write(&file, "<?php").unwrap();
    assert_eq!(heard(&file), Heard::Other, "not saved by us");
    replacing(&file, &file);
    assert_eq!(heard(&file), Heard::Replaced);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(heard(&file), Heard::Other, "deleted since: an entry change");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The folder a save of ours wrote in is ours too (its entries as they were); another
/// folder, or one gone since, as heard.
#[test]
fn the_folder_a_save_wrote_in_holds_the_same_entries() {
    let dir = std::env::temp_dir().join(format!(
        "own-saves-holder-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let other = dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let file = dir.join("a.php");
    std::fs::write(&file, "<?php").unwrap();
    replacing(&file, &file);
    assert_eq!(heard(&dir), Heard::Holder);
    assert_eq!(heard(&other), Heard::Other, "a folder nothing was saved in");
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(heard(&dir), Heard::Other, "gone since: an entry change");
}

/// A file our save put in place stays ours however late it's heard; made again by another
/// program since (another file in its place), it's an entry change.
#[cfg(unix)]
#[test]
fn a_file_made_again_since_the_save_is_an_entry_change() {
    let dir = std::env::temp_dir().join(format!(
        "own-saves-again-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.php");
    std::fs::write(&file, "<?php").unwrap();
    // As a save does: the temporary file's identity is what the rename puts in place.
    let tmp = dir.join(temporary_name("a.php", 0));
    std::fs::write(&tmp, "<?php // saved").unwrap();
    replacing(&file, &tmp);
    std::fs::rename(&tmp, &file).unwrap();
    assert_eq!(heard(&file), Heard::Replaced);
    // Kept aside so the system can't give the new file the same number.
    let aside = dir.join("aside.php");
    std::fs::rename(&file, &aside).unwrap();
    std::fs::write(&file, "<?php // another program's").unwrap();
    assert_eq!(heard(&file), Heard::Other, "another file in its place");
    let _ = std::fs::remove_dir_all(&dir);
}
