use std::collections::{HashMap, HashSet};

use super::*;
use crate::{FileEdit, FilePlan, FileRename, Occurrence, Plan};

/// Open documents and a disk in memory, with a log of what was done to them.
#[derive(Default)]
struct Fake {
    open: HashMap<PathBuf, String>,
    disk: HashMap<PathBuf, String>,
    /// Paths that can't be written.
    read_only: HashSet<PathBuf>,
    log: Vec<String>,
}

impl ProjectFiles for Fake {
    fn open_text(&self, path: &Path) -> Option<String> {
        self.open.get(path).cloned()
    }
    fn edit_open(&mut self, path: &Path, edits: &[TextEdit]) {
        let text = self.open.get_mut(path).unwrap();
        *text = apply_edits(text, edits).unwrap();
        self.log.push(format!("edit {}", path.display()));
    }
    fn replace_open(&mut self, path: &Path, text: String) {
        self.open.insert(path.to_path_buf(), text);
        self.log.push(format!("replace {}", path.display()));
    }
    fn read(&self, path: &Path) -> Option<String> {
        self.disk.get(path).cloned()
    }
    fn write(&mut self, path: &Path, text: &str) -> io::Result<()> {
        if self.read_only.contains(path) {
            return Err(io::Error::other("read-only"));
        }
        self.disk.insert(path.to_path_buf(), text.to_owned());
        self.log.push(format!("write {}", path.display()));
        Ok(())
    }
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        if self.disk.contains_key(to) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        let text = self.disk.remove(from).ok_or(io::ErrorKind::NotFound)?;
        self.disk.insert(to.to_path_buf(), text);
        if let Some(text) = self.open.remove(from) {
            self.open.insert(to.to_path_buf(), text);
        }
        self.log.push(format!("rename {} {}", from.display(), to.display()));
        Ok(())
    }
}

/// A place renaming `old` to `new` at `at` of `text`.
fn place(text: &str, at: usize, new: &str) -> Occurrence {
    let old = "save";
    let range = at..at + old.len();
    let (line, line_text, in_line) = crate::line_of(text, &range);
    Occurrence {
        range,
        original: text[at..at + old.len()].to_owned(),
        replacement: new.to_owned(),
        certainty: crate::Certainty::Certain,
        chosen: true,
        line,
        line_text,
        in_line,
    }
}

const OPEN: &str = "function save() {}";
const CLOSED: &str = "$a->save(); $b->save();";

fn project() -> Fake {
    let mut fake = Fake::default();
    fake.open.insert("/p/Repo.php".into(), OPEN.into());
    fake.disk.insert("/p/Repo.php".into(), OPEN.into());
    fake.disk.insert("/p/use.php".into(), CLOSED.into());
    fake
}

fn plan() -> Plan {
    Plan {
        files: vec![
            FilePlan { path: "/p/Repo.php".into(), occurrences: vec![place(OPEN, 9, "store")] },
            FilePlan {
                path: "/p/use.php".into(),
                occurrences: vec![place(CLOSED, 4, "store"), place(CLOSED, 16, "store")],
            },
        ],
        renames: Vec::new(),
    }
}

#[test]
fn applies_open_files_in_their_documents_and_closed_ones_on_disk() {
    let mut fake = project();
    let applied = apply_plan(&plan(), &mut fake);
    assert_eq!((applied.places, applied.files, applied.skipped), (3, 2, 0));
    assert!(!applied.changed_nothing());
    assert_eq!(fake.open[Path::new("/p/Repo.php")], "function store() {}");
    assert_eq!(fake.disk[Path::new("/p/Repo.php")], OPEN, "the open file isn't saved");
    assert_eq!(fake.disk[Path::new("/p/use.php")], "$a->store(); $b->store();");
    assert_eq!(fake.log, ["edit /p/Repo.php", "write /p/use.php"]);
    assert_eq!(applied.undo.files(), [PathBuf::from("/p/Repo.php"), "/p/use.php".into()]);

    let undone = undo_plan(applied.undo, &mut fake);
    assert_eq!(undone.skipped, 0);
    assert_eq!(fake.open[Path::new("/p/Repo.php")], OPEN);
    assert_eq!(fake.disk[Path::new("/p/use.php")], CLOSED);
    assert_eq!(&fake.log[2..], ["replace /p/Repo.php", "write /p/use.php"]);
}

#[test]
fn a_changed_file_is_skipped_and_nothing_left_is_stale() {
    let mut fake = project();
    fake.disk.insert("/p/use.php".into(), format!("// new\n{CLOSED}"));
    let applied = apply_plan(&plan(), &mut fake);
    assert_eq!((applied.places, applied.files, applied.skipped), (1, 1, 1));

    let mut fake = project();
    fake.open.insert("/p/Repo.php".into(), "changed".into());
    fake.disk.insert("/p/use.php".into(), "changed".into());
    let applied = apply_plan(&plan(), &mut fake);
    assert!(applied.changed_nothing(), "stale: nothing to undo");
    assert_eq!(applied.skipped, 2);
    assert!(fake.log.is_empty());
}

#[test]
fn an_unwritable_file_is_skipped_and_told() {
    let mut fake = project();
    fake.read_only.insert("/p/use.php".into());
    let applied = apply_plan(&plan(), &mut fake);
    assert_eq!((applied.places, applied.skipped), (1, 1));
    assert_eq!(applied.problems.len(), 1);
    assert_eq!(applied.problems[0].to_string(), "can't write /p/use.php: read-only");
}

#[test]
fn undo_keeps_the_work_done_since() {
    let mut fake = project();
    let applied = apply_plan(&plan(), &mut fake);
    let later = "$a->store(); // later".to_owned();
    fake.disk.insert("/p/use.php".into(), later.clone());
    let undone = undo_plan(applied.undo, &mut fake);
    assert_eq!(undone.skipped, 1);
    assert_eq!(fake.disk[Path::new("/p/use.php")], later, "not overwritten");
    assert_eq!(fake.open[Path::new("/p/Repo.php")], OPEN, "the unchanged one undone");
}

#[test]
fn renames_follow_the_edits_and_go_back_first() {
    let mut fake = project();
    let mut plan = plan();
    plan.renames = vec![
        FileRename { from: "/p/Repo.php".into(), to: "/p/Store.php".into(), chosen: true },
        // Not chosen, or not a change: left alone.
        FileRename { from: "/p/use.php".into(), to: "/p/other.php".into(), chosen: false },
        FileRename { from: "/p/use.php".into(), to: "/p/use.php".into(), chosen: true },
    ];
    let applied = apply_plan(&plan, &mut fake);
    assert_eq!(applied.undo.renames(), [("/p/Repo.php".into(), "/p/Store.php".into())]);
    assert_eq!(fake.log.last().unwrap(), "rename /p/Repo.php /p/Store.php");
    assert_eq!(fake.open[Path::new("/p/Store.php")], "function store() {}");

    let undone = undo_plan(applied.undo, &mut fake);
    assert_eq!(fake.log[3], "rename /p/Store.php /p/Repo.php", "before the texts");
    assert_eq!(undone.renames, [("/p/Store.php".into(), "/p/Repo.php".into())]);
    assert_eq!(undone.files, [PathBuf::from("/p/Repo.php"), "/p/use.php".into()]);
    assert_eq!(fake.open[Path::new("/p/Repo.php")], OPEN);
}

#[test]
fn a_plain_move_is_a_change_and_a_failed_one_is_told() {
    let mut fake = project();
    let mut plan = Plan::default();
    plan.renames.push(FileRename {
        from: "/p/use.php".into(),
        to: "/q/use.php".into(),
        chosen: true,
    });
    let applied = apply_plan(&plan, &mut fake);
    assert!(!applied.changed_nothing());
    assert_eq!(applied.places, 0);

    plan.renames[0].to = "/p/Repo.php".into();
    let mut fake = project();
    let applied = apply_plan(&plan, &mut fake);
    assert!(applied.changed_nothing());
    assert_eq!(applied.skipped, 1);
    assert!(applied.problems[0].to_string().starts_with("can't rename /p/use.php: "));
}

#[test]
fn edits_of_one_file_come_sorted() {
    // `Plan::edits` gives them sorted: the port gets them as they are.
    let edits: Vec<FileEdit> = plan().edits();
    assert!(edits[1].edits.windows(2).all(|w| w[0].range.start < w[1].range.start));
}

/// A disk's entries in memory.
#[derive(Default)]
struct Disk {
    entries: std::sync::Mutex<Vec<PathBuf>>,
    dirs: std::sync::Mutex<Vec<PathBuf>>,
}

impl Entries for Disk {
    fn is_taken(&self, from: &Path, to: &Path) -> bool {
        // A change of case only is the same entry.
        let same = from.to_string_lossy().eq_ignore_ascii_case(&to.to_string_lossy());
        !same && self.entries.lock().unwrap().iter().any(|e| e == to)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.dirs.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.iter_mut().find(|e| *e == from).ok_or(io::ErrorKind::NotFound)?;
        *entry = to.to_path_buf();
        Ok(())
    }
}

#[test]
fn an_entry_moves_into_its_new_folder_never_over_another() {
    let disk = Disk::default();
    disk.entries.lock().unwrap().extend(["/p/a.php".into(), "/p/b.php".into()]);
    let e = rename_entry(&disk, Path::new("/p/a.php"), Path::new("/p/b.php")).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
    assert!(disk.dirs.lock().unwrap().is_empty(), "nothing made");

    rename_entry(&disk, Path::new("/p/a.php"), Path::new("/p/new/a.php")).unwrap();
    assert_eq!(*disk.dirs.lock().unwrap(), [PathBuf::from("/p/new")]);
    assert!(disk.entries.lock().unwrap().contains(&PathBuf::from("/p/new/a.php")));
    // Only its case changed: not taken by itself.
    rename_entry(&disk, Path::new("/p/b.php"), Path::new("/p/B.php")).unwrap();
}
