#![cfg(unix)]

use super::*;

/// A program's output and exit code, in its folder; a failing exit is an outcome, not an
/// error.
#[test]
fn a_program_runs_in_its_folder_and_its_outcome_is_kept() {
    let dir = std::env::temp_dir();
    let out = run(&Run::shell("echo out; echo err 1>&2; pwd; exit 3").dir(&dir)).unwrap();
    assert_eq!(out.code, Some(3));
    assert!(!out.success());
    assert!(out.stdout.starts_with("out"));
    assert_eq!(out.stderr.trim(), "err");
    assert_eq!(out.summary(), "err");
    let shown = std::path::Path::new(out.stdout.lines().nth(1).unwrap());
    assert_eq!(shown.canonicalize().unwrap(), dir.canonicalize().unwrap());
}

/// What's given on stdin reaches it; nothing given: an empty stdin (it doesn't wait).
#[test]
fn stdin_is_written_and_otherwise_empty() {
    let out = run(&Run::shell("cat").stdin("hello")).unwrap();
    assert_eq!(out.stdout, "hello");
    let out = run(&Run::shell("cat").timeout(Duration::from_secs(5))).unwrap();
    assert_eq!(out.stdout, "", "an empty stdin: it ends");
}

/// Stopped after its timeout, or when a newer run replaces it; a missing program said so.
#[test]
fn stopped_by_its_timeout_or_on_request_and_missing_programs_said() {
    let slow = || Run::shell("sleep 5");
    let started = Instant::now();
    assert_eq!(run(&slow().timeout(Duration::from_millis(200))), Err(RunError::TimedOut));
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        flag.store(true, Ordering::Relaxed);
    });
    assert_eq!(run(&slow().stop_when(stop)), Err(RunError::Stopped));
    assert!(started.elapsed() < Duration::from_secs(3), "neither waited the 5 seconds");
    let missing = run(&Run::new("no-such-program-anywhere", [] as [&str; 0]));
    assert!(matches!(missing, Err(RunError::NotFound(_))));
    assert!(which("sh").is_some());
}

/// A program printing without end fills no memory: what's past the limit is dropped, and
/// the program still runs to its end.
#[cfg(unix)]
#[test]
fn outputs_are_capped() {
    let spec = Run::new("sh", ["-c", "head -c 20000000 /dev/zero; echo done >&2"])
        .timeout(Duration::from_secs(30));
    let out = run(&spec).unwrap();
    assert_eq!(out.stdout.len(), MAX_OUTPUT);
    assert_eq!(out.stderr.trim(), "done");
}

/// Stopped after its timeout, a shell goes with what it started: the pipes close, nothing
/// keeps running.
#[cfg(unix)]
#[test]
fn a_timeout_stops_what_the_program_started() {
    let marker = std::env::temp_dir().join(format!("process-tree-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!("(sleep 0.4; touch '{}') & sleep 30", marker.display());
    let spec = Run::new("sh", ["-c", script.as_str()]).timeout(Duration::from_millis(150));
    assert_eq!(run(&spec).unwrap_err(), RunError::TimedOut);
    // Well past the background command's 0.4 s: it would have left its marker.
    std::thread::sleep(Duration::from_millis(700));
    assert!(!marker.exists(), "the background command was stopped too");
}

/// A program kept running: its pipes are the caller's, it's listed while it runs, and it
/// goes (with what it started) when stopped or dropped.
#[test]
fn a_started_program_talks_on_its_pipes_and_is_stopped_when_dropped() {
    use std::io::{BufRead as _, BufReader};
    let running = Arc::new(Running::new());
    let mut started = start(
        &Run::new("sh", ["-c", "read line; echo \"got $line\"; sleep 30"])
            .listed_in(Arc::clone(&running)),
    )
    .unwrap();
    let pid = started.id();
    assert_eq!(running.pids(), vec![pid]);
    started.stdin.as_mut().unwrap().write_all(b"hello\n").unwrap();
    let mut line = String::new();
    BufReader::new(started.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert_eq!(line, "got hello\n");
    assert_eq!(started.exited(), None, "still running");
    assert!(!started.wait_for(Duration::from_millis(50)));
    let begun = Instant::now();
    drop(started);
    assert!(begun.elapsed() < Duration::from_secs(5), "killed, not waited for");
    assert!(running.pids().is_empty(), "off the list");
    assert!(matches!(
        start(&Run::new("surely-not-a-program-here", Vec::<String>::new())),
        Err(RunError::NotFound(_))
    ));
}

/// A program listed in a `Running` asked to stop (the Task Manager, through its part's
/// owner): its run stops it — what it started too — and ends `Stopped`; it's off the list.
#[cfg(unix)]
#[test]
fn a_listed_program_is_stopped_when_asked() {
    let running = Arc::new(Running::new());
    let spec =
        Run::shell("sleep 30").timeout(Duration::from_secs(60)).listed_in(Arc::clone(&running));
    let started = Instant::now();
    let run = std::thread::spawn(move || run(&spec));
    let deadline = Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Some(pid) = running.pids().first().copied() {
            break pid;
        }
        assert!(Instant::now() < deadline, "it started");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(!running.stop(pid + 100_000), "not one of these");
    assert!(running.stop(pid));
    assert!(matches!(run.join().unwrap(), Err(RunError::Stopped)));
    assert!(started.elapsed() < Duration::from_secs(20), "stopped, not waited for");
    assert!(running.pids().is_empty(), "off the list");
    assert!(!running.stop(pid), "gone: nothing to stop");
}
