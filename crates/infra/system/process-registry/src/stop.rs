//! Stopping programs left running: asked to end (SIGTERM; on Windows, which has no such
//! request, ended at once), then, after a grace, made to (SIGKILL). Each is known by its
//! pid and start time: a pid now another program's is never touched. Blocks: on a worker.

use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System};

/// How long a program asked to end has before it's made to.
pub const GRACE: Duration = Duration::from_secs(3);

/// `programs` (pid, start time) stopped: asked, then after `grace` made to. How many were
/// still running when asked.
pub fn stop(programs: &[(u32, u64)], grace: Duration) -> usize {
    let mut system = System::new();
    let pids: Vec<Pid> = programs
        .iter()
        .filter(|(pid, _)| *pid > 1 && *pid != std::process::id())
        .map(|(pid, _)| Pid::from_u32(*pid))
        .collect();
    let refresh = |system: &mut System| {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing(),
        );
    };
    let running = |system: &System| -> Vec<Pid> {
        programs
            .iter()
            .filter(|(pid, started)| {
                pids.contains(&Pid::from_u32(*pid))
                    && system
                        .process(Pid::from_u32(*pid))
                        .is_some_and(|p| p.start_time() == *started && !zombie(p))
            })
            .map(|(pid, _)| Pid::from_u32(*pid))
            .collect()
    };
    refresh(&mut system);
    let asked = running(&system);
    for pid in &asked {
        if let Some(process) = system.process(*pid) {
            // Windows has no request to end: ended at once.
            if process.kill_with(Signal::Term).is_none() {
                process.kill();
            }
        }
    }
    let since = Instant::now();
    loop {
        refresh(&mut system);
        let left = running(&system);
        if left.is_empty() {
            break;
        }
        if since.elapsed() >= grace {
            for pid in left {
                if let Some(process) = system.process(pid) {
                    tracing::info!(pid = pid.as_u32(), "a program left running is made to end");
                    process.kill();
                }
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    asked.len()
}

/// Ended, waiting for its parent to read how (nothing left to stop).
fn zombie(process: &sysinfo::Process) -> bool {
    matches!(process.status(), sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead)
}
