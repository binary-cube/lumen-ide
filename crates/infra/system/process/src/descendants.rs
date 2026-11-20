//! What a process started and what those started in turn (a terminal's shell, an agent, a
//! helper an agent detached into its own process group), found by walking the system's
//! table of parents down from it. Taken while the tree still hangs together: a program
//! whose parent has ended is adopted by the system and can't be told apart any more.
//!
//! Stopping them is careful: each is named by its id *and* its start time (an id is reused
//! once its program ends), asked to end first (`SIGTERM`), then, after a short grace, made
//! to (`SIGKILL`) only if it is still that same program. Nothing outside the tree is ever
//! touched: a program counts as a child only if it started after its parent did (an id
//! reused by a younger process never adopts an older one).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

/// A program found under a root: its id, when it started (seconds since the epoch), its
/// name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Found {
    pub pid: u32,
    pub start: u64,
    pub name: String,
}

/// How a program being stopped ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ended {
    /// When asked.
    Asked,
    /// Made to, not having ended in time.
    Forced,
}

/// The descendants of a process at one moment (the root itself not among them).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Descendants(Vec<Found>);

impl Descendants {
    /// Everything `root` started, and what those started, now (empty if `root` isn't
    /// running).
    pub fn of(root: u32) -> Self {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let rows = system
            .processes()
            .iter()
            .filter(|(_, p)| !gone(p.status()))
            .map(|(pid, p)| Row {
                pid: pid.as_u32(),
                parent: p.parent().map(Pid::as_u32),
                start: p.start_time(),
                name: p.name().to_string_lossy().into_owned(),
            })
            .collect::<Vec<_>>();
        Self(tree(&rows, root))
    }

    /// Whether nothing was found.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many were found.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The programs found.
    pub fn found(&self) -> &[Found] {
        &self.0
    }

    /// Asks each one still running (the same program: id and start time) to end; those
    /// still running after `grace` are made to. How many had to be made to.
    pub fn stop(&self, grace: Duration) -> usize {
        let mut forced = 0;
        self.stop_each(grace, |_, ended| forced += usize::from(ended == Ended::Forced));
        forced
    }

    /// [`Self::stop`], telling `ended` of each one as it goes (its id and how), in the order
    /// they end; one already gone before it was asked counts as ended when asked.
    pub fn stop_each(&self, grace: Duration, mut ended: impl FnMut(u32, Ended)) {
        let mut system = System::new();
        let mut left = still_running(&mut system, &self.0);
        for f in &self.0 {
            if !left.iter().any(|l| l.pid == f.pid) {
                ended(f.pid, Ended::Asked);
            }
        }
        for f in &left {
            signal(f.pid, Signal::Term);
        }
        let since = Instant::now();
        while !left.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
            let now = still_running(&mut system, &left);
            for f in left.iter().filter(|f| !now.iter().any(|n| n.pid == f.pid)) {
                ended(f.pid, Ended::Asked);
            }
            left = now;
            if !left.is_empty() && since.elapsed() >= grace {
                for f in &left {
                    signal(f.pid, Signal::Kill);
                    ended(f.pid, Ended::Forced);
                }
                return;
            }
        }
    }
}

/// A process as the system's table has it.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub pid: u32,
    pub parent: Option<u32>,
    pub start: u64,
    pub name: String,
}

/// The descendants of `root` in `rows`: each once, a child only if it started no earlier
/// than its parent (an id reused later never adopts an older program); `root` not among
/// them, nor a cycle a stale table could hold.
pub(crate) fn tree(rows: &[Row], root: u32) -> Vec<Found> {
    let Some(root_start) = rows.iter().find(|r| r.pid == root).map(|r| r.start) else {
        return Vec::new();
    };
    let mut children: HashMap<u32, Vec<&Row>> = HashMap::new();
    for row in rows {
        if let Some(parent) = row.parent {
            children.entry(parent).or_default().push(row);
        }
    }
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::from([root]);
    let mut stack = vec![(root, root_start)];
    while let Some((pid, start)) = stack.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if child.start < start || !seen.insert(child.pid) {
                continue;
            }
            found.push(Found { pid: child.pid, start: child.start, name: child.name.clone() });
            stack.push((child.pid, child.start));
        }
    }
    found
}

/// Ended, only not yet reaped by its parent: nothing to stop.
fn gone(status: ProcessStatus) -> bool {
    matches!(status, ProcessStatus::Zombie | ProcessStatus::Dead)
}

/// Those of `found` still running as the same program (its id and start time).
fn still_running(system: &mut System, found: &[Found]) -> Vec<Found> {
    let pids = found.iter().map(|f| Pid::from_u32(f.pid)).collect::<Vec<_>>();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing(),
    );
    found
        .iter()
        .filter(|f| {
            system
                .process(Pid::from_u32(f.pid))
                .is_some_and(|p| p.start_time() == f.start && !gone(p.status()))
        })
        .cloned()
        .collect()
}

enum Signal {
    Term,
    Kill,
}

/// Sends `signal` to program `pid` alone (never its group: only what was found is
/// touched).
fn signal(pid: u32, signal: Signal) {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else { return };
        if pid <= 0 {
            return;
        }
        let signal = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: `kill` only sends a signal; a positive pid names one process.
        #[allow(unsafe_code)]
        unsafe {
            libc::kill(pid, signal);
        }
    }
    #[cfg(not(unix))]
    {
        // Elsewhere only the end is made (no asking first).
        if matches!(signal, Signal::Kill) {
            let mut system = System::new();
            let pid = Pid::from_u32(pid);
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing(),
            );
            if let Some(p) = system.process(pid) {
                p.kill();
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/descendants.rs"]
mod tests;
