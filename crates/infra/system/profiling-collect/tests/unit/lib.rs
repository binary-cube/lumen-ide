use super::*;

/// A thread's CPU since the last sample (one new since counts from zero, threads of one
/// name together, an idle one left out).
#[test]
fn a_sample_counts_each_thread_s_time_since_the_last() {
    let ms = Duration::from_millis;
    let before = vec![(1, "main".to_owned(), ms(100)), (2, "worker".to_owned(), ms(10))];
    let now = vec![
        (1, "main".to_owned(), ms(130)),
        (2, "worker".to_owned(), ms(15)),
        (3, "worker".to_owned(), ms(5)),
        (4, "idle".to_owned(), ms(0)),
    ];
    let s = sample(ms(0), ms(100), &before, &now, Some(7));
    assert_eq!(s.span_ms, 100);
    assert_eq!(s.cpu_us, 40_000);
    let rows: Vec<(&str, u64)> = s.threads.iter().map(|t| (t.thread.as_str(), t.us)).collect();
    assert_eq!(rows, [("main", 30_000), ("worker", 10_000)]);
    assert_eq!(s.gpu, Some(7));
}

/// A short recording: its probes counted, stopped when asked, written as JSON that reads
/// back, with only the system's basics; a second one refused while it runs.
/// One recording at a time in the whole process: the tests that start one take turns.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn a_recording_is_made_and_written() {
    let _turn = turn();
    let dir = std::env::temp_dir().join(format!("profiling-{}", uuid::Uuid::new_v4()));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let plan = Plan {
        dir: dir.clone(),
        length: Duration::from_secs(5),
        interval: Duration::from_millis(50),
        gpu: "Some GPU".into(),
        gpu_driver: "1.0".into(),
        scale: 2.0,
        app_version: "1.0.0".into(),
        build: "debug".into(),
        owners: Arc::new(|| Owners(vec![("main".into(), "app".into())])),
        gpu_memory: Arc::new(|| Some(1024)),
        threads: Arc::new(Vec::new),
        memory: Arc::new(|| Some(1)),
        io: Arc::new(Vec::new),
        disk: Arc::new(|| None),
        readings: Arc::new(|| (Vec::new(), Vec::new())),
        tick: Arc::new(|| {}),
        done: Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        }),
    };
    let recorder = start(plan).unwrap();
    assert!(start_refused(&dir), "one at a time");
    std::thread::sleep(Duration::from_millis(80));
    kernel::probe::trigger("test-view", kernel::probe::Cause::Poll);
    kernel::probe::count("test-part", "items", 7);
    std::thread::sleep(Duration::from_millis(80));
    recorder.stop();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let path = recorder.finished().unwrap().unwrap();
    assert!(recorder.finished().is_none(), "told once");
    let recording = Recording::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!recording.samples.is_empty());
    assert_eq!(recording.system.gpu, "Some GPU");
    assert_eq!(recording.owners.of("main"), Some("app"));
    let triggers: u64 = recording.samples.iter().flat_map(|s| &s.triggers).map(|t| t.count).sum();
    assert_eq!(triggers, 1);
    let counted: u64 = recording.samples.iter().flat_map(|s| &s.counters).map(|c| c.count).sum();
    assert_eq!(counted, 7, "a part's own count, kept with nothing else changed");
    assert!(!kernel::probe::recording(), "the probes off again");
    let _ = std::fs::remove_dir_all(dir);
}

/// Whether another recording would be refused now (one started to see is ended at once).
fn start_refused(dir: &Path) -> bool {
    let plan = Plan {
        dir: dir.to_path_buf(),
        length: Duration::from_millis(1),
        interval: Duration::from_millis(1),
        gpu: String::new(),
        gpu_driver: String::new(),
        scale: 1.0,
        app_version: String::new(),
        build: String::new(),
        owners: Arc::new(Owners::default),
        gpu_memory: Arc::new(|| None),
        threads: Arc::new(Vec::new),
        memory: Arc::new(|| Some(1)),
        io: Arc::new(Vec::new),
        disk: Arc::new(|| None),
        readings: Arc::new(|| (Vec::new(), Vec::new())),
        done: Arc::new(|| {}),
        tick: Arc::new(|| {}),
    };
    // Not refused: the one it started is ended and waited for (it holds the slot).
    let Ok(started) = start(plan) else { return true };
    started.stop();
    let since = std::time::Instant::now();
    while started.finished().is_none() && since.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// What a part moved in a sample: since the last one (one new counts from zero; one that
/// moved nothing left out).
#[test]
fn network_and_disk_count_since_the_last_sample() {
    let io =
        |owner: &str, sent, read| Io { owner: owner.into(), sent, received: 0, read, written: 0 };
    let before = vec![io("http-client", 100, 0), io("indexer", 0, 50)];
    let now = vec![io("http-client", 160, 0), io("indexer", 0, 50), io("remote", 7, 0)];
    let moved = io_since(&before, &now);
    assert_eq!(moved, [io("http-client", 60, 0), io("remote", 7, 0)]);
}

/// The recorder never counts itself: its own threads are left out of what it records; the
/// threads without a name are the system's workers.
#[test]
fn the_recorder_s_own_threads_are_left_out() {
    let ms = Duration::from_millis;
    let plan = Plan {
        dir: std::env::temp_dir(),
        length: ms(1),
        interval: ms(1),
        gpu: String::new(),
        gpu_driver: String::new(),
        scale: 1.0,
        app_version: String::new(),
        build: String::new(),
        owners: Arc::new(Owners::default),
        gpu_memory: Arc::new(|| None),
        threads: Arc::new(move || {
            vec![
                (1, "main".to_owned(), ms(5)),
                (2, THREAD.to_owned(), ms(9)),
                (3, READ_THREAD.to_owned(), ms(9)),
                (4, String::new(), ms(2)),
            ]
        }),
        memory: Arc::new(|| Some(1)),
        io: Arc::new(Vec::new),
        disk: Arc::new(|| None),
        readings: Arc::new(|| (Vec::new(), Vec::new())),
        done: Arc::new(|| {}),
        tick: Arc::new(|| {}),
    };
    let names: Vec<String> =
        thread_times(&plan, &Owners::default()).into_iter().map(|t| t.1).collect();
    assert_eq!(names, ["main", SYSTEM_WORKERS]);
}

/// A thread's name kept anonymous: one the app knows as it is; one after a known prefix
/// (a language server's, named by the user) as the prefix; any other only up to its first
/// `-` or space.
#[test]
fn thread_names_are_made_anonymous() {
    let owners =
        Owners(vec![("indexer".into(), "code.index".into()), ("lsp-".into(), "lsp".into())]);
    assert_eq!(anonymous_thread("indexer", &owners), "indexer");
    assert_eq!(anonymous_thread("lsp-acme billing", &owners), "lsp-*");
    assert_eq!(anonymous_thread("tokio-runtime-worker", &owners), "tokio-*");
    assert_eq!(anonymous_thread("main", &owners), "main");
    assert_eq!(anonymous_thread("my secret thread", &owners), "my-*");
}

/// A module's figures failing mid-recording: the probes go off, the slot is free again, and
/// whoever waits is told (never left waiting).
#[test]
fn a_failure_while_recording_leaves_nothing_on() {
    let _turn = turn();
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let plan = Plan {
        dir: std::env::temp_dir(),
        length: Duration::from_secs(5),
        interval: Duration::from_millis(30),
        gpu: String::new(),
        gpu_driver: String::new(),
        scale: 1.0,
        app_version: String::new(),
        build: String::new(),
        owners: Arc::new(Owners::default),
        gpu_memory: Arc::new(|| None),
        threads: Arc::new(Vec::new),
        memory: Arc::new(|| None),
        io: Arc::new(Vec::new),
        disk: Arc::new(|| None),
        readings: Arc::new(|| panic!("a module's figures failed")),
        tick: Arc::new(|| {}),
        done: Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        }),
    };
    let recorder = start(plan).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).expect("told it ended");
    assert!(recorder.finished().unwrap().is_err());
    assert!(!kernel::probe::recording(), "the probes off again");
    assert!(!start_refused(&std::env::temp_dir()), "the slot free again");
}
