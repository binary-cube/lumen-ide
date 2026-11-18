use super::*;
use std::collections::BTreeSet;
use std::sync::Mutex;

/// A disk in memory: files and folders by path.
#[derive(Default)]
struct Memory {
    files: Mutex<BTreeSet<PathBuf>>,
    dirs: Mutex<BTreeSet<PathBuf>>,
    trashed: Mutex<Vec<PathBuf>>,
}

impl Memory {
    fn with(files: &[&str], dirs: &[&str]) -> Self {
        let m = Self::default();
        m.files.lock().unwrap().extend(files.iter().map(PathBuf::from));
        m.dirs.lock().unwrap().extend(dirs.iter().map(PathBuf::from));
        m
    }
    fn under(&self, root: &Path) -> Vec<PathBuf> {
        self.files.lock().unwrap().iter().filter(|f| f.starts_with(root)).cloned().collect()
    }
}

impl EntryStore for Memory {
    fn exists(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains(path) || self.dirs.lock().unwrap().contains(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.lock().unwrap().contains(path)
    }
    fn count_files(&self, path: &Path) -> u64 {
        if self.is_dir(path) { self.under(path).len() as u64 } else { 1 }
    }
    fn copy(
        &self,
        from: &Path,
        to: &Path,
        copied: &mut dyn FnMut(u64),
        stop: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        if self.is_dir(from) {
            self.dirs.lock().unwrap().insert(to.to_path_buf());
            for f in self.under(from) {
                if stop() {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let target = to.join(f.strip_prefix(from).unwrap());
                self.files.lock().unwrap().insert(target);
                copied(1);
            }
        } else {
            self.files.lock().unwrap().insert(to.to_path_buf());
            copied(1);
        }
        Ok(())
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let moved: Vec<PathBuf> = self.under(from);
        let mut files = self.files.lock().unwrap();
        for f in moved {
            files.remove(&f);
            files.insert(to.join(f.strip_prefix(from).unwrap()));
        }
        if files.remove(from) {
            files.insert(to.to_path_buf());
        }
        let mut dirs = self.dirs.lock().unwrap();
        if dirs.remove(from) {
            dirs.insert(to.to_path_buf());
        }
        Ok(())
    }
    fn trash(&self, paths: &[PathBuf]) -> io::Result<()> {
        self.trashed.lock().unwrap().extend(paths.iter().cloned());
        Ok(())
    }
    fn create_file(&self, path: &Path) -> io::Result<()> {
        self.files.lock().unwrap().insert(path.to_path_buf());
        Ok(())
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.dirs.lock().unwrap().insert(path.to_path_buf());
        Ok(())
    }
}

#[test]
fn a_new_entry_needs_a_usable_free_name() {
    let disk = Memory::with(&["/p/a.txt"], &["/p"]);
    for bad in ["", "  ", "/abs", "../up", "a/../b", "x?.txt", "a:b"] {
        assert!(name_problem(bad), "{bad:?}");
    }
    assert!(!name_problem("src/new.php"), "a path inside is fine");
    let p = Path::new("/p");
    assert!(create(&disk, p, "a.txt", false).is_err(), "it exists");
    assert_eq!(create(&disk, p, " b.txt ", false).unwrap(), PathBuf::from("/p/b.txt"));
    assert_eq!(create(&disk, p, "lib", true).unwrap(), PathBuf::from("/p/lib"));
    assert!(disk.is_dir(Path::new("/p/lib")));
}

#[test]
fn a_paste_takes_the_file_manager_s_then_ours_then_text_paths() {
    let disk = Memory::with(&["/p/a.txt", "/x/os.txt", "/p/src/s.txt"], &["/p", "/p/src", "/x"]);
    let into = Path::new("/p/src");
    let ours = Some((vec![PathBuf::from("/p/a.txt")], true));
    // The file manager's, copied after: they win, as copies.
    let os = [PathBuf::from("/x/os.txt")];
    assert_eq!(paste_sources(&disk, ours.clone(), &os, None, into), (os.to_vec(), false));
    // Ours: a cut stays a cut.
    assert_eq!(
        paste_sources(&disk, ours, &[], None, into),
        (vec![PathBuf::from("/p/a.txt")], true)
    );
    // Text paths that exist (and are absolute): copied. (`/p` has no drive: absolute only
    // where paths start at `/`.)
    if cfg!(unix) {
        let text = "/p/a.txt\nrelative.txt\n/missing.txt";
        assert_eq!(
            paste_sources(&disk, None, &[], Some(text), into),
            (vec![PathBuf::from("/p/a.txt")], false)
        );
    }
    // A folder isn't pasted into itself.
    let src = Some((vec![PathBuf::from("/p/src")], false));
    assert_eq!(paste_sources(&disk, src, &[], None, into).0, Vec::<PathBuf>::new());
}

#[test]
fn a_paste_copies_or_moves_under_free_names_and_stops_when_asked() {
    let disk = Memory::with(&["/p/a.txt", "/p/lib/x.php", "/p/lib/y.php"], &["/p", "/p/lib"]);
    let into = Path::new("/p");
    let mut seen = Vec::new();
    let mut files_copied = 0;
    // Copied where it already is: a free name.
    let out = paste(
        &disk,
        vec![PathBuf::from("/p/a.txt"), PathBuf::from("/p/lib")],
        false,
        into,
        &mut |done, total| seen.push((done, total)),
        &mut |_| files_copied += 1,
        &|| false,
    );
    assert_eq!(files_copied, 3, "each copied file's bytes told");
    assert_eq!(out.failed, []);
    assert_eq!(out.placed[1].1, PathBuf::from("/p/lib copy"));
    assert!(disk.exists(Path::new("/p/lib copy/x.php")));
    assert_eq!(seen.last(), Some(&(3, 3)), "every file counted");
    // A cut into another folder moves it; one already there is left alone.
    let out = paste(
        &disk,
        vec![PathBuf::from("/p/a.txt"), PathBuf::from("/p/lib/x.php")],
        true,
        Path::new("/p/lib"),
        &mut |_, _| {},
        &mut |_| {},
        &|| false,
    );
    assert_eq!(out.placed, [(PathBuf::from("/p/a.txt"), PathBuf::from("/p/lib/a.txt"))]);
    assert!(!disk.exists(Path::new("/p/a.txt")));
    // Stopped: nothing after the entry going on.
    let out = paste(
        &disk,
        vec![PathBuf::from("/p/lib"), PathBuf::from("/p/lib copy")],
        false,
        Path::new("/q"),
        &mut |_, _| {},
        &mut |_| {},
        &|| true,
    );
    assert_eq!(out.placed.len() + out.failed.len(), 1);
}

#[test]
fn removed_entries_go_to_the_trash() {
    let disk = Memory::with(&["/p/a.txt"], &["/p"]);
    assert_eq!(trash(&disk, &[PathBuf::from("/p/a.txt")]), Ok(()));
    assert_eq!(*disk.trashed.lock().unwrap(), [PathBuf::from("/p/a.txt")]);
}
