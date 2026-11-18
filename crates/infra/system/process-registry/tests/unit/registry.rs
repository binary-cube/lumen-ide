#![cfg(unix)]

use super::*;

use process::launched::Kind;

/// A folder of the test's own.
fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(kernel::id::uuid_text(kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn launch(pid: u32, kind: Kind, told_end: bool) -> Launch {
    Launch {
        pid,
        kind,
        program: "sleep".into(),
        project: Some(PathBuf::from("/work/shop")),
        told_end,
    }
}

/// A program started is written with its start time, its end removes it (and the file with
/// nothing left); taken-over ones stay at a normal exit, nothing else does.
#[test]
fn programs_are_written_and_removed() {
    let dir = folder();
    let registry = Registry::open(&dir).unwrap();
    let mut child = std::process::Command::new("sleep").arg("60").spawn().unwrap();
    let pid = child.id();
    registry.started(launch(pid, Kind::Server, true));
    registry.flush();
    let file = InstanceFile::read(registry.file()).expect("written");
    assert_eq!(file.app_pid, std::process::id());
    assert_eq!(file.programs.len(), 1);
    let written = &file.programs[0];
    let started = start_time(&mut System::new(), pid).unwrap();
    assert_eq!((written.pid, written.started), (pid, started));
    assert_eq!((written.kind.as_str(), written.program.as_str()), ("server", "sleep"));
    assert_eq!(written.project.as_deref(), Some("/work/shop"));

    // A pid that isn't running: nothing kept.
    child.kill().unwrap();
    child.wait().unwrap();
    registry.ended(pid);
    registry.started(launch(pid, Kind::Git, true));
    registry.flush();
    assert!(!registry.file().exists(), "nothing runs: no file");

    // Taken over from an earlier run: kept past a normal exit, until answered.
    let earlier = Record {
        pid: 4_000_000,
        started: 5,
        kind: "terminal".into(),
        program: "zsh".into(),
        project: None,
    };
    registry.adopt(vec![earlier.clone()]);
    assert_eq!(InstanceFile::read(registry.file()).unwrap().programs, vec![earlier.clone()]);
    registry.finish();
    assert_eq!(InstanceFile::read(registry.file()).unwrap().programs, vec![earlier]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Answered: no longer kept; a normal exit with nothing to ask removes the file.
#[test]
fn answered_programs_are_let_go_and_a_normal_exit_removes_the_file() {
    let dir = folder();
    let registry = Registry::open(&dir).unwrap();
    let mut child = std::process::Command::new("sleep").arg("60").spawn().unwrap();
    registry.started(launch(child.id(), Kind::Terminal, false));
    let earlier = Record {
        pid: 4_000_001,
        started: 5,
        kind: "agent".into(),
        program: "claude".into(),
        project: None,
    };
    registry.adopt(vec![earlier]);
    assert_eq!(InstanceFile::read(registry.file()).unwrap().programs.len(), 2);
    registry.forget(vec![(4_000_001, 5)]);
    registry.flush();
    assert_eq!(InstanceFile::read(registry.file()).unwrap().programs.len(), 1);
    registry.finish();
    assert!(!registry.file().exists());
    child.kill().unwrap();
    child.wait().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}
