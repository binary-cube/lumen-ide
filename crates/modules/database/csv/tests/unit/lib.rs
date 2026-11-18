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
            "database-csv-{name}-{}-{}",
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

/// Counts the bytes asked of the disk (telling a CSV file must not read the whole file).
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
fn csv_files_are_known_by_their_extension_and_a_text_start() {
    let dir = TempDir::new("files");
    let big: Vec<u8> = b"id,name\n".iter().copied().cycle().take(1 << 20).collect();
    let real = dir.0.join("people.csv");
    std::fs::write(&real, &big).unwrap();
    let disk = Counting(std::cell::Cell::new(0));
    assert!(is_csv_file(&real, &disk));
    assert_eq!(disk.0.get(), db_csv::HEAD, "only the start is read");

    let tsv = dir.0.join("DATA.TSV");
    std::fs::write(&tsv, b"a\tb\n").unwrap();
    assert!(is_local_csv_file(&tsv));
    assert!(files().has_extension(Path::new("x.tab")));
    // UTF-16 (NUL bytes), another extension, nothing there.
    let utf16 = dir.0.join("wide.csv");
    std::fs::write(&utf16, b"\xFF\xFEa\0,\0b\0").unwrap();
    assert!(!is_local_csv_file(&utf16));
    let text = dir.0.join("notes.txt");
    std::fs::write(&text, b"a,b\n").unwrap();
    assert!(!is_local_csv_file(&text));
    assert!(!is_local_csv_file(&dir.0.join("missing.csv")));
    // An empty file is an empty table.
    let empty = dir.0.join("empty.csv");
    std::fs::write(&empty, b"").unwrap();
    assert!(is_local_csv_file(&empty));
}

#[test]
fn the_module_contributes_the_driver_and_its_files() {
    let mut registry = ModuleRegistry::default();
    registry.register(Box::new(DatabaseCsvModule));
    let (mut contributions, reports) = registry.activate(&ModuleSelection::default());
    assert_eq!(reports.len(), 1);
    let factories = contributions.take::<DriverFactory>();
    assert_eq!(factories.iter().map(|f| &*f.id).collect::<Vec<_>>(), ["csv"]);
    let files = contributions.take::<DataFiles>();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].driver, "csv");
    assert_eq!(files[0].extensions, db_csv::EXTENSIONS);
}
