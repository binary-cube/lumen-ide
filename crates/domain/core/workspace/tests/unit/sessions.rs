//! The app's sessions: a new one first once something's open, its windows kept as they
//! change (nothing open leaves the last ones), ended cleanly or not, a few kept.

use super::*;

fn window(project: &str, files: &[&str]) -> SessionWindow {
    SessionWindow {
        project: (!project.is_empty()).then(|| PathBuf::from(project)),
        files: files.iter().map(PathBuf::from).collect(),
    }
}

#[test]
fn a_session_is_recorded_as_it_changes_and_kept_when_everything_closes() {
    let mut sessions = Sessions::default();
    assert!(!sessions.record(1, 100, Vec::new()), "nothing open yet: no session");
    assert!(sessions.record(1, 100, vec![window("/p/shop", &["/p/shop/a.php"])]));
    assert!(!sessions.record(1, 110, vec![window("/p/shop", &["/p/shop/a.php"])]), "the same");
    assert!(sessions.record(1, 120, vec![window("/p/shop", &[]), window("", &["/tmp/notes.md"])]));
    assert_eq!(sessions.list[0].updated, 120);
    assert_eq!(sessions.list[0].started, 100);
    assert!(!sessions.record(1, 130, Vec::new()), "all closed: what it had stays");
    assert_eq!(sessions.list[0].windows.len(), 2);
    assert_eq!(sessions.list[0].file_count(), 1);
    assert!(!sessions.list[0].clean, "not ended yet: as after a crash");
    assert!(sessions.end(1) && sessions.list[0].clean);
}

#[test]
fn the_past_sessions_are_the_newest_first_but_this_one() {
    let mut sessions = Sessions::default();
    for id in 1..=5 {
        sessions.record(id, id as u64, vec![window(&format!("/p/{id}"), &[])]);
    }
    sessions.keep(3);
    let ids: Vec<u128> = sessions.list.iter().map(|s| s.id).collect();
    assert_eq!(ids, [5, 4, 3]);
    let past: Vec<u128> = sessions.past(5).map(|s| s.id).collect();
    assert_eq!(past, [4, 3]);
}

#[test]
#[allow(clippy::disallowed_methods)] // A moment to count from: the domain never reads the clock.
fn reopening_hands_out_one_window_at_a_time_after_the_one_before_drew() {
    let start = Instant::now();
    let pause = Duration::from_millis(300);
    let windows = vec![window("/p/a", &[]), window("/p/b", &[]), window("", &["/n.md"])];
    let mut reopen = Reopen::new(windows, 0, pause, start);
    assert_eq!(reopen.progress(), (0, 3));
    let first = reopen.next(start).expect("the first one at once");
    assert_eq!(first.project.as_deref(), Some(std::path::Path::new("/p/a")));
    assert!(reopen.next(start).is_none(), "not before its window appeared and drew");
    reopen.appeared(7, start);
    assert_eq!(reopen.waiting_for(), Some(7));
    reopen.drawn(8, start);
    assert!(reopen.next(start).is_none(), "another window's frame doesn't count");
    let drawn = start + Duration::from_millis(50);
    reopen.drawn(7, drawn);
    assert!(reopen.next(drawn).is_none(), "the pause after its first frame");
    assert_eq!(reopen.deadline(), Some(drawn + pause));
    let second = reopen.next(drawn + pause).expect("after the pause");
    assert_eq!(second.project.as_deref(), Some(std::path::Path::new("/p/b")));
    // Skipped (already open): the next one at once.
    reopen.skipped(drawn + pause);
    assert_eq!(reopen.next(drawn + pause).map(|w| w.files.len()), Some(1));
    assert_eq!(reopen.progress(), (3, 3));
    assert!(!reopen.is_done(drawn + pause), "the last one still opening");
    reopen.appeared(9, drawn + pause);
    reopen.drawn(9, drawn + pause);
    assert!(reopen.is_done(drawn + pause));
}

#[test]
#[allow(clippy::disallowed_methods)] // A moment to count from: the domain never reads the clock.
fn reopening_stops_at_the_limit_and_doesnt_wait_forever() {
    let start = Instant::now();
    let windows = vec![window("/p/a", &[]), window("/p/b", &[]), window("/p/c", &[])];
    let mut reopen = Reopen::new(windows, 2, Duration::from_millis(300), start);
    assert_eq!(reopen.progress(), (0, 2), "the limit");
    assert!(reopen.next(start).is_some());
    // Its window never appears: given up on after the wait.
    assert!(reopen.next(start + REOPEN_WAIT / 2).is_none());
    assert!(reopen.next(start + REOPEN_WAIT).is_some());
    assert!(reopen.next(start + REOPEN_WAIT * 3).is_none(), "nothing past the limit");
    assert!(reopen.is_done(start + REOPEN_WAIT * 3));
}

#[test]
fn another_instance_s_sessions_are_kept_when_this_one_writes() {
    let mut here = Sessions::default();
    here.record(1, 100, vec![window("/p/a", &[])]);
    here.record(1, 300, vec![window("/p/a", &["/p/a/x"])]);
    // On disk: another instance's session (2), and an older copy of this one.
    let mut disk = Sessions::default();
    disk.record(2, 200, vec![window("/p/b", &[])]);
    disk.record(1, 100, vec![window("/p/a", &[])]);
    let merged = here.merged(1, disk, 10);
    let ids: Vec<u128> = merged.list.iter().map(|s| s.id).collect();
    assert_eq!(ids, [1, 2], "this one as kept here, the other as on disk, newest first");
    assert_eq!(merged.list[0].file_count(), 1);
    assert_eq!(here.merged(1, merged, 1).list.len(), 1, "the limit");
}
