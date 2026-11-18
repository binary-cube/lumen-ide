use std::collections::HashSet;

use super::*;

#[test]
fn pasted_entries_get_a_free_name() {
    let taken: HashSet<PathBuf> =
        ["/p/a.php", "/p/a copy.php", "/p/.env", "/p/src"].iter().map(PathBuf::from).collect();
    let exists = |p: &Path| taken.contains(p);
    assert_eq!(
        paste_target(Path::new("/x/b.php"), false, Path::new("/p"), &exists),
        PathBuf::from("/p/b.php")
    );
    assert_eq!(
        paste_target(Path::new("/x/a.php"), false, Path::new("/p"), &exists),
        PathBuf::from("/p/a copy 2.php")
    );
    assert_eq!(
        paste_target(Path::new("/x/.env"), false, Path::new("/p"), &exists),
        PathBuf::from("/p/.env copy")
    );
    assert_eq!(
        paste_target(Path::new("/x/src"), true, Path::new("/p"), &exists),
        PathBuf::from("/p/src copy")
    );
}

#[test]
fn a_folder_isnt_pasted_into_itself_and_moves_follow() {
    assert!(paste_forbidden(Path::new("/p/src"), Path::new("/p/src/app")));
    assert!(paste_forbidden(Path::new("/p/src"), Path::new("/p/src")));
    assert!(!paste_forbidden(Path::new("/p/src"), Path::new("/p/srcx")));
    let (from, to) = (Path::new("/p/src"), Path::new("/p/lib"));
    assert_eq!(
        moved_path(Path::new("/p/src/a/b.php"), from, to),
        Some(PathBuf::from("/p/lib/a/b.php"))
    );
    assert_eq!(moved_path(Path::new("/p/src"), from, to), Some(PathBuf::from("/p/lib")));
    assert_eq!(moved_path(Path::new("/p/other.php"), from, to), None);
}
