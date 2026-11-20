use super::*;

fn row(pid: u32, parent: Option<u32>, start: u64) -> Row {
    Row { pid, parent, start, name: format!("p{pid}") }
}

/// Children and theirs, each once; the root not among them; nothing beside the tree.
#[test]
fn the_tree_under_a_root_is_found_and_nothing_else() {
    let rows = [
        row(1, None, 0),
        row(10, Some(1), 100),
        row(11, Some(10), 110),
        row(12, Some(11), 120),
        row(13, Some(10), 111),
        row(20, Some(1), 105),
        row(21, Some(20), 106),
    ];
    let mut pids = tree(&rows, 10).into_iter().map(|f| f.pid).collect::<Vec<_>>();
    pids.sort_unstable();
    assert_eq!(pids, [11, 12, 13]);
    assert!(tree(&rows, 99).is_empty(), "a root not running has none");
}

/// An id reused by a program younger than one that names it as parent never adopts it;
/// a cycle a stale table could hold is walked once.
#[test]
fn a_reused_id_adopts_no_older_program_and_a_cycle_ends() {
    let rows = [
        row(10, None, 100),
        // Started before 10 did: its parent was another program with the same id.
        row(30, Some(10), 50),
        row(31, Some(30), 60),
        row(11, Some(10), 101),
        row(12, Some(11), 102),
        row(11, Some(12), 102),
    ];
    let pids = tree(&rows, 10).into_iter().map(|f| f.pid).collect::<Vec<_>>();
    assert!(!pids.contains(&30) && !pids.contains(&31), "{pids:?}");
    assert_eq!(pids.iter().filter(|p| **p == 11).count(), 1);
    assert!(pids.contains(&12));
}

/// A real tree: a shell with a program it started in the background, both found under it
/// and both stopped; the shell's own parent (this test) untouched.
#[cfg(unix)]
#[test]
fn a_started_program_and_what_it_started_are_stopped() {
    use std::process::{Command, Stdio};
    let mut shell = Command::new("/bin/sh")
        .args(["-c", "sleep 61 & sleep 62; wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let root = shell.id();
    // The shell starts both of its sleeps.
    let since = Instant::now();
    let mut found = Descendants::of(root);
    while found.len() < 2 && since.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        found = Descendants::of(root);
    }
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.found().iter().all(|f| f.pid != root && f.pid != std::process::id()));
    assert!(found.found().iter().all(|f| f.name == "sleep"), "{found:?}");
    let mut told = Vec::new();
    found.stop_each(Duration::from_secs(2), |pid, ended| told.push((pid, ended)));
    told.sort_unstable_by_key(|(pid, _)| *pid);
    let mut expected = found.found().iter().map(|f| (f.pid, Ended::Asked)).collect::<Vec<_>>();
    expected.sort_unstable_by_key(|(pid, _)| *pid);
    assert_eq!(told, expected, "each told once, as it ended when asked");
    assert!(Descendants::of(root).is_empty());
    // The shell, its children gone, ends by itself.
    let since = Instant::now();
    while shell.try_wait().unwrap().is_none() && since.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = shell.kill();
    let _ = shell.wait();
    // Gone already: each told as ended, none forced.
    assert_eq!(found.stop(Duration::from_millis(50)), 0);
}

/// One that ignores being asked is made to end once the grace is over, and told so.
#[cfg(unix)]
#[test]
fn one_that_won_t_end_when_asked_is_made_to() {
    use std::process::{Command, Stdio};
    let mut shell = Command::new("/bin/sh")
        .args(["-c", "trap '' TERM; sleep 63 & wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let root = shell.id();
    let since = Instant::now();
    let mut found = Descendants::of(root);
    while found.is_empty() && since.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        found = Descendants::of(root);
    }
    // The sleep ignores TERM as its shell did when it started it.
    let mut told = Vec::new();
    found.stop_each(Duration::from_millis(200), |pid, ended| told.push((pid, ended)));
    assert_eq!(told.len(), found.len());
    assert!(told.iter().all(|(_, e)| *e == Ended::Forced), "{told:?}");
    let _ = shell.kill();
    let _ = shell.wait();
}
