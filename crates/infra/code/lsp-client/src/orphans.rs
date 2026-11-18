//! Servers left behind: an app that ended without stopping its servers (it crashed, it was
//! killed) leaves them running with no one to talk to — a server is told the app's process id
//! (`initialize`'s `processId`) but not every one watches it. At the next start, the programs
//! running from the servers' folder whose parent is gone are stopped. Another running copy
//! of the app keeps its own (their parent is alive); nothing outside the folder is touched.

use std::path::Path;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Whether a process running `exe` with its parent `alive` is a server left behind in
/// `dir` (the servers' folder).
pub fn is_left_behind(exe: Option<&Path>, parent_alive: bool, dir: &Path) -> bool {
    !parent_alive && exe.is_some_and(|exe| exe.starts_with(dir))
}

/// The servers left behind in `dir` stopped (on a worker: a look at every process); how
/// many.
pub fn stop_left_behind(dir: &Path) -> usize {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    let own = sysinfo::get_current_pid().ok();
    let mut stopped = 0;
    for (pid, process) in system.processes() {
        if Some(*pid) == own {
            continue;
        }
        // Unix: adopted by the system's first process; anywhere: a parent no longer there.
        let parent_alive =
            process.parent().is_some_and(|p| p != Pid::from_u32(1) && system.process(p).is_some());
        if is_left_behind(process.exe(), parent_alive, &dir) {
            tracing::info!(pid = pid.as_u32(), "a language server left behind is stopped");
            if process.kill() {
                stopped += 1;
            }
        }
    }
    stopped
}

#[cfg(test)]
#[path = "../tests/unit/orphans.rs"]
mod tests;
