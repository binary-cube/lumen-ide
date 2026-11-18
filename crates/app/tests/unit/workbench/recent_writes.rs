use super::*;
use workspace::RecentFiles as _;

/// A store in memory, counting its writes.
#[derive(Debug, Default)]
struct Memory {
    lists: Mutex<HashMap<PathBuf, Vec<PathBuf>>>,
    writes: Mutex<usize>,
}

impl workspace::RecentFiles for Memory {
    fn load(&self, root: &Path) -> Vec<PathBuf> {
        self.lists.lock().unwrap().get(root).cloned().unwrap_or_default()
    }

    fn save(&self, root: &Path, files: &[PathBuf]) -> std::io::Result<()> {
        *self.writes.lock().unwrap() += 1;
        self.lists.lock().unwrap().insert(root.to_path_buf(), files.to_vec());
        Ok(())
    }
}

fn files(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(|n| PathBuf::from("/p").join(n)).collect()
}

#[test]
fn what_was_saved_is_read_back_and_on_disk_after_a_flush() {
    let store = Arc::new(Memory::default());
    let queued = QueuedRecentFiles::new(Arc::clone(&store) as Arc<dyn workspace::RecentFiles>);
    let root = Path::new("/p");
    queued.save(root, &files(&["a.php"]));
    queued.save(root, &files(&["a.php", "b.php"]));
    // Read back as last saved, written yet or not.
    assert_eq!(queued.load(root), files(&["a.php", "b.php"]));
    queued.flush();
    assert_eq!(store.load(root), files(&["a.php", "b.php"]));
    // Merged: never more writes than saves, the latest list last.
    assert!(*store.writes.lock().unwrap() <= 2);
}

#[test]
fn dropped_it_writes_what_is_left() {
    let store = Arc::new(Memory::default());
    let root = Path::new("/p");
    {
        let queued = QueuedRecentFiles::new(Arc::clone(&store) as Arc<dyn workspace::RecentFiles>);
        for n in 0..20 {
            queued.save(root, &files(&[&format!("{n}.php")]));
        }
    }
    assert_eq!(store.load(root), files(&["19.php"]));
}

#[test]
fn each_project_keeps_its_own_list() {
    let store = Arc::new(Memory::default());
    let queued = QueuedRecentFiles::new(Arc::clone(&store) as Arc<dyn workspace::RecentFiles>);
    queued.save(Path::new("/p"), &files(&["a.php"]));
    queued.save(Path::new("/q"), &[PathBuf::from("/q/b.php")]);
    queued.flush();
    assert_eq!(store.load(Path::new("/p")), files(&["a.php"]));
    assert_eq!(store.load(Path::new("/q")), vec![PathBuf::from("/q/b.php")]);
}
