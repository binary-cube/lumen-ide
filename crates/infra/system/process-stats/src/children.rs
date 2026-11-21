//! The programs the app started (a terminal's shell, `git`, a formatter): each one with
//! everything it started in turn (a shell's `npm run build`), measured together — the CPU
//! time used so far, the memory held now, what was read and written. Read through
//! `sysinfo`, on the Task Manager's thread only (a look at every process costs a few ms).

use std::collections::HashMap;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// A started program and its descendants, as last seen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeUse {
    /// User + system CPU time used so far (by those still running).
    pub cpu: Duration,
    /// Memory held now (bytes).
    pub memory: u64,
    /// Read from and written to the disk so far (bytes).
    pub read: u64,
    pub written: u64,
    /// The programs in it (the started one and its descendants).
    pub processes: u32,
    /// How long the started program has been running.
    pub running: Duration,
}

/// Looks at started programs (kept between looks: `sysinfo` reuses what it read).
#[derive(Default)]
pub struct ProcessTrees {
    system: Option<System>,
    /// The programs' names, by process id and start time (an id is reused), kept while they
    /// run ([`Self::name`]).
    names: HashMap<u32, (u64, String)>,
    /// The names they were started by, the same way ([`Self::invoked`]).
    invoked: HashMap<u32, (u64, Option<String>)>,
}

impl std::fmt::Debug for ProcessTrees {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProcessTrees")
    }
}

impl ProcessTrees {
    /// What each of `roots` (process ids) and its descendants use now; one missing (it
    /// ended) isn't listed.
    pub fn sample(&mut self, roots: &[u32]) -> HashMap<u32, TreeUse> {
        if roots.is_empty() {
            return HashMap::new();
        }
        let system = self.system.get_or_insert_with(System::new);
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage();
        // Only the trees' own processes when the system names a process's children (Linux):
        // every process of the system read each second costs a desktop's hundreds of them.
        let walked = if cfg!(target_os = "linux") {
            trees_of(roots, &linux_children, &|pid| {
                std::path::Path::new(&format!("/proc/{pid}")).exists()
            })
        } else {
            None
        };
        match walked {
            Some(pids) => {
                let pids: Vec<Pid> = pids.into_iter().map(Pid::from_u32).collect();
                // One held that's no longer in a tree (it ended, or left it): the table made
                // anew with the trees' own (what's read is totals, nothing lost).
                let kept: std::collections::HashSet<Pid> = pids.iter().copied().collect();
                if system.processes().keys().any(|p| !kept.contains(p)) {
                    *system = System::new();
                }
                system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, kind);
            }
            None => {
                system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
            }
        }
        let processes = system.processes();
        // The names of programs that ended are forgotten (an id is reused later).
        self.names.retain(|pid, _| processes.contains_key(&Pid::from_u32(*pid)));
        self.invoked.retain(|pid, _| processes.contains_key(&Pid::from_u32(*pid)));
        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for (pid, process) in processes {
            if let Some(parent) = process.parent() {
                children.entry(parent).or_default().push(*pid);
            }
        }
        let mut out = HashMap::new();
        for &root in roots {
            let root_pid = Pid::from_u32(root);
            if !processes.contains_key(&root_pid) {
                continue;
            }
            let mut total = TreeUse {
                running: processes
                    .get(&root_pid)
                    .map_or(Duration::ZERO, |p| Duration::from_secs(p.run_time())),
                ..TreeUse::default()
            };
            let mut stack = vec![root_pid];
            // A process seen twice (a cycle in a stale table) counts once.
            let mut seen = std::collections::HashSet::new();
            while let Some(pid) = stack.pop() {
                if !seen.insert(pid) {
                    continue;
                }
                let Some(process) = processes.get(&pid) else { continue };
                let disk = process.disk_usage();
                total.cpu += Duration::from_millis(process.accumulated_cpu_time());
                // As the system's monitor counts it (not RSS: the shared libraries).
                total.memory +=
                    crate::footprint_of(pid.as_u32()).unwrap_or_else(|| process.memory());
                total.read += disk.total_read_bytes;
                total.written += disk.total_written_bytes;
                total.processes = total.processes.saturating_add(1);
                stack.extend(children.get(&pid).into_iter().flatten().copied());
            }
            out.insert(root, total);
        }
        out
    }

    /// The name program `pid` was started by (its command's first word, without its folder:
    /// `claude` for a program whose file is named after its version), read once while it
    /// runs — only that word: its arguments (they may hold secrets) are neither read into
    /// anything kept nor shown. Tidied to one short line. `None`: not known (not ours to
    /// read, ended).
    pub fn invoked(&mut self, pid: u32) -> Option<String> {
        let (start, settled) = self.started(pid);
        if let Some((_, known)) = self.invoked.get(&pid).filter(|(at, _)| *at == start) {
            return known.clone();
        }
        let name = crate::first_word::first_word(pid).as_deref().and_then(crate::invoked_name);
        if settled {
            self.invoked.insert(pid, (start, name.clone()));
        }
        name
    }

    /// Program `pid`'s name ([`crate::process_name`]), read once while it runs.
    pub fn name(&mut self, pid: u32) -> Option<String> {
        let (start, settled) = self.started(pid);
        if let Some((_, name)) = self.names.get(&pid).filter(|(at, _)| *at == start) {
            return Some(name.clone());
        }
        let name = crate::process_name(pid)?;
        if settled {
            self.names.insert(pid, (start, name.clone()));
        }
        Some(name)
    }

    /// When `pid` started (as last sampled: `0` unknown), and whether it has run long
    /// enough for its names to be kept — one just forked may not have become its program
    /// yet (it still bears the app's).
    fn started(&self, pid: u32) -> (u64, bool) {
        let process = self.system.as_ref().and_then(|s| s.process(Pid::from_u32(pid)));
        process.map_or((0, true), |p| (p.start_time(), p.run_time() >= 2))
    }
}

/// The most processes walked under the roots (a runaway tree is measured in part, never
/// walked forever).
const MOST_PROCESSES: usize = 4096;

/// `roots` and every process under them, found by asking each for its children
/// (`children`: `None` when they can't be read); `None` if they can't be read for one still
/// running (`alive`): then every process is looked at instead. One that ended has nothing
/// under it. Each once, a cycle never walked twice.
pub(crate) fn trees_of(
    roots: &[u32],
    children: &dyn Fn(u32) -> Option<Vec<u32>>,
    alive: &dyn Fn(u32) -> bool,
) -> Option<Vec<u32>> {
    let mut found: Vec<u32> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<u32> = roots.to_vec();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) || found.len() >= MOST_PROCESSES {
            continue;
        }
        found.push(pid);
        match children(pid) {
            Some(more) => stack.extend(more),
            // Ended meanwhile: nothing under it. Unknown for a root still running: can't say.
            None if alive(pid) => return None,
            None => {}
        }
    }
    Some(found)
}

/// Process `pid`'s children as Linux lists them (`/proc/<pid>/task/<tid>/children`, every
/// thread's); `None` when they can't be read (it ended, or the kernel doesn't keep them).
fn linux_children(pid: u32) -> Option<Vec<u32>> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    let mut out = Vec::new();
    for task in tasks.flatten() {
        let text = std::fs::read_to_string(task.path().join("children")).ok()?;
        out.extend(text.split_whitespace().filter_map(|n| n.parse::<u32>().ok()));
    }
    Some(out)
}

// Its tests start a program under another name (Unix's `arg0`).
#[cfg(all(test, unix))]
#[path = "../tests/unit/children.rs"]
mod tests;
