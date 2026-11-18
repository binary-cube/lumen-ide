use super::*;

fn record(pid: u32, started: u64, kind: &str) -> Record {
    Record { pid, started, kind: kind.into(), program: "x".into(), project: None }
}

fn seen(list: &[(u32, Option<u32>, u64)]) -> HashMap<u32, Seen> {
    list.iter().map(|&(pid, parent, started)| (pid, Seen { pid, parent, started })).collect()
}

/// A pid running another program now (another start time) is not the one recorded.
#[test]
fn a_reused_pid_is_not_the_program_recorded() {
    let now = seen(&[(100, Some(1), 50)]);
    assert!(still_runs(&record(100, 50, "terminal"), &now));
    assert!(!still_runs(&record(100, 49, "terminal"), &now));
    assert!(!still_runs(&record(101, 50, "terminal"), &now));
}

/// What a program started (and they started) goes with it; an older process with a reused
/// parent pid doesn't.
#[test]
fn a_programs_tree_is_what_it_started() {
    let now = seen(&[
        (100, Some(1), 50),
        (101, Some(100), 51),
        (102, Some(101), 52),
        (103, Some(100), 40),
        (104, Some(1), 60),
    ]);
    let mut tree = tree_of(&now[&100], &now);
    tree.sort_unstable();
    assert_eq!(tree, vec![(100, 50), (101, 51), (102, 52)]);
}

/// Helpers to stop, the user's work to ask about, ended ones and hostile pids dropped.
#[test]
fn helpers_are_stopped_and_the_users_work_asked() {
    let now = seen(&[(100, Some(1), 50), (200, Some(1), 60), (300, Some(1), 70), (1, None, 1)]);
    let (stop, ask) = decide(
        vec![
            record(100, 50, "server"),
            record(200, 60, "terminal"),
            record(300, 71, "agent"),
            record(400, 80, "task"),
            record(1, 1, "git"),
            record(std::process::id(), 0, "other"),
        ],
        &now,
    );
    assert_eq!(stop.iter().map(|(r, _)| r.pid).collect::<Vec<_>>(), vec![100]);
    assert_eq!(ask.iter().map(|(r, _)| r.pid).collect::<Vec<_>>(), vec![200]);
}

/// The start-up look with programs of its own: a dead instance's server (and what it
/// started) stopped, its terminal kept and asked about, a pid with another start time left
/// alone; the file taken, then removed; a live instance's file never touched.
#[cfg(unix)]
#[test]
fn the_start_up_look_stops_helpers_and_finds_the_users_work() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(kernel::id::uuid_text(kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut server = Command::new("sh").args(["-c", "sleep 60 & wait"]).spawn().unwrap();
    let mut terminal = Command::new("sleep").arg("60").spawn().unwrap();
    let mut other = Command::new("sleep").arg("60").spawn().unwrap();
    // Their children appear.
    std::thread::sleep(Duration::from_millis(200));
    let mut system = System::new();
    let now = look(&mut system);
    let started = |pid: u32| now[&pid].started;
    let server_child = now.values().find(|s| s.parent == Some(server.id())).map(|s| s.pid);
    let server_child = server_child.expect("the server's child");
    let at = |pid: u32, kind: &str| Record {
        pid,
        started: started(pid),
        kind: kind.into(),
        program: "sleep".into(),
        project: Some("/work/shop".into()),
    };
    // This process, started at another time: an instance that's gone.
    let dead = InstanceFile {
        app_pid: std::process::id(),
        app_started: 1,
        programs: vec![
            at(server.id(), "server"),
            at(terminal.id(), "terminal"),
            Record { started: started(other.id()) + 1, ..at(other.id(), "terminal") },
        ],
    };
    let dead_file = dir.join("dead.toml");
    std::fs::write(&dead_file, dead.to_text()).unwrap();
    // This very process: alive, never touched.
    let alive = InstanceFile {
        app_pid: std::process::id(),
        app_started: started(std::process::id()),
        programs: vec![at(other.id(), "server")],
    };
    let alive_file = dir.join("alive.toml");
    std::fs::write(&alive_file, alive.to_text()).unwrap();
    let own = dir.join("own.toml");

    let mut found = survey(&dir, &own);
    assert_eq!(found.stopped, 1);
    assert!(server.wait().is_ok());
    let system_after = look(&mut System::new());
    assert!(!system_after.contains_key(&server_child), "what the server started stopped too");
    assert_eq!(found.asked.len(), 1);
    let asked = found.asked[0].clone();
    assert_eq!(asked.record.pid, terminal.id());
    assert_eq!(asked.tree[0], (terminal.id(), started(terminal.id())));
    assert!(!asked.working(), "a sleep waits");
    assert!(terminal.try_wait().unwrap().is_none(), "the user's work kept");
    assert!(other.try_wait().unwrap().is_none(), "a reused pid left alone");
    assert!(!dead_file.exists() && dir.join("dead.taken").exists(), "taken");
    found.done();
    assert!(!dir.join("dead.taken").exists(), "removed once handled");
    assert!(alive_file.exists(), "a live instance's file kept");
    // Nothing left to find the second time.
    assert!(survey(&dir, &own).asked.is_empty());

    // Stopping the user's work: asked, then made to.
    assert_eq!(crate::stop::stop(&asked.tree, Duration::from_secs(2)), 1);
    assert!(terminal.wait().is_ok());
    other.kill().unwrap();
    other.wait().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A program left running is known to run while it does, then not once it ended (and a
/// pid with another start time never is).
#[test]
fn a_kept_program_is_seen_to_end() {
    let mut program = std::process::Command::new("sleep").arg("60").spawn().unwrap();
    let pid = program.id();
    let mut system = System::new();
    let started = look(&mut system)[&pid].started;
    let found =
        |started: u64| Found { record: record(pid, started, "terminal"), tree: vec![], cpu: 0.0 };
    let both = [found(started), found(started.saturating_add(7))];
    assert_eq!(still_running(&mut system, &both), [true, false]);
    program.kill().unwrap();
    program.wait().unwrap();
    assert_eq!(still_running(&mut system, &both[..1]), [false]);
}
