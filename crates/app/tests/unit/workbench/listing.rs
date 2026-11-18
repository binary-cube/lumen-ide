use super::*;
use workspace::{DirEntry, NodeKind};

/// A module that knows `remote.php` and `vendor/` are in the project's root.
#[derive(Debug)]
struct Knows(PathBuf);

impl workbench_model::ProjectService for Knows {
    fn attach(&self, _: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        Vec::new()
    }

    fn listed(&self, dir: &Path) -> Option<Vec<DirEntry>> {
        if dir == self.0 {
            Some(vec![
                DirEntry::new("remote.php", NodeKind::File),
                DirEntry::new("vendor", NodeKind::Directory),
            ])
        } else if dir == self.0.join("vendor") {
            Some(vec![DirEntry::new("lib.php", NodeKind::File)])
        } else if dir == self.0.join("coming") {
            Some(Vec::new())
        } else {
            None
        }
    }
}

#[test]
fn the_tree_lists_the_disk_and_what_modules_know_is_elsewhere() {
    let root = std::env::temp_dir().join(format!("listed-fs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("here.php"), "<?php").unwrap();
    let services: Vec<Arc<dyn workbench_model::ProjectService>> =
        vec![Arc::new(Knows(root.clone()))];
    let fs = ListedFs::new(&fs_local::LocalFileSystem, &services);
    let mut listed = fs.read_dir(&root).unwrap();
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    let shown: Vec<_> = listed.iter().map(|e| (e.name.as_str(), e.absent)).collect();
    assert_eq!(shown, [("here.php", false), ("remote.php", true), ("vendor", true)]);
    // A folder that's only known (not on disk) still lists.
    let vendor = fs.read_dir(&root.join("vendor")).unwrap();
    assert_eq!((vendor[0].name.as_str(), vendor[0].absent), ("lib.php", true));
    // Known, its contents still coming: listed (empty), so it opens.
    assert!(fs.read_dir(&root.join("coming")).unwrap().is_empty());
    // One nobody knows: not found, as before.
    assert!(fs.read_dir(&root.join("nowhere")).is_err());
    // Without modules: the disk alone.
    let none: Vec<Arc<dyn workbench_model::ProjectService>> = Vec::new();
    assert_eq!(ListedFs::new(&fs_local::LocalFileSystem, &none).read_dir(&root).unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}
