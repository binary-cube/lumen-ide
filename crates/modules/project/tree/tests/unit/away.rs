use super::*;

/// A scratch folder with `sub/` in it, both last changed a minute ago; removed at the end.
#[cfg(unix)]
struct Scratch(PathBuf);

#[cfg(unix)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn scratch() -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "away-check-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let before = SystemTime::now() - Duration::from_secs(60);
    for d in [dir.join("sub"), dir.clone()] {
        std::fs::File::open(&d).unwrap().set_modified(before).unwrap();
    }
    Scratch(dir)
}

/// Runs what was heard, its job here (as the listing's thread would).
#[cfg(unix)]
fn run(check: &AwayCheck, loaded: &[PathBuf]) -> Vec<PathBuf> {
    let loaded = loaded.to_vec();
    check.run(move || loaded, |job| job(), None)
}

/// Left, then back: only the folder whose entries changed meanwhile is told (at the poll
/// after the look), once; nothing changed, nothing told.
#[cfg(unix)]
#[test]
fn only_the_folders_changed_while_away_are_told() {
    let dir = scratch();
    let (root, sub) = (dir.0.clone(), dir.0.join("sub"));
    let loaded = [root, sub.clone()];
    let check = AwayCheck::default();
    // Away, nothing changed.
    check.heard(true);
    check.heard(false);
    assert!(run(&check, &loaded).is_empty());
    assert!(run(&check, &loaded).is_empty(), "nothing found");
    // Away again; made meanwhile, unheard (an install).
    check.heard(true);
    std::fs::write(sub.join("new.txt"), "n").unwrap();
    check.heard(false);
    assert!(run(&check, &loaded).is_empty(), "found by the look, told at the next poll");
    assert_eq!(run(&check, &loaded), vec![sub]);
    assert!(run(&check, &loaded).is_empty(), "told once");
}

/// Back without having been left (the window taken at the project's opening): nothing
/// looked at, the folders not even listed.
#[test]
fn back_without_having_left_does_nothing() {
    let check = AwayCheck::default();
    check.heard(false);
    let asked = std::cell::Cell::new(false);
    let found = check.run(
        || {
            asked.set(true);
            Vec::new()
        },
        |job| job(),
        None,
    );
    assert!(found.is_empty() && !asked.get());
}
