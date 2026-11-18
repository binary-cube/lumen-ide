//! What the Task Manager's collector works out at each look (its thread is the hub's,
//! [`super::hub`]): once a second it reads the picture — this window's project's, or every open
//! project's — and the threads' CPU time, works out each part's CPU (the time its own
//! work took, when it measures it; else its threads' since the last look), its network and
//! disk rates (its bytes since the last look), adds the programs it started (with their
//! descendants: CPU, memory, disk — and each program's own row), and makes the frame the
//! views draw. Rows are known by their owner, name and project (its identity): two projects
//! with the same name never mix.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use workbench_model::{ResourceSnapshot, ResourceUse};

use super::frame::{Frame, Line, ProcessRow, key};

/// How often it looks, unless the settings say otherwise.
pub const EVERY: Duration = Duration::from_secs(1);

/// The threads' CPU time at the last look, by thread.
#[derive(Debug, Default)]
pub struct Sampler {
    last: HashMap<u64, Duration>,
    at: Option<Instant>,
}

impl Sampler {
    /// Each thread's CPU (percent of a core) since the last look; the first look counts
    /// nothing (nothing to compare with).
    pub fn sample(
        &mut self,
        threads: &[process_stats::ThreadTime],
        now: Instant,
    ) -> Vec<(String, f32)> {
        let elapsed = self.at.map(|at| now.duration_since(at).as_secs_f32()).filter(|s| *s > 0.0);
        let out = threads
            .iter()
            .map(|t| {
                let before = self.last.get(&t.id).copied().unwrap_or(t.cpu);
                let used = t.cpu.saturating_sub(before);
                (t.name.clone(), elapsed.map_or(0.0, |s| used.as_secs_f32() / s * 100.0))
            })
            .collect();
        self.last = threads.iter().map(|t| (t.id, t.cpu)).collect();
        self.at = Some(now);
        out
    }
}

/// A part's rates since the last look: network and disk (bytes a second), its measured CPU
/// and its programs' (percent of a core), each when it counts it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PartRates {
    pub net: Option<f32>,
    pub disk: Option<f32>,
    pub cpu: Option<f32>,
    pub programs: Option<f32>,
}

/// The programs the parts started, measured with their descendants: added to each part's
/// memory and disk; their CPU time (by the part's [`key`]) returned, for its rate — and each
/// program's own use (with its descendants), by its process id, for its row.
pub fn add_programs(
    trees: &mut process_stats::ProcessTrees,
    parts: &mut [ResourceUse],
) -> (HashMap<String, Duration>, HashMap<u32, process_stats::TreeUse>) {
    let roots: Vec<u32> = parts.iter().flat_map(|p| p.processes.iter().copied()).collect();
    let used = trees.sample(&roots);
    let mut cpu = HashMap::new();
    for part in parts.iter_mut().filter(|p| !p.processes.is_empty()) {
        let mut total = process_stats::TreeUse::default();
        for tree in part.processes.iter().filter_map(|pid| used.get(pid)) {
            total.cpu += tree.cpu;
            total.memory += tree.memory;
            total.read += tree.read;
            total.written += tree.written;
        }
        part.memory = Some(part.memory.unwrap_or(0) + total.memory);
        part.read = Some(part.read.unwrap_or(0) + total.read);
        part.written = Some(part.written.unwrap_or(0) + total.written);
        cpu.insert(key(&part.owner, &part.name, part.project), total.cpu);
    }
    (cpu, used)
}

/// The programs' counters at the last look (CPU time, bytes moved), by process id: their
/// rows' rates since.
#[derive(Debug, Default)]
pub struct ProcessRates {
    last: HashMap<u32, (Duration, u64)>,
    at: Option<Instant>,
}

impl ProcessRates {
    /// Each program's row now: its CPU and disk since the last look (nothing the first
    /// time it's seen), its memory, its names (`names`: the system's, and the one it was
    /// started by — both kept while it runs).
    pub fn sample(
        &mut self,
        used: &HashMap<u32, process_stats::TreeUse>,
        mut names: impl FnMut(u32) -> (Option<String>, Option<String>),
        now: Instant,
    ) -> HashMap<u32, ProcessRow> {
        let elapsed = self.at.map(|at| now.duration_since(at).as_secs_f32()).filter(|s| *s > 0.0);
        let mut rows = HashMap::with_capacity(used.len());
        let mut last = HashMap::with_capacity(used.len());
        for (&pid, tree) in used {
            let moved = tree.read + tree.written;
            let (cpu_before, moved_before) =
                self.last.get(&pid).copied().unwrap_or((tree.cpu, moved));
            let cpu = elapsed
                .map_or(0.0, |s| tree.cpu.saturating_sub(cpu_before).as_secs_f32() / s * 100.0);
            let disk = elapsed.map_or(0.0, |s| moved.saturating_sub(moved_before) as f32 / s);
            let (name, invoked) = names(pid);
            rows.insert(
                pid,
                ProcessRow {
                    pid,
                    name,
                    cpu,
                    memory: tree.memory,
                    disk,
                    invoked,
                    processes: tree.processes,
                    running: tree.running,
                    project: None,
                },
            );
            last.insert(pid, (tree.cpu, moved));
        }
        self.last = last;
        self.at = Some(now);
        rows
    }
}

/// The parts' counters at the last look (bytes moved, CPU time), and the process's disk:
/// their rates since.
#[derive(Debug, Default)]
pub struct Rates {
    last: HashMap<String, (u64, u64, Duration, Duration)>,
    disk: Option<u64>,
    at: Option<Instant>,
}

impl Rates {
    /// Each part's rates since the last look, by [`key`]; and the process's disk rate.
    pub fn sample(
        &mut self,
        parts: &[ResourceUse],
        programs: &HashMap<String, Duration>,
        disk: Option<(u64, u64)>,
        now: Instant,
    ) -> (HashMap<String, PartRates>, Option<f32>) {
        let elapsed = self.at.map(|at| now.duration_since(at).as_secs_f32()).filter(|s| *s > 0.0);
        let per_second =
            |now: u64, before: u64| elapsed.map_or(0.0, |s| now.saturating_sub(before) as f32 / s);
        let mut rates = HashMap::new();
        let mut last = HashMap::new();
        for part in parts {
            let net = (part.sent.is_some() || part.received.is_some())
                .then(|| part.sent.unwrap_or(0) + part.received.unwrap_or(0));
            let disk = (part.read.is_some() || part.written.is_some())
                .then(|| part.read.unwrap_or(0) + part.written.unwrap_or(0));
            let key = key(&part.owner, &part.name, part.project);
            let started = programs.get(&key).copied();
            if net.is_none() && disk.is_none() && part.cpu.is_none() && started.is_none() {
                continue;
            }
            let now_counts = (
                net.unwrap_or(0),
                disk.unwrap_or(0),
                part.cpu.unwrap_or_default(),
                started.unwrap_or_default(),
            );
            let before = self.last.get(&key).copied().unwrap_or(now_counts);
            let percent = |now: Duration, before: Duration| {
                let used = now.saturating_sub(before).as_secs_f32();
                elapsed.map_or(0.0, |s| used / s * 100.0)
            };
            let rate = PartRates {
                net: net.map(|n| per_second(n, before.0)),
                disk: disk.map(|d| per_second(d, before.1)),
                cpu: part.cpu.map(|cpu| percent(cpu, before.2)),
                programs: started.map(|cpu| percent(cpu, before.3)),
            };
            rates.insert(key.clone(), rate);
            last.insert(key, now_counts);
        }
        let process = disk.map(|(read, written)| read + written);
        let disk_rate = process.map(|now| per_second(now, self.disk.unwrap_or(now)));
        self.last = last;
        self.disk = process;
        self.at = Some(now);
        (rates, disk_rate)
    }
}

/// The names of the rows the frame makes itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameLabels {
    /// The threads no part claims.
    pub other_threads: String,
    /// The memory nobody reported (with the heap counted: the data nobody reported).
    pub unattributed: String,
    /// What isn't the app's data: the code, the GPU's memory, the system's libraries.
    pub outside_heap: String,
}

impl FrameLabels {
    /// In the user's language.
    pub fn translated() -> Self {
        Self {
            other_threads: i18n::t("tasks.other_threads"),
            unattributed: i18n::t("tasks.unattributed"),
            outside_heap: i18n::t("tasks.outside_heap"),
        }
    }
}

/// The name of the project program `pid` of `part` works for, when the part says it (it's
/// every project's): any open project's, this window's or another's.
fn project_of(snapshot: &ResourceSnapshot, part: &ResourceUse, pid: u32) -> Option<String> {
    let (_, id) = part.process_projects.iter().find(|(p, _)| *p == pid)?;
    let open = snapshot.projects.iter().chain(&snapshot.elsewhere);
    open.into_iter().find(|p| p.id == *id).map(|p| p.name.clone())
}

/// The frame for `snapshot` with these threads' CPU, these rates and these programs' rows:
/// each thread counts for the first part it works for; the others, and the memory nobody
/// reported, have rows of their own.
pub fn frame(
    snapshot: ResourceSnapshot,
    threads: &[(String, f32)],
    rates: &HashMap<String, PartRates>,
    programs: &HashMap<u32, ProcessRow>,
    labels: &FrameLabels,
) -> Frame {
    use workbench_model::resources::{APP, MODULES};
    let mut lines: Vec<Line> = snapshot
        .parts
        .iter()
        .map(|p: &ResourceUse| {
            let mut line = Line::new(p.name.clone(), p.owner.clone(), p.group, p.project);
            let rate = rates.get(&line.key).copied().unwrap_or_default();
            line.cpu = rate.cpu.unwrap_or(0.0) + rate.programs.unwrap_or(0.0);
            line.memory = p.memory;
            line.gpu = p.gpu;
            line.net = rate.net;
            line.disk = rate.disk;
            line.detail = p.detail.clone();
            line.activities = p.activities.clone();
            line.actions = p.actions.clone();
            line.facts = p.facts.clone();
            line.processes = p
                .processes
                .iter()
                .filter_map(|pid| {
                    let mut row = programs.get(pid).cloned()?;
                    row.project = project_of(&snapshot, p, *pid);
                    Some(row)
                })
                .collect();
            line
        })
        .collect();
    let mut others = Line::new(labels.other_threads.clone(), APP, APP, None);
    let mut names = Vec::new();
    for (name, cpu) in threads {
        // A part of a project (a module's reporter, the app's) before the module's own row:
        // its threads' CPU goes to what it works for.
        let part = |modules: bool| {
            snapshot.parts.iter().position(|p| (p.group == MODULES) == modules && p.runs_on(name))
        };
        match part(false).or_else(|| part(true)) {
            // Its part measures its own CPU (its threads are shared by projects: every
            // project's indexer has the same name): the thread is claimed, not added.
            Some(i) if snapshot.parts[i].cpu.is_some() => {
                lines[i].threads += 1;
                lines[i].thread_cpu.push((name.clone(), *cpu));
            }
            Some(i) => {
                lines[i].cpu += cpu;
                lines[i].threads += 1;
                lines[i].thread_cpu.push((name.clone(), *cpu));
            }
            None => {
                others.cpu += cpu;
                others.threads += 1;
                others.thread_cpu.push((name.clone(), *cpu));
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
    }
    if others.threads > 0 {
        others.detail = names.join(" · ");
        lines.push(others);
    }
    // What the process holds that no part reported: with the heap counted, split into what
    // isn't the app's data (the code, the GPU's memory, the system's) and the data nobody
    // reported (last: the memory card's "not attributed").
    let known: u64 = lines.iter().filter_map(|l| l.memory).sum();
    let mut unattributed = Line::new(labels.unattributed.clone(), APP, APP, None);
    match snapshot.heap {
        Some(heap) => {
            let mut outside = Line::new(labels.outside_heap.clone(), APP, APP, None);
            outside.memory = Some(snapshot.memory.saturating_sub(heap.max(known)));
            outside.gpu = snapshot.gpu;
            lines.push(outside);
            unattributed.memory = Some(heap.saturating_sub(known));
        }
        None => unattributed.memory = Some(snapshot.memory.saturating_sub(known)),
    }
    lines.push(unattributed);
    Frame {
        cpu: threads.iter().map(|(_, c)| c).sum(),
        memory: snapshot.memory,
        gpu: snapshot.gpu,
        net: rates.values().filter_map(|r| r.net).sum(),
        disk: None,
        threads: threads.len(),
        projects: snapshot.projects,
        all: false,
        lines,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/collector/sample.rs"]
mod tests;
