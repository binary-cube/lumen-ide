use super::*;

use std::collections::HashMap;
use std::sync::Mutex;

/// A file's versions with their texts.
type Versions = Vec<(Revision, Vec<u8>)>;

/// Versions in memory.
#[derive(Debug, Default)]
struct MemoryStore {
    files: Mutex<HashMap<String, Versions>>,
}

impl HistoryStore for MemoryStore {
    fn put(&self, path: &str, revision: &Revision, text: &[u8]) -> std::io::Result<()> {
        let mut files = self.files.lock().unwrap();
        files.entry(path.into()).or_default().insert(0, (revision.clone(), text.to_vec()));
        Ok(())
    }
    fn revisions(&self, path: &str) -> Vec<Revision> {
        let files = self.files.lock().unwrap();
        files.get(path).map(|v| v.iter().map(|(r, _)| r.clone()).collect()).unwrap_or_default()
    }
    fn read(&self, path: &str, at: u64) -> Option<Vec<u8>> {
        let files = self.files.lock().unwrap();
        files.get(path)?.iter().find(|(r, _)| r.at == at).map(|(_, t)| t.clone())
    }
    fn remove(&self, path: &str, at: u64) -> std::io::Result<()> {
        let mut files = self.files.lock().unwrap();
        if let Some(v) = files.get_mut(path) {
            v.retain(|(r, _)| r.at != at);
        }
        Ok(())
    }
    fn paths(&self) -> Vec<String> {
        self.files.lock().unwrap().keys().cloned().collect()
    }
}

#[test]
fn repeats_large_and_binary_files_are_not_kept() {
    let store = MemoryStore::default();
    let retention = Retention { max_file_size: 10, ..Retention::default() };
    let keep = |text: &[u8], now| keep(&store, &retention, "a.php", text, RevisionKind::Saved, now);
    assert!(keep(b"one", 1).unwrap().is_some());
    assert!(keep(b"one", 2).unwrap().is_none(), "the same as the last");
    assert!(keep(b"two", 2).unwrap().is_some());
    assert!(keep(b"far too long text", 3).unwrap().is_none());
    assert!(keep(b"bin\0ary", 3).unwrap().is_none());
    // Same millisecond: still ordered.
    let third = keep(b"three", 2).unwrap().unwrap();
    assert_eq!(third.at, 3);
    let list = versions(&store, "a.php");
    assert_eq!(list.iter().map(|r| r.at).collect::<Vec<_>>(), [3, 2, 1]);
}

#[test]
fn a_version_is_compared_with_the_current_text() {
    let store = MemoryStore::default();
    let retention = Retention::default();
    let kept = keep(&store, &retention, "src/a.php", b"old", RevisionKind::BeforeSave, 5)
        .unwrap()
        .unwrap();
    let input = compare_with_current(
        &store,
        "src/a.php",
        kept.at,
        Path::new("/p/src/a.php"),
        "new",
        "10:42",
    )
    .unwrap();
    assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("old", "new"));
    assert_eq!(input.labels[0].as_deref(), Some("a.php · 10:42"));
    assert_eq!(input.read_only, [true, false]);
}

#[test]
fn pruning_follows_the_retention() {
    let store = MemoryStore::default();
    let retention = Retention { max_per_file: 2, ..Retention::default() };
    // Put there as an older run left them (keeping now trims as it goes).
    for (i, text) in [b"1", b"2", b"3", b"4"].iter().enumerate() {
        let at = i as u64 + 1;
        let revision = Revision { at, kind: RevisionKind::Saved, size: 1, hash: text_hash(*text) };
        store.put("a.php", &revision, *text).unwrap();
    }
    assert_eq!(prune(&store, &retention, 10), 2);
    assert_eq!(versions(&store, "a.php").len(), 2);
}

/// Each version kept drops what the retention expires in its file: saving all day stays
/// within the most per file (not only when pruned at the next start).
#[test]
fn keeping_a_version_drops_those_past_the_most_per_file() {
    let store = MemoryStore::default();
    let retention = Retention { max_per_file: 3, ..Retention::default() };
    for i in 0..10u64 {
        let text = format!("version {i}");
        keep(&store, &retention, "a.php", text.as_bytes(), RevisionKind::Saved, 100 + i).unwrap();
    }
    let list = versions(&store, "a.php");
    assert_eq!(list.iter().map(|r| r.at).collect::<Vec<_>>(), [109, 108, 107], "the newest three");
    assert_eq!(store.read("a.php", 109).as_deref(), Some(&b"version 9"[..]));
}
