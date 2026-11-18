use super::*;

#[test]
fn searchable_files() {
    // The folders the languages' modules register (Composer's, npm's, the builds').
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    assert!(searchable("app/Http/Controller.php", &PathMarks::default()));
    assert!(searchable("resources/views/home.blade.php", &PathMarks::default()));
    assert!(!searchable("vendor/laravel/framework/src/Foo.php", &PathMarks::default()));
    assert!(!searchable("public/js/app.min.js", &PathMarks::default()));
    assert!(!searchable("logo.png", &PathMarks::default()));
}

#[test]
fn cache_round_trip() {
    let mut index = SearchIndex::default();
    index.set_file("a.php", "function sendMail() {}\n", None);
    let mut files = HashMap::new();
    files.insert("a.php".to_owned(), Fingerprint { len: 1, modified: 2, hash: 3 });
    let dir = std::env::temp_dir().join(format!("search-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("search.bin");
    std::fs::write(&path, encode(&index, &files, 0)).unwrap();
    let (loaded, loaded_files) = load(&path, 0).unwrap();
    assert_eq!(loaded.chunks, index.chunks);
    assert_eq!(loaded_files, files);
    assert_eq!(loaded.search("send mail", None, 3).len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// A refresh of a built index takes only what changed on disk since (new, changed, gone
/// files), in the index in memory — without reading the cache back.
#[test]
fn a_refresh_applies_only_the_changes() {
    use std::sync::atomic::AtomicBool;
    let dir = std::env::temp_dir().join(format!("search-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("a.php", "function sendMail() {}\n");
    write("b.php", "function readInvoice() {}\n");
    let walk = || {
        let mut files: Vec<PathBuf> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        files
    };
    let stop = AtomicBool::new(false);
    let mut state = SearchState::new(Arc::default(), dir.join("cache"), Default::default());
    assert!(state.build(&dir, &walk(), false, &stop));
    let found = |state: &SearchState, q: &str| {
        let index = state.index.read().unwrap();
        index.search(q, None, 10).into_iter().map(|h| h.path).collect::<Vec<_>>()
    };
    assert_eq!(found(&state, "send mail"), ["a.php"]);
    // Nothing changed: nothing to do (and no cache file was ever written to read back).
    assert!(!state.build(&dir, &walk(), true, &stop));
    assert!(!state.cache_file().exists());
    // Changed, new, gone.
    write("a.php", "function sendInvoice() {}\n");
    write("c.php", "function sendMail() {}\n");
    std::fs::remove_file(dir.join("b.php")).unwrap();
    assert!(state.build(&dir, &walk(), true, &stop));
    assert_eq!(found(&state, "send mail"), ["c.php", "a.php"]);
    assert!(found(&state, "read invoice").iter().all(|p| p != "b.php"));
    assert_eq!(state.index.read().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Told of files already indexed as they are (the save, then the watcher's word on it; a
/// folder walked again): none is read; one changed since is.
#[test]
fn an_update_reads_only_what_changed() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = std::env::temp_dir().join(format!("search-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), "function sendMail() {}\n").unwrap();
    std::fs::write(dir.join("b.php"), "function readInvoice() {}\n").unwrap();
    let files = [dir.join("a.php"), dir.join("b.php")];
    let usage: Arc<crate::Usage> = Arc::default();
    let mut state = SearchState::new(Arc::clone(&usage), dir.join("cache"), Default::default());
    assert!(state.build(&dir, &files, false, &AtomicBool::new(false)));
    let read = usage.read.load(Ordering::Relaxed);
    assert!(!state.update(&dir, &files));
    assert_eq!(usage.read.load(Ordering::Relaxed), read, "nothing read again");
    std::fs::write(dir.join("b.php"), "function readInvoices() {}\n").unwrap();
    assert!(state.update(&dir, &files));
    assert_eq!(usage.read.load(Ordering::Relaxed), read + 27, "only b.php");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The index is written only when it changed since read or written (tens of MB each time):
/// once after it's built, not again for nothing; again after a file changed.
#[test]
fn the_index_is_written_only_when_it_changed() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = std::env::temp_dir().join(format!("search-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), "function sendMail() {}\n").unwrap();
    let files = [dir.join("a.php")];
    let usage: Arc<crate::Usage> = Arc::default();
    let mut state = SearchState::new(Arc::clone(&usage), dir.join("cache"), Default::default());
    assert!(state.build(&dir, &files, false, &AtomicBool::new(false)));
    let written = || usage.written.load(Ordering::Relaxed);
    state.save().unwrap();
    let once = written();
    assert!(once > 0 && state.cache_file().exists());
    state.save().unwrap();
    assert_eq!(written(), once, "unchanged: not written again");
    std::fs::write(dir.join("a.php"), "function sendMails() {}\n").unwrap();
    assert!(state.update(&dir, &files));
    state.save().unwrap();
    assert!(written() > once, "changed: written");
    // Let go of memory: what's on disk is the truth, nothing to write.
    state.clear();
    let after = written();
    state.save().unwrap();
    assert_eq!(written(), after);
    let _ = std::fs::remove_dir_all(&dir);
}
