use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A folder of its own under the system's temporary one.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("file-identity-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The local disk, its canonical lookups counted.
#[derive(Default)]
struct Counted(AtomicUsize);

impl FileSystem for Counted {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<workspace::DirEntry>> {
        fs_local::LocalFileSystem.read_dir(path)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs_local::LocalFileSystem.read_file(path)
    }

    fn canonical(&self, path: &Path) -> io::Result<PathBuf> {
        self.0.fetch_add(1, Ordering::Relaxed);
        fs_local::LocalFileSystem.canonical(path)
    }
}

#[cfg(unix)]
#[test]
fn a_linked_folder_names_the_same_file() {
    let dir = temp("link");
    std::fs::create_dir_all(dir.join("real")).unwrap();
    std::fs::write(dir.join("real/shop.db"), b"").unwrap();
    std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
    let identities = FileIdentities::default();
    assert!(identities.same(&dir.join("link/shop.db"), &dir.join("real/shop.db")));
    assert!(identities.same(&dir.join("real/./x/../shop.db"), &dir.join("link/shop.db")));
    assert!(!identities.same(&dir.join("real/shop.db"), &dir.join("real/other.db")));
    // Not made yet: its folder's links still count, and it isn't kept.
    let kept = identities.len();
    assert!(identities.same(&dir.join("link/new.db"), &dir.join("real/new.db")));
    assert_eq!(identities.len(), kept);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_is_known_by_its_path_as_written_and_never_kept() {
    let fs = Arc::new(Counted::default());
    let identities = FileIdentities::new(Arc::clone(&fs) as Arc<dyn FileSystem + Send + Sync>);
    let dir = std::env::temp_dir().join(format!("file-identity-missing-{}", std::process::id()));
    let (a, b) = (dir.join("a/./b/../c.db"), dir.join("a/c.db"));
    assert_eq!(identities.key(&a), identities.key(&b));
    // Nothing of it there (a relative path under no folder that is): as written.
    let nowhere = Path::new("no-such-folder-anywhere/./x/../c.db");
    assert_eq!(
        identities.key(nowhere),
        FileKey::lexical(Path::new("no-such-folder-anywhere/c.db"))
    );
    assert!(identities.is_empty(), "not kept: once made, its links count");
    assert!(fs.0.load(Ordering::Relaxed) > 0);
}

#[test]
fn a_file_found_is_kept_until_purged() {
    let dir = temp("kept");
    let file = dir.join("shop.db");
    std::fs::write(&file, b"").unwrap();
    let fs = Arc::new(Counted::default());
    let mut identities = FileIdentities::new(Arc::clone(&fs) as Arc<dyn FileSystem + Send + Sync>);
    let first = identities.key(&file);
    assert_eq!(identities.key(&file), first);
    assert_eq!(fs.0.load(Ordering::Relaxed), 1, "looked up once");
    assert_eq!(identities.len(), 1);
    identities.purge();
    assert!(identities.is_empty());
    assert_eq!(identities.key(&file), first);
    assert_eq!(fs.0.load(Ordering::Relaxed), 2, "looked up again after the purge");
    let _ = std::fs::remove_dir_all(&dir);
}
