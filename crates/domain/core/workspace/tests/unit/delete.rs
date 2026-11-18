use super::*;

#[test]
fn only_entries_inside_the_project_and_not_twice() {
    let root = Path::new("/p");
    let chosen: Vec<PathBuf> =
        ["/p/src", "/p/src/a.php", "/p", "/elsewhere/x", "/p/b.txt", "/p/b.txt"]
            .iter()
            .map(PathBuf::from)
            .collect();
    assert_eq!(deletion_roots(&chosen, root), [PathBuf::from("/p/src"), PathBuf::from("/p/b.txt")]);
    // `src2` isn't inside `src`.
    let chosen = vec![PathBuf::from("/p/src"), PathBuf::from("/p/src2/a")];
    assert_eq!(deletion_roots(&chosen, root).len(), 2);
}

#[test]
fn files_in_a_deleted_folder_were_deleted() {
    let deleted = vec![PathBuf::from("/p/src")];
    assert!(was_deleted(Path::new("/p/src/a/b.php"), &deleted));
    assert!(!was_deleted(Path::new("/p/srcx/b.php"), &deleted));
}
