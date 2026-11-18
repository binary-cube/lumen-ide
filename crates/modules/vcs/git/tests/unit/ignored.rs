//! Writes to ignored files ask for no `git status`: under an ignored folder, matched by a
//! nested `.gitignore` or `info/exclude`; a tracked-looking file, a `.gitignore` itself or
//! a file outside any repository still do.

use super::*;

#[test]
fn ignored_writes_are_told_apart() {
    let root = std::env::temp_dir().join(format!("git-ignored-{:x}", kernel::id::new_uuid()));
    let write = |rel: &str, text: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    };
    std::fs::create_dir_all(root.join(".git/info")).unwrap();
    write(".gitignore", "/storage/logs/\n*.cache\n!keep.cache\n");
    write("public/.gitignore", "build/\n");
    write(".git/info/exclude", "local.txt\n");
    let ignored = Ignored::default();
    let is = |rel: &str| ignored.all(&[root.join(rel)]);
    assert!(is("storage/logs/laravel.log"), "under an ignored folder");
    assert!(is("var/app.cache"));
    assert!(!is("var/keep.cache"), "a negation");
    assert!(is("public/build/app.js"), "a nested .gitignore");
    assert!(is("local.txt"), "info/exclude");
    assert!(!is("src/Cart.php"));
    assert!(!is(".gitignore"), "the rules themselves");
    assert!(
        !ignored.all(&[root.join("storage/logs/a.log"), root.join("src/Cart.php")]),
        "all of them"
    );
    assert!(!ignored.all(&[]));
    // The rules change: read again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(".gitignore", "/storage/logs/\n*.cache\n!keep.cache\n/src/\n");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    let file = std::fs::File::options().write(true).open(root.join(".gitignore")).unwrap();
    file.set_modified(later).unwrap();
    assert!(is("src/Cart.php"), "the new rule");
    std::fs::remove_dir_all(&root).unwrap();
}
