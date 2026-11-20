use super::*;

/// Nothing counts while nothing records; recording, the triggers, the parts' time and the
/// frames are counted and handed over once (then anew); stopped, nothing again.
#[test]
fn the_probes_count_only_while_recording() {
    // One test for the whole table (it's global): the steps in order.
    stop();
    trigger("idle", Cause::Poll);
    drop(time("idle", "paint"));
    assert_eq!(take(), Gathered::default());
    start();
    mark_interface_thread();
    trigger("log-viewer", Cause::Poll);
    trigger("log-viewer", Cause::Poll);
    assert!(asked("caret", Cause::Deadline, true));
    assert!(!asked("caret", Cause::Animation, false));
    {
        let _t = time("log-viewer", "paint");
        std::thread::sleep(Duration::from_millis(2));
    }
    frame(Duration::from_millis(5));
    frame(Duration::from_millis(9));
    count("indexer", "files", 3);
    count("indexer", "files", 2);
    level("lsp", "pending", 4);
    level("lsp", "pending", 1);
    let got = take();
    assert_eq!(got.triggers, [("log-viewer", Cause::Poll, 2), ("caret", Cause::Deadline, 1)]);
    let (owner, what, busy) = got.busy[0];
    assert_eq!((owner, what, busy.calls), ("log-viewer", "paint", 1));
    assert!(busy.total >= Duration::from_millis(2));
    assert_eq!(got.frames.count, 2);
    assert_eq!(got.frames.longest, Duration::from_millis(9));
    assert_eq!(got.counts, [("indexer", "files", 5)]);
    assert_eq!(got.levels, [("lsp", "pending", 4, 1)], "the highest and the last");
    assert_eq!(take().frames.count, 0, "handed over: anew");
    // Timed work nested: each way down kept with its calls and time (the outer one's with
    // what it held), as a flame graph draws it.
    {
        let _frame = time("test-app", "frame");
        {
            let _paint = time("test-view", "paint");
            std::thread::sleep(Duration::from_millis(2));
        }
        let _poll = time("test-view", "poll");
    }
    let got = take();
    let ours: Vec<_> = got.spans.iter().filter(|s| s.0[0].0 == "test-app").collect();
    let mut paths: Vec<_> = ours.iter().map(|s| s.0.clone()).collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            vec![("test-app", "frame")],
            vec![("test-app", "frame"), ("test-view", "paint")],
            vec![("test-app", "frame"), ("test-view", "poll")],
        ]
    );
    let total = |p: &[(&str, &str)]| ours.iter().find(|s| s.0 == p).map(|s| s.2).unwrap();
    // The interface's time is each step's own: the frame's is less its paint's.
    let own = |w: &str| got.busy.iter().find(|b| b.0 == "test-app" && b.1 == w).map(|b| b.2.total);
    assert!(own("frame").unwrap() < total(&[("test-app", "frame")]));
    // Another thread's timed work: in the ways down, never the interface's time.
    std::thread::spawn(|| drop(time("test-worker", "request"))).join().unwrap();
    let later = take();
    assert!(later.spans.iter().any(|s| s.0 == [("test-worker", "request")]));
    assert!(later.busy.iter().all(|b| b.0 != "test-worker"));
    assert!(
        total(&[("test-app", "frame")]) >= total(&[("test-app", "frame"), ("test-view", "paint")])
    );
    stop();
    trigger("after", Cause::Input);
    assert_eq!(take(), Gathered::default());
}

/// Off, a probe is next to nothing (one atomic load).
#[test]
#[ignore = "a measurement"]
fn a_probe_off_costs_next_to_nothing() {
    stop();
    const N: u32 = 10_000_000;
    let started = Instant::now();
    for _ in 0..N {
        trigger(std::hint::black_box("x"), Cause::Poll);
        drop(time(std::hint::black_box("x"), "paint"));
    }
    let each = started.elapsed() / N;
    println!("a probe off: {each:?} each");
    assert!(each < Duration::from_nanos(20));
}
