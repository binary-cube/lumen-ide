use super::*;
use std::sync::Mutex;

/// What the index was asked, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Changed(Vec<PathBuf>),
    Refresh,
    Rebuild,
    Busy,
}

/// An index wanting `.php` files only (files indexed under `/p/src/lib`), recording what
/// it's asked.
#[derive(Default)]
struct Recorder(Mutex<Vec<Call>>);

impl Sink for Recorder {
    fn wants(&self, path: &Path) -> bool {
        path.extension().is_some_and(|e| e == "php")
    }
    fn indexed_under(&self, dir: &Path) -> Vec<PathBuf> {
        let indexed = PathBuf::from("/p/src/lib/A.php");
        if indexed.starts_with(dir) { vec![indexed] } else { Vec::new() }
    }
    fn changed(&self, paths: Vec<PathBuf>) {
        self.0.lock().unwrap().push(Call::Changed(paths));
    }
    fn refresh(&self) {
        self.0.lock().unwrap().push(Call::Refresh);
    }
    fn rebuild(&self) {
        self.0.lock().unwrap().push(Call::Rebuild);
    }
    fn mark_busy(&self) {
        self.0.lock().unwrap().push(Call::Busy);
    }
}

/// A feed with short waits (the test doesn't wait 300 ms each time).
fn feed() -> (EventBus, Arc<Recorder>, Feed) {
    let events = EventBus::new();
    let recorder = Arc::new(Recorder::default());
    let feed = Feed::start_timed(
        &events,
        Arc::clone(&recorder) as Arc<dyn Sink>,
        Duration::from_millis(40),
        Duration::from_millis(400),
    )
    .unwrap();
    (events, recorder, feed)
}

/// Waits (5 s at most) until the index was asked `n` times; what it was asked.
fn calls(recorder: &Recorder, n: usize) -> Vec<Call> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while recorder.0.lock().unwrap().len() < n && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    // Nothing more is coming in a while.
    std::thread::sleep(Duration::from_millis(100));
    recorder.0.lock().unwrap().clone()
}

fn php(name: &str) -> PathBuf {
    PathBuf::from(format!("/p/{name}.php"))
}

#[test]
fn a_file_saved_written_and_renamed_meanwhile_is_reindexed_once() {
    let (events, recorder, feed) = feed();
    events.publish(&FileSaved { path: php("a") });
    // Our own save, seen by the watcher too; again; another file; a README nobody indexes.
    events.publish(&FilesWritten { paths: vec![php("a"), PathBuf::from("/p/README.md")] });
    events.publish(&FileSaved { path: php("a") });
    events.publish(&FileRenamed { from: php("b"), to: php("c") });
    assert_eq!(calls(&recorder, 1), [Call::Changed(vec![php("a"), php("b"), php("c")])]);
    let stats = feed.stats();
    assert_eq!((stats.batches, stats.merged), (1, 2), "a.php asked 3 times: twice merged");
}

#[test]
fn a_paste_of_many_files_is_one_batch() {
    let (events, recorder, feed) = feed();
    events.publish(&BulkChangeStarted { id: 42 });
    for i in 0..1000 {
        events.publish(&EntriesChanged { paths: vec![php(&format!("f{}", i % 100))] });
    }
    // Long past the quiet and the limit: held while the paste runs.
    std::thread::sleep(Duration::from_millis(500));
    assert!(recorder.0.lock().unwrap().is_empty());
    events.publish(&BulkChangeEnded { id: 42 });
    let got = calls(&recorder, 1);
    assert_eq!(got.len(), 1, "one reindex");
    assert!(matches!(&got[0], Call::Changed(paths) if paths.len() == 100));
    assert_eq!(feed.stats().batches, 1);
}

#[test]
fn a_folder_s_files_are_reindexed_and_a_gone_one_drops_its_own() {
    // The folders the languages' modules register (Composer's, npm's, the builds').
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    // A folder made on disk (a scratch one): its files, walked; not the whole project.
    let dir = std::env::temp_dir().join(format!("feed-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub/node_modules")).unwrap();
    std::fs::write(dir.join("sub/A.php"), "").unwrap();
    std::fs::write(dir.join("sub/notes.txt"), "").unwrap();
    std::fs::write(dir.join("sub/node_modules/B.php"), "").unwrap();
    let (events, recorder, _feed) = feed();
    events.publish(&EntriesChanged { paths: vec![dir.join("sub")] });
    assert_eq!(calls(&recorder, 1), [Call::Changed(vec![dir.join("sub/A.php")])]);
    recorder.0.lock().unwrap().clear();
    // A `Makefile` changed, a folder gone that held nothing indexed: nothing to do.
    std::fs::write(dir.join("Makefile"), "").unwrap();
    events.publish(&EntriesChanged {
        paths: vec![dir.join("Makefile"), PathBuf::from("/p/gone-empty")],
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(recorder.0.lock().unwrap().is_empty());
    // A folder gone with files indexed in it: those files (missing, they're dropped), not
    // the whole project.
    events.publish(&EntriesDeleted { paths: vec![PathBuf::from("/p/src/lib"), php("x")] });
    assert_eq!(
        calls(&recorder, 1),
        [Call::Changed(vec![PathBuf::from("/p/src/lib/A.php"), php("x")])]
    );
    recorder.0.lock().unwrap().clear();
    // A rescan: the whole project, once.
    events.publish(&EntriesRescan { root: PathBuf::from("/p") });
    assert_eq!(calls(&recorder, 1), [Call::Refresh]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refactoring_goes_at_once_and_a_rebuild_asked_twice_runs_once() {
    let (events, recorder, feed) = feed();
    let started = std::time::Instant::now();
    events.publish(&RefactoringApplied {
        changed: vec![php("User"), php("Open")],
        unsaved: vec![php("Open")],
        renamed: vec![(php("Old"), php("New"))],
    });
    let got = calls(&recorder, 1);
    assert!(started.elapsed() < Duration::from_millis(400) + Duration::from_millis(100));
    assert_eq!(got, [Call::Changed(vec![php("New"), php("Old"), php("User")])], "sorted");
    recorder.0.lock().unwrap().clear();
    // A save waiting, then the rebuild asked twice before it starts (held by a batch of
    // changes meanwhile, so the test doesn't race the queue's thread): busy at once, one
    // rebuild, which covers the save.
    events.publish(&BulkChangeStarted { id: 5 });
    events.publish(&FileSaved { path: php("a") });
    let reindex = || ReindexRequested { root: PathBuf::from("/p") };
    events.publish(&reindex());
    events.publish(&reindex());
    events.publish(&BulkChangeEnded { id: 5 });
    assert_eq!(calls(&recorder, 3), [Call::Busy, Call::Busy, Call::Rebuild]);
    let _ = feed;
}

#[test]
fn closed_with_the_project_it_asks_nothing_more() {
    let (events, recorder, feed) = feed();
    drop(feed);
    events.publish(&FileSaved { path: php("a") });
    std::thread::sleep(Duration::from_millis(200));
    assert!(recorder.0.lock().unwrap().is_empty());
}

/// Two projects (two windows), each with its own events: a long paste in one holds nothing
/// in the other.
#[test]
fn another_project_s_batch_never_holds_this_one() {
    let (events, recorder, _feed) = feed();
    let (other_events, other_recorder, _other) = feed();
    other_events.publish(&BulkChangeStarted { id: 7 });
    other_events.publish(&FileSaved { path: php("theirs") });
    events.publish(&FileSaved { path: php("ours") });
    assert_eq!(calls(&recorder, 1), [Call::Changed(vec![php("ours")])]);
    assert!(other_recorder.0.lock().unwrap().is_empty(), "theirs waits for its paste");
    other_events.publish(&BulkChangeEnded { id: 7 });
    assert_eq!(calls(&other_recorder, 1), [Call::Changed(vec![php("theirs")])]);
}
