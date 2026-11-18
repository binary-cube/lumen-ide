// The tests set up what surrounds the pure code: files on disk, threads.
#![allow(clippy::disallowed_methods)]

use super::*;
use crate::domain::Mark;
use std::cell::Cell;

/// In-memory file system: `src/` contains `main.php`.
struct FakeFs {
    reads: Cell<usize>,
}

impl FileSystem for FakeFs {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        self.reads.set(self.reads.get() + 1);
        Ok(match path.file_name().and_then(|n| n.to_str()) {
            Some("project") => vec![DirEntry::new("src", NodeKind::Directory)],
            Some("src") => vec![DirEntry::new("main.php", NodeKind::File)],
            _ => return Err(io::Error::from(io::ErrorKind::NotFound)),
        })
    }

    fn read_file(&self, _path: &Path) -> io::Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

/// What an entry is, told by default from its folder's listing.
#[test]
fn an_entrys_kind_comes_from_its_folders_listing() {
    let fs = FakeFs { reads: Cell::new(0) };
    assert_eq!(fs.kind(Path::new("/tmp/project/src")), Some(NodeKind::Directory));
    assert_eq!(fs.kind(Path::new("/tmp/project/src/main.php")), Some(NodeKind::File));
    assert_eq!(fs.kind(Path::new("/tmp/project/src/none.php")), None);
    assert_eq!(fs.kind(Path::new("/")), None);
    assert_eq!(fs.stamp(Path::new("/tmp/project/src/main.php")), None, "not told");
}

#[test]
fn directory_is_read_once_and_toggles() {
    let fs = FakeFs { reads: Cell::new(0) };
    let mut tree = open_project(PathBuf::from("/tmp/project"), &fs).unwrap();
    let src = tree.visible_rows()[1].id;

    toggle_directory(&mut tree, src, &fs).unwrap();
    assert_eq!(tree.visible_rows().len(), 3);
    toggle_directory(&mut tree, src, &fs).unwrap();
    assert_eq!(tree.visible_rows().len(), 2);
    toggle_directory(&mut tree, src, &fs).unwrap();

    assert_eq!(fs.reads.get(), 2); // root + src only once
}

#[test]
fn toggling_a_file_does_nothing() {
    let fs = FakeFs { reads: Cell::new(0) };
    let mut tree = open_project(PathBuf::from("/tmp/project"), &fs).unwrap();
    let src = tree.visible_rows()[1].id;
    toggle_directory(&mut tree, src, &fs).unwrap();
    let file = tree.visible_rows()[2].id;
    toggle_directory(&mut tree, file, &fs).unwrap();
    assert_eq!(tree.visible_rows().len(), 3);
}

#[test]
fn reveal_expands_parents_and_finds_the_file() {
    let fs = FakeFs { reads: Cell::new(0) };
    let mut tree = open_project(PathBuf::from("/tmp/project"), &fs).unwrap();
    let file = reveal_path(&mut tree, Path::new("/tmp/project/src/main.php"), &fs).unwrap();
    assert_eq!(file.map(|f| tree.name(f).to_owned()), Some("main.php".into()));
    assert_eq!(tree.visible_rows().len(), 3);
    assert_eq!(reveal_path(&mut tree, Path::new("/elsewhere/x.php"), &fs).unwrap(), None);

    tree.collapse_all();
    assert_eq!(tree.visible_rows().len(), 2);
    assert!(tree.expand_loaded());
    assert_eq!(tree.visible_rows().len(), 3);
    assert!(!tree.expand_loaded(), "nothing read is left closed");
    // "Expand all" goes on: everything shown is open, nothing more to read here.
    assert!(!expand_more(&mut tree, &fs).unwrap());
}

#[test]
fn refresh_keeps_expanded_directories() {
    let fs = FakeFs { reads: Cell::new(0) };
    let mut tree = open_project(PathBuf::from("/tmp/project"), &fs).unwrap();
    let src = tree.visible_rows()[1].id;
    toggle_directory(&mut tree, src, &fs).unwrap();
    // "src" changed on disk (a new file in the root): the root is re-read.
    assert!(refresh_path(&mut tree, Path::new("/tmp/project/nou.php"), &fs).unwrap());
    assert_eq!(tree.find(Path::new("/tmp/project/src")), Some(src), "the same node");
    assert!(tree.is_expanded(src), "stays expanded");
    assert_eq!(tree.visible_rows().len(), 3);
    assert!(!refresh_path(&mut tree, Path::new("/elsewhere/x"), &fs).unwrap());
}

#[test]
fn a_listing_adds_what_is_known_but_not_on_disk() {
    let disk = vec![DirEntry::new("index.php", NodeKind::File)];
    let known =
        vec![DirEntry::new("index.php", NodeKind::File), DirEntry::new("app", NodeKind::Directory)];
    let merged = merge_listing(Some(disk), Some(known)).unwrap();
    assert_eq!(merged.len(), 2);
    assert!(!merged[0].absent, "on disk: the disk wins");
    assert_eq!((merged[1].name.as_str(), merged[1].absent), ("app", true));
    // A folder only known (not on disk yet) still lists; one nobody has doesn't.
    assert!(merge_listing(None, Some(vec![DirEntry::new("a.php", NodeKind::File)])).is_some());
    // Known, its contents still coming: listed empty (it opens); unknown: not there.
    assert_eq!(merge_listing(None, Some(Vec::new())), Some(Vec::new()));
    assert!(merge_listing(None, None).is_none());
    // In the tree: absent until it's on disk.
    let mut tree = FileTree::new(PathBuf::from("/p"));
    tree.set_children(tree.root(), vec![DirEntry::absent("a.php", NodeKind::File)]);
    let a = tree.child_named(tree.root(), "a.php").unwrap();
    assert!(tree.is_absent(a));
    tree.set_children(tree.root(), vec![DirEntry::new("a.php", NodeKind::File)]);
    assert!(!tree.is_absent(a), "the same node, now on disk");
}

#[test]
fn a_known_entry_marks_the_one_on_disk() {
    let disk = vec![DirEntry::new("a.php", NodeKind::File), DirEntry::new("b.php", NodeKind::File)];
    let known = vec![DirEntry::new("a.php", NodeKind::File).marked(Mark::Changed)];
    let merged = merge_listing(Some(disk), Some(known)).unwrap();
    assert_eq!((merged[0].mark, merged[0].absent), (Mark::Changed, false));
    assert_eq!(merged[1].mark, Mark::None);
    // The tree keeps it, and follows it when listed again.
    let mut tree = FileTree::new(PathBuf::from("/p"));
    tree.set_children(tree.root(), merged);
    let a = tree.child_named(tree.root(), "a.php").unwrap();
    assert_eq!(tree.mark(a), Mark::Changed);
    tree.set_children(tree.root(), vec![DirEntry::new("a.php", NodeKind::File)]);
    assert_eq!(tree.mark(a), Mark::None, "sent meanwhile");
}

/// A folder re-read itself (entries put into it); one not loaded yet isn't read.
#[test]
fn a_folder_is_read_again_itself() {
    let dir = std::env::temp_dir().join(format!("refresh-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let fs = Disk;
    let mut tree = open_project(dir.clone(), &fs).unwrap();
    let src = tree.child_named(tree.root(), "src").unwrap();
    toggle_directory(&mut tree, src, &fs).unwrap();
    std::fs::write(dir.join("src/new.php"), "").unwrap();
    assert!(refresh_dir(&mut tree, &dir.join("src"), &fs).unwrap());
    assert!(tree.find(&dir.join("src/new.php")).is_some());
    std::fs::create_dir_all(dir.join("later")).unwrap();
    let later = dir.join("later");
    assert!(!refresh_dir(&mut tree, &later, &fs).unwrap(), "not in the tree yet");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The disk, plainly (the test's own: the domain has no I/O).
struct Disk;

impl FileSystem for Disk {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        std::fs::read_dir(path)?
            .map(|e| {
                let e = e?;
                let kind =
                    if e.file_type()?.is_dir() { NodeKind::Directory } else { NodeKind::File };
                Ok(DirEntry::new(e.file_name().to_string_lossy(), kind))
            })
            .collect()
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

/// What a module says of an entry (its note) reaches the tree, on disk or not.
#[test]
fn a_known_entry_s_note_reaches_the_tree() {
    let note = crate::domain::Note { text: "rw------- deploy".into(), warn: true };
    let disk = vec![DirEntry::new("index.php", NodeKind::File)];
    let known = vec![DirEntry::new("index.php", NodeKind::File).noted(Some(note.clone()))];
    let merged = merge_listing(Some(disk), Some(known)).unwrap();
    assert_eq!(merged[0].note.as_ref(), Some(&note));
    let mut tree = FileTree::new(PathBuf::from("/p"));
    let root = tree.root();
    tree.set_children(root, merged);
    let id = tree.child_named(root, "index.php").unwrap();
    assert_eq!(tree.note(id), Some(&note));
    // Listed again without it: gone.
    tree.set_children(root, vec![DirEntry::new("index.php", NodeKind::File)]);
    assert_eq!(tree.note(id), None);
}
