use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use extensions::{ModuleRegistry, ModuleSelection};

use super::*;

/// A folder of the system's temp dir, gone with it (never the user's files).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "database-sqlite-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Counts the bytes asked of the disk (the header must not read the whole file).
struct Counting(std::cell::Cell<usize>);

impl DocumentStorage for Counting {
    fn read(&self, _: &Path) -> std::io::Result<Vec<u8>> {
        panic!("the whole file is never read");
    }

    fn head(&self, path: &Path, max: usize) -> std::io::Result<(u64, Vec<u8>)> {
        self.0.set(self.0.get() + max);
        fs_local::LocalFileSystem.head(path, max)
    }

    fn write(&self, _: &Path, _: &[u8]) -> std::io::Result<()> {
        panic!("nothing is written");
    }
}

#[test]
fn sqlite_files_are_known_by_their_extension_and_header() {
    let dir = TempDir::new("files");
    let mut database = b"SQLite format 3\0".to_vec();
    database.resize(4096, 0);
    let real = dir.0.join("shop.sqlite3");
    std::fs::write(&real, &database).unwrap();
    let disk = Counting(std::cell::Cell::new(0));
    assert!(is_sqlite_file(&real, &disk));
    assert_eq!(disk.0.get(), 16, "only the header is read");

    // Another program's `.db` that isn't SQLite.
    let other = dir.0.join("cache.db");
    std::fs::write(&other, b"BDB\0 a Berkeley DB, or anything else...").unwrap();
    assert!(!is_local_sqlite_file(&other));
    // A database under another extension isn't one of its files; an empty one neither.
    let renamed = dir.0.join("shop.bin");
    std::fs::write(&renamed, &database).unwrap();
    assert!(!is_local_sqlite_file(&renamed));
    let empty = dir.0.join("empty.db");
    std::fs::write(&empty, b"").unwrap();
    assert!(!is_local_sqlite_file(&empty));
    assert!(!is_local_sqlite_file(&dir.0.join("missing.db")));
    assert!(files().has_extension(Path::new("UPPER.DB3")));
}

#[test]
fn the_module_contributes_the_driver_and_its_files() {
    let mut registry = ModuleRegistry::default();
    registry.register(Box::new(DatabaseSqliteModule));
    let (mut contributions, reports) = registry.activate(&ModuleSelection::default());
    assert_eq!(reports.len(), 1);
    let factories = contributions.take::<DriverFactory>();
    assert_eq!(factories.iter().map(|f| &*f.id).collect::<Vec<_>>(), ["sqlite"]);
    let files = contributions.take::<DataFiles>();
    assert_eq!(files.len(), 1);
    assert_eq!((&*files[0].driver, files[0].head), ("sqlite", 16));
}
