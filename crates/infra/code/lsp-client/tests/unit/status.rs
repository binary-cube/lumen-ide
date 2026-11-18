use std::time::{Duration, Instant};

use super::*;

#[test]
fn percentiles_follow_the_latencies() {
    let status = Status::default();
    let now = Instant::now();
    assert_eq!(status.snapshot(now).p50, None);
    for ms in 1..=100 {
        status.answered(Duration::from_millis(ms));
    }
    let snapshot = status.snapshot(now);
    assert_eq!(snapshot.p50, Some(Duration::from_millis(50)));
    assert_eq!(snapshot.p95, Some(Duration::from_millis(95)));
}

#[test]
fn requests_per_minute_count_the_last_minute() {
    let status = Status::default();
    let start = Instant::now();
    status.requested(start);
    status.requested(start + Duration::from_secs(30));
    status.requested(start + Duration::from_secs(61));
    assert_eq!(status.snapshot(start + Duration::from_secs(61)).per_minute, 2);
    assert_eq!(status.snapshot(start + Duration::from_secs(200)).per_minute, 0);
}

#[test]
fn the_log_keeps_its_last_lines_and_says_when_it_changed() {
    let status = Status::default();
    let before = status.log_generation();
    status.log("one\ntwo");
    for i in 0..LOG_LINES {
        status.log(format!("line {i}"));
    }
    let (lines, generation) = status.log_lines();
    assert_eq!(lines.len(), LOG_LINES);
    assert_eq!(lines.last().map(String::as_str), Some(format!("line {}", LOG_LINES - 1).as_str()));
    assert!(generation > before);
}

/// A view's window of the log: only its lines, with how many there are in all.
#[test]
fn a_window_of_the_log_copies_only_its_lines() {
    let status = Status::default();
    for i in 0..10 {
        status.log(format!("line {i}"));
    }
    let (lines, len, generation) = status.log_window(|len| (len - 3, 5));
    assert_eq!(lines, ["line 7", "line 8", "line 9"]);
    assert_eq!(len, 10);
    assert_eq!(generation, status.log_generation());
    assert!(status.log_window(|_| (20, 5)).0.is_empty());
}

#[test]
fn an_error_is_kept_and_logged() {
    let status = Status::default();
    status.error("timed out");
    assert_eq!(status.snapshot(Instant::now()).last_error.as_deref(), Some("timed out"));
    assert_eq!(status.log_lines().0, ["error: timed out"]);
}

#[test]
fn features_are_answered_only_when_ready_and_supported() {
    let status = Status::default();
    status.set_capabilities(Capabilities { hover: true, ..Capabilities::default() });
    assert!(!status.answers(Feature::Hover));
    status.set_state(State::Ready);
    assert!(status.answers(Feature::Hover));
    assert!(!status.answers(Feature::Completion));
    status.set_state(State::Idle);
    assert!(status.answers(Feature::Hover));
    status.set_state(State::Restarting);
    assert!(!status.answers(Feature::Hover));
}

#[test]
fn progress_shows_the_furthest_work_and_ends_with_it() {
    let status = Status::default();
    status.set_state(State::Ready);
    status.progress("a", Some(("Indexing".into(), Some(40))));
    status.progress("b", Some(("Loading".into(), None)));
    assert_eq!(status.snapshot(Instant::now()).progress, Some(("Indexing".into(), Some(40))));
    status.progress("a", None);
    status.progress("b", None);
    assert_eq!(status.snapshot(Instant::now()).progress, None);
}

#[test]
fn a_time_is_stamped_as_a_utc_date() {
    let at = std::time::SystemTime::UNIX_EPOCH + Duration::from_millis(1_793_611_837_485);
    assert_eq!(stamp(at), "2026-11-02 09:30:37.485");
    assert_eq!(stamp(std::time::SystemTime::UNIX_EPOCH), "1970-01-01 00:00:00.000");
    let leap = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(951_782_400);
    assert_eq!(stamp(leap), "2000-02-29 00:00:00.000");
}

#[test]
fn the_log_is_kept_on_disk_too_with_what_it_held() {
    let dir = std::env::temp_dir().join(format!("lsp-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("logs").join("server.log");
    let status = Status::default();
    status.log("before");
    status.keep_log_in(&path);
    status.log("after\nand more");
    let written = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines.len(), 3, "{written}");
    assert!(lines[0].ends_with(" before") && lines[2].ends_with(" and more"), "{written}");
    // Past its size it's moved aside and started again.
    let long = "x".repeat(64 * 1024);
    for _ in 0..(LOG_FILE_BYTES / (64 * 1024) + 2) {
        status.log(long.as_str());
    }
    assert!(dir.join("logs").join("server.1.log").exists());
    assert!(std::fs::metadata(&path).unwrap().len() < LOG_FILE_BYTES);
    let _ = std::fs::remove_dir_all(&dir);
}
