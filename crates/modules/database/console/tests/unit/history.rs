//! A project's histories: read once (on the caller's thread for a run, in the background for
//! the window), each run appended, the file compacted past the limit, a deletion written
//! again with what's there then, a broken file said once, purged and read again.

use super::*;
use crate::fake::Manual;
use database::history::{Loaded, MemoryHistory};
use database::sql::{Flavor, ParamStyles};

fn entry(source: DataSourceId, text: &str, at: i64) -> HistoryEntry {
    HistoryEntry::new(source, text, Flavor::Sqlite, &ParamStyles::of(Flavor::Sqlite), at)
}

#[test]
fn runs_are_recorded_and_written_and_deleted() {
    let manual = Manual::new();
    let store = Arc::new(MemoryHistory::default());
    let source = DataSourceId::new();
    store.append(source, &entry(source, "SELECT old", 1)).unwrap();
    let h = Arc::new(Histories::new(Arc::clone(&store) as _, manual.runtime()));
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = Arc::clone(&woken);
    h.add_wake(
        "test",
        Arc::new(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
    );
    let before = h.generation();
    // A run reads the file first (here), then appends.
    h.record(entry(source, "SELECT 1", 2));
    assert!(h.generation() > before);
    assert!(woken.load(std::sync::atomic::Ordering::SeqCst) > 0);
    assert_eq!(store.blocks(source).len(), 2);
    let all = h.entries(source).unwrap();
    assert_eq!(all.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["SELECT 1", "SELECT old"]);
    // Deleted: the file written again in the background, with a run recorded meanwhile.
    assert!(h.remove(source, all[1].id));
    h.record(entry(source, "SELECT 2", 3));
    manual.run_all();
    let texts: Vec<String> = store.blocks(source).into_iter().map(|e| e.text).collect();
    assert_eq!(texts, ["SELECT 1", "SELECT 2"]);
    assert!(!h.remove(source, all[1].id), "gone already");
    assert!(h.clear(source));
    manual.run_all();
    assert!(store.blocks(source).is_empty());
    assert!(!h.clear(source));
}

#[test]
fn the_window_reads_it_in_the_background_and_a_purge_reads_it_again() {
    let manual = Manual::new();
    let store = Arc::new(MemoryHistory::default());
    let source = DataSourceId::new();
    store.append(source, &entry(source, "SELECT 1", 1)).unwrap();
    let h = Arc::new(Histories::new(Arc::clone(&store) as _, manual.runtime()));
    assert_eq!(h.entries(source), None);
    assert_eq!(h.entries(source), None, "read once");
    assert_eq!(manual.run_all(), 1);
    assert_eq!(h.entries(source).unwrap().len(), 1);
    store.append(source, &entry(source, "SELECT 2", 2)).unwrap();
    h.purge();
    assert_eq!(h.entries(source), None);
    manual.run_all();
    assert_eq!(h.entries(source).unwrap().len(), 2);
}

#[test]
fn past_its_limit_the_file_is_compacted() {
    let manual = Manual::new();
    let store = Arc::new(MemoryHistory::default());
    let source = DataSourceId::new();
    let h = Arc::new(Histories::new(Arc::clone(&store) as _, manual.runtime()));
    h.set_limit(Some(10));
    for i in 0..40 {
        h.record(entry(source, &format!("SELECT {i}"), i));
    }
    assert_eq!(h.entries(source).unwrap().len(), 10);
    assert!(store.blocks(source).len() <= 10 + 16);
}

/// A broken file kept aside is said once.
#[derive(Debug, Default)]
struct Broken;

impl HistoryStore for Broken {
    fn load(&self, _source: DataSourceId) -> Result<Loaded, String> {
        Ok(Loaded { entries: Vec::new(), broken: Some("a.toml.broken".into()) })
    }

    fn append(&self, _source: DataSourceId, _entry: &HistoryEntry) -> Result<(), String> {
        Ok(())
    }

    fn rewrite(&self, _source: DataSourceId, _entries: &[HistoryEntry]) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn a_broken_file_is_told_once() {
    let manual = Manual::new();
    let h = Arc::new(Histories::new(Arc::new(Broken), manual.runtime()));
    let source = DataSourceId::new();
    h.record(entry(source, "SELECT 1", 1));
    assert_eq!(h.take_broken(), [std::path::PathBuf::from("a.toml.broken")]);
    assert!(h.take_broken().is_empty());
}
