//! What an earlier run left running, looked at once at start (on a worker): the files of
//! instances no longer running (their app's pid gone, or now another program's) are taken
//! (renamed, so two starts never handle one twice), each program in them still running with
//! the same start time is found with what it started; helpers (servers, git, the rest) are
//! stopped, the user's work (terminals, agents, tasks) is measured (its CPU now) and handed
//! back to be asked about. The files taken are removed by [`Survey::done`], once what's
//! asked about is kept elsewhere.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::record::{InstanceFile, Record};

/// A file taken by a start that then ended before handling it is taken again after this.
const TAKEN_STALE: Duration = Duration::from_secs(60);

/// The suffix of a file taken.
const TAKEN: &str = "taken";

/// The CPU above which a program is working (percent of one core).
pub const WORKING_CPU: f32 = 2.0;

/// A program left running, found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub record: Record,
    /// It and what it started (pid, start time), it first.
    pub tree: Vec<(u32, u64)>,
    /// Its CPU now, with what it started (percent of one core).
    pub cpu: f32,
}

impl Found {
    /// Whether it's working now (else it waits).
    pub fn working(&self) -> bool {
        self.cpu >= WORKING_CPU
    }
}

/// What the look found.
#[derive(Debug, Default)]
pub struct Survey {
    /// The helpers stopped.
    pub stopped: usize,
    /// The user's work still running, to ask about.
    pub asked: Vec<Found>,
    /// The files taken (removed by [`Self::done`]).
    taken: Vec<PathBuf>,
}

impl Survey {
    /// The files taken removed (what's asked about is kept elsewhere by now).
    pub fn done(&mut self) {
        for file in self.taken.drain(..) {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// A process as seen now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub pid: u32,
    pub parent: Option<u32>,
    pub started: u64,
}

/// Whether `record` still runs: its pid there with the same start time.
pub fn still_runs(record: &Record, seen: &HashMap<u32, Seen>) -> bool {
    seen.get(&record.pid).is_some_and(|s| s.started == record.started)
}

/// `root` and what it started (and they started…), as (pid, start time), `root` first. A
/// child is one whose parent is in the tree and that started no earlier than its parent.
pub fn tree_of(root: &Seen, seen: &HashMap<u32, Seen>) -> Vec<(u32, u64)> {
    let mut children: HashMap<u32, Vec<&Seen>> = HashMap::new();
    for s in seen.values() {
        if let Some(parent) = s.parent
            && parent != s.pid
        {
            children.entry(parent).or_default().push(s);
        }
    }
    let mut tree = vec![(root.pid, root.started)];
    let mut i = 0;
    // Bounded: a pid is visited once.
    while i < tree.len() && tree.len() < 4096 {
        let (pid, started) = tree[i];
        if let Some(kids) = children.get(&pid) {
            for kid in kids {
                if kid.started >= started
                    && kid.pid != std::process::id()
                    && !tree.iter().any(|(p, _)| *p == kid.pid)
                {
                    tree.push((kid.pid, kid.started));
                }
            }
        }
        i += 1;
    }
    tree
}

/// A record still running, with its tree ([`tree_of`]).
pub type Rooted = (Record, Vec<(u32, u64)>);

/// The records of dead instances split: still running and a helper (to stop), still
/// running and the user's (to ask about); the rest ended.
pub fn decide(records: Vec<Record>, seen: &HashMap<u32, Seen>) -> (Vec<Rooted>, Vec<Rooted>) {
    let mut stop = Vec::new();
    let mut ask = Vec::new();
    let own = std::process::id();
    for record in records {
        // A file is hostile: never the system's first process, nor this app.
        if record.pid <= 1 || record.pid == own || !still_runs(&record, seen) {
            continue;
        }
        let Some(root) = seen.get(&record.pid) else { continue };
        let tree = tree_of(root, seen);
        if record.kind().is_users() {
            ask.push((record, tree));
        } else {
            stop.push((record, tree));
        }
    }
    (stop, ask)
}

/// Every process now.
fn look(system: &mut System) -> HashMap<u32, Seen> {
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system
        .processes()
        .iter()
        .map(|(pid, p)| {
            let pid = pid.as_u32();
            (pid, Seen { pid, parent: p.parent().map(Pid::as_u32), started: p.start_time() })
        })
        .collect()
}

/// The instances' files in `dir` other than `own`: the dead ones taken (renamed) and read.
fn take_dead(dir: &Path, own: &Path, seen: &HashMap<u32, Seen>) -> Vec<(PathBuf, InstanceFile)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut taken = Vec::new();
    // Bounded: a folder of ours holds a few.
    for entry in entries.flatten().take(1024) {
        let path = entry.path();
        if path == own || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
        let path = match ext {
            "toml" => {
                let Some(file) = InstanceFile::read(&path) else { continue };
                let alive = seen
                    .get(&file.app_pid)
                    .is_some_and(|s| s.started == file.app_started || file.app_started == 0);
                if alive {
                    continue;
                }
                let to = path.with_extension(TAKEN);
                // Another start took it first.
                if std::fs::rename(&path, &to).is_err() {
                    continue;
                }
                to
            }
            // Taken by a start that ended before handling it.
            TAKEN => {
                let old = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age >= TAKEN_STALE);
                if !old {
                    continue;
                }
                path
            }
            _ => continue,
        };
        match InstanceFile::read(&path) {
            Some(file) => taken.push((path, file)),
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    taken
}

/// What runs in `dir`'s dead instances' files (other than `own`, this instance's): helpers
/// stopped, the user's work measured and handed back. Blocks (a look at every process, a
/// moment for the CPU, the helpers' grace): call it on a worker.
pub fn survey(dir: &Path, own: &Path) -> Survey {
    let mut system = System::new();
    let seen = look(&mut system);
    let files = take_dead(dir, own, &seen);
    if files.is_empty() {
        return Survey::default();
    }
    let mut survey = Survey::default();
    let mut records = Vec::new();
    for (path, file) in files {
        records.extend(file.programs);
        survey.taken.push(path);
    }
    let (stop, ask) = decide(records, &seen);
    for (record, _) in &stop {
        tracing::info!(pid = record.pid, kind = %record.kind, "a helper left running is stopped");
    }
    let trees: Vec<(u32, u64)> = stop.iter().flat_map(|(_, tree)| tree.iter().copied()).collect();
    survey.stopped = stop.len();
    if !trees.is_empty() {
        crate::stop::stop(&trees, crate::stop::GRACE);
    }
    if ask.is_empty() {
        return survey;
    }
    let cpus = cpu_of(&mut system, &ask);
    survey.asked = ask
        .into_iter()
        .zip(cpus)
        .map(|((record, tree), cpu)| Found { record, tree, cpu })
        .collect();
    survey
}

/// Which of `found` still run (each its pid there with the same start time): a look at
/// only their pids. Blocks for a moment: call it on a worker.
pub fn still_running(system: &mut System, found: &[Found]) -> Vec<bool> {
    let mut pids: Vec<Pid> = found.iter().map(|f| Pid::from_u32(f.record.pid)).collect();
    pids.sort_unstable();
    pids.dedup();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing(),
    );
    found
        .iter()
        .map(|f| {
            system
                .process(Pid::from_u32(f.record.pid))
                .is_some_and(|p| p.start_time() == f.record.started)
        })
        .collect()
}

/// Each tree's CPU now (two looks a moment apart).
fn cpu_of(system: &mut System, trees: &[Rooted]) -> Vec<f32> {
    let pids: Vec<Pid> =
        trees.iter().flat_map(|(_, t)| t.iter().map(|(p, _)| Pid::from_u32(*p))).collect();
    let refresh = |system: &mut System| {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing().with_cpu(),
        );
    };
    refresh(system);
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(250)));
    refresh(system);
    trees
        .iter()
        .map(|(_, tree)| {
            tree.iter()
                .filter_map(|(pid, started)| {
                    system
                        .process(Pid::from_u32(*pid))
                        .filter(|p| p.start_time() == *started)
                        .map(sysinfo::Process::cpu_usage)
                })
                .sum()
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/survey.rs"]
mod tests;
