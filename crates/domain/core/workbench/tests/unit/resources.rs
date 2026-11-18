// The tests set up what surrounds the pure code: files on disk, threads.
#![allow(clippy::disallowed_methods)]

use super::*;

#[test]
fn a_part_s_threads_are_named_or_prefixed() {
    let mut git = ResourceUse::new("Git", "vcs.git");
    git.threads = vec!["git-", "symbols"];
    assert!(git.runs_on("git-log") && git.runs_on("symbols"));
    assert!(!git.runs_on("git-") && !git.runs_on("gitx") && !git.runs_on("syntax"));
}

/// Disk and CPU counted as they go, from any thread.
#[test]
fn disk_and_cpu_are_counted_and_reported() {
    let disk = DiskIo::new();
    let cpu = CpuMeter::new();
    std::thread::scope(|s| {
        s.spawn(|| disk.read(100));
        s.spawn(|| disk.written(40));
        s.spawn(|| {
            cpu.measure(|| {
                let until = Instant::now() + Duration::from_millis(30);
                while Instant::now() < until {
                    std::hint::black_box(0);
                }
            })
        });
    });
    disk.read(1);
    let mut part = ResourceUse::new("Index", "indexing");
    disk.report(&mut part);
    cpu.report(&mut part);
    assert_eq!((part.read, part.written), (Some(101), Some(40)));
    assert!(part.cpu.unwrap() >= Duration::from_millis(10), "{:?}", part.cpu);
}

/// Operations: started, advanced, ended; each project keeps its last ended ones, its part
/// sees only its own, and a project closed forgets them.
#[test]
fn operations_are_followed_per_project() {
    let activities = Activities::new(Instant::now);
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let up =
        activities.start(Some(a), ActivityKind::Upload, "x.php", ActivityUnit::Bytes, Some(10));
    activities.advance(up, 4);
    let copy = activities.start(Some(b), ActivityKind::Copy, "src", ActivityUnit::Files, None);
    let mut part = ResourceUse::new("Remote", "remote");
    part.project = Some(a);
    activities.report(&mut part);
    assert_eq!(part.activities.len(), 1, "only its project's");
    assert_eq!((part.activities[0].done, part.activities[0].running()), (4, true));
    activities.end(up, Outcome::Done);
    for i in 0..Activities::KEPT + 5 {
        let id =
            activities.start(Some(a), ActivityKind::Save, i.to_string(), ActivityUnit::Files, None);
        activities.end(id, Outcome::Done);
    }
    activities.report(&mut part);
    assert_eq!(part.activities.len(), Activities::KEPT, "the last ended ones only");
    assert!(part.activities.iter().all(|x| x.id != up), "the oldest went first");
    activities.forget(a);
    activities.report(&mut part);
    assert!(part.activities.is_empty());
    // The other project's is still running.
    let mut other = ResourceUse::new("Files", "app");
    other.project = Some(b);
    activities.report(&mut other);
    assert_eq!(other.activities.iter().map(|x| x.id).collect::<Vec<_>>(), [copy]);
}

/// Two actions with the same id, kind and target are the same (the data stays `Eq`); a
/// reporter or a source doing nothing says so; a part is named by owner, name and project.
#[test]
fn actions_are_plain_data_and_unsupported_by_default() {
    let stop = PartAction::new("stop", ActionKind::Stop, ActionTarget::Process(7)).confirmed();
    assert_eq!(
        stop,
        PartAction {
            id: "stop",
            kind: ActionKind::Stop,
            target: ActionTarget::Process(7),
            confirm: true
        }
    );
    assert_ne!(stop, PartAction::new("stop", ActionKind::Stop, ActionTarget::Process(8)));
    #[derive(Debug)]
    struct Quiet;
    impl ResourceReporter for Quiet {
        fn report(&self) -> Vec<ResourceUse> {
            Vec::new()
        }
    }
    struct Empty;
    impl ResourceSource for Empty {
        fn snapshot(&self) -> ResourceSnapshot {
            ResourceSnapshot::default()
        }
    }
    let part = PartRef { owner: "m".into(), name: "x".into(), project: None };
    assert_eq!(Quiet.act(&part, "stop", ActionTarget::Part), ActOutcome::Unsupported);
    assert_eq!(Empty.act(&part, "stop", ActionTarget::Part), ActOutcome::Unsupported);
    assert!(part.is(&ResourceUse::new("x", "m")));
    assert!(!part.is(&ResourceUse::new("x", "other")));
}

/// A stoppable operation: reported with its "Stop"; stopped, its flag is set (it ends
/// itself, Stopped); ended, it can't be stopped any more and offers nothing.
#[test]
fn a_stoppable_operation_is_stopped_through_its_flag() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let activities = Activities::new(Instant::now);
    let id = activities.start(None, ActivityKind::Upload, "a.php", ActivityUnit::Bytes, None);
    let plain = activities.start(None, ActivityKind::Copy, "b", ActivityUnit::Files, None);
    let flag = Arc::new(AtomicBool::new(false));
    activities.stoppable(id, Arc::clone(&flag));
    let mut part = ResourceUse::new("Remote", "remote");
    activities.report(&mut part);
    assert!(part.activities.iter().find(|a| a.id == id).unwrap().stoppable);
    assert!(!part.activities.iter().find(|a| a.id == plain).unwrap().stoppable);
    assert_eq!(
        part.actions,
        [PartAction::new(STOP_OPERATION, ActionKind::Stop, ActionTarget::Activity(id))]
    );
    assert_eq!(activities.act(ActionTarget::Activity(plain)), ActOutcome::Unsupported);
    assert_eq!(activities.act(ActionTarget::Activity(id)), ActOutcome::Done);
    assert!(flag.load(Ordering::Relaxed), "told to stop");
    // It sees its flag and ends, stopped.
    activities.end(id, Outcome::Stopped);
    assert!(!activities.stop(id), "ended: nothing to stop");
    let mut part = ResourceUse::new("Remote", "remote");
    activities.report(&mut part);
    assert!(part.actions.is_empty());
    let ended = part.activities.iter().find(|a| a.id == id).unwrap();
    assert_eq!(ended.ended.as_ref().map(|(_, o)| o), Some(&Outcome::Stopped));
}
