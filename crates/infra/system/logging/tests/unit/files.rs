use super::*;

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("logging-files-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_day_s_files_are_named_in_order() {
    assert_eq!(name("2026-11-20", 0, None), "app-2026-11-20.jsonl");
    assert_eq!(name("2026-11-20", 2, None), "app-2026-11-20.2.jsonl");
    assert_eq!(name("2026-11-20", 0, Some("abc")), "app-2026-11-20-abc.jsonl");
    assert_eq!(name("2026-11-20", 1, Some("abc")), "app-2026-11-20-abc.1.jsonl");
}

/// The day's file while it has room, the next one past it; a second instance writes its
/// own while the first holds the day's.
#[test]
fn a_full_file_continues_in_the_next_and_another_instance_writes_its_own() {
    let dir = scratch();
    let limits = Limits { file: 10, ..Limits::default() };
    let first = open(&dir, "2026-11-20", "one", limits).unwrap();
    assert_eq!(first.path, dir.join("app-2026-11-20.jsonl"));
    let other = open(&dir, "2026-11-20", "two", limits).unwrap();
    assert_eq!(other.path, dir.join("app-2026-11-20-two.jsonl"), "held by the first");
    drop(other);
    std::fs::write(&first.path, "0123456789").unwrap();
    let full = Current { size: 10, ..first };
    let next = next(&dir, &full, "one", limits).unwrap();
    assert_eq!(next.path, dir.join("app-2026-11-20.1.jsonl"));
    drop((full, next));
    // Opened again later: the day's last with room.
    let again = open(&dir, "2026-11-20", "three", limits).unwrap();
    assert_eq!(again.index, 1);
    let _ = std::fs::remove_dir_all(dir);
}

fn kept(day: &str, name: &str, size: u64) -> Kept {
    Kept { path: PathBuf::from(name), day: day.into(), size }
}

/// The days past the number kept go, then the oldest while over the total; the file in
/// use never.
#[test]
fn the_oldest_go_first_and_the_file_in_use_never() {
    let limits = Limits { file: 100, days: 2, total: 250 };
    let all = vec![
        kept("2026-11-17", "a", 100),
        kept("2026-11-18", "b", 100),
        kept("2026-11-19", "c", 100),
        kept("2026-11-20", "d", 100),
        kept("2026-11-20", "e", 100),
    ];
    let gone = to_remove(&all, "2026-11-20", limits, Path::new("e"));
    assert_eq!(gone, [PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")]);
    // Under its total and within its days: nothing goes.
    let few = vec![kept("2026-11-20", "d", 10)];
    assert!(to_remove(&few, "2026-11-20", limits, Path::new("x")).is_empty());
    // Over the total within one day: the oldest file of it, not the one in use.
    let big = vec![kept("2026-11-20", "x", 200), kept("2026-11-20", "y", 200)];
    assert_eq!(to_remove(&big, "2026-11-20", limits, Path::new("y")), [PathBuf::from("x")]);
}

/// Only our files are listed (and let go): another file in the folder is left alone.
#[test]
fn only_the_log_s_own_files_are_listed_and_cleaned() {
    let dir = scratch();
    for name in [
        "app-2026-01-01.jsonl",
        "app-2026-11-20.jsonl",
        "notes.txt",
        "app-x.jsonl",
        "app-2026-01-01.log",
    ] {
        std::fs::write(dir.join(name), "x").unwrap();
    }
    let names: Vec<String> = listed(&dir)
        .iter()
        .map(|k| k.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["app-2026-01-01.jsonl", "app-2026-11-20.jsonl"]);
    let limits = Limits { days: 1, ..Limits::default() };
    assert_eq!(clean(&dir, "2026-11-20", limits, &dir.join("app-2026-11-20.jsonl")), 1);
    assert!(dir.join("notes.txt").exists() && dir.join("app-2026-01-01.log").exists());
    assert!(!dir.join("app-2026-01-01.jsonl").exists());
    let _ = std::fs::remove_dir_all(dir);
}
