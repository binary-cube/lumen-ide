use super::*;

fn temp_store(name: &str) -> (DiskHistory, PathBuf) {
    let dir = std::env::temp_dir().join(format!("history-fs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (DiskHistory::new(&dir), dir)
}

#[test]
fn versions_round_trip_compressed_and_listed_by_name() {
    let (store, dir) = temp_store("round");
    let text = "<?php\n".repeat(500);
    let rev = Revision {
        at: 1000,
        kind: RevisionKind::Saved,
        size: text.len() as u64,
        hash: text_hash(text.as_bytes()),
    };
    store.put("src/a.php", &rev, text.as_bytes()).unwrap();
    let older = Revision { at: 900, kind: RevisionKind::BeforeSave, ..rev };
    store.put("src/a.php", &older, b"old").unwrap();
    assert_eq!(store.revisions("src/a.php"), [rev.clone(), older]);
    assert_eq!(store.read("src/a.php", 1000).unwrap(), text.as_bytes());
    assert_eq!(store.read("src/a.php", 900).unwrap(), b"old");
    assert_eq!(store.paths(), ["src/a.php"]);
    // Compressed on disk.
    let stored = std::fs::read(store.version_path("src/a.php", &rev)).unwrap();
    assert!(stored.len() < text.len() / 4);
    store.remove("src/a.php", 900).unwrap();
    store.remove("src/a.php", 1000).unwrap();
    assert!(store.paths().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn names_parse_back() {
    let rev =
        Revision { at: 1_700_000_000_123, kind: RevisionKind::BeforeSave, size: 42, hash: 0xabc };
    assert_eq!(parse_name(&name_of(&rev)), Some(rev));
    assert_eq!(parse_name("garbage.gz"), None);
}
