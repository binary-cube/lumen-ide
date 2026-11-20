//! What a recording says, in the order it's looked into: how busy the app was, which
//! threads (and the modules they work for) took the CPU, who asked for frames and why (an
//! app that should be idle and isn't: something keeps asking), what took the interface's
//! time, the frames, the peaks of memory and GPU.

use std::collections::HashMap;

use crate::format::Recording;

/// The share of one core (percent: 100 is one core busy the whole time).
fn percent(us: u64, ms: u64) -> f64 {
    if ms == 0 { 0.0 } else { us as f64 / (ms as f64 * 10.0) }
}

/// What a recording says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub seconds: f64,
    /// The whole process's CPU (percent of one core, on average).
    pub cpu: f64,
    /// Each thread's CPU (percent of one core) and whose it is, the busiest first.
    pub threads: Vec<(String, Option<String>, f64)>,
    /// Each module's CPU (its threads' together; `app` for the app's own and the
    /// interface's), the busiest first.
    pub modules: Vec<(String, f64)>,
    /// Who asked for frames, why, how many a second, the most first.
    pub triggers: Vec<(String, String, f64)>,
    /// What took the interface's thread: whose, what, its share of the time (percent),
    /// its calls, its longest call (milliseconds), the most first.
    pub busy: Vec<(String, String, f64, u64, f64)>,
    /// Frames a second, their average and longest time (milliseconds).
    pub fps: f64,
    pub frame_ms: f64,
    pub frame_longest_ms: f64,
    /// The highest memory and GPU memory seen (bytes).
    pub memory_peak: u64,
    pub gpu_peak: Option<u64>,
    /// What the parts counted: whose, what, its unit, how many a second, how many in all.
    pub counters: Vec<(String, String, String, f64, u64)>,
    /// What the parts held: whose, what, the highest, the last.
    pub levels: Vec<(String, String, u64, u64)>,
    /// What each part moved (bytes in all), the most first; the process's disk in all.
    pub io: Vec<crate::format::Io>,
    pub disk_read: u64,
    pub disk_written: u64,
    /// The timed work as it nested, each way down once: its steps, calls, time in all
    /// (microseconds), the costliest first (a flame graph's).
    pub spans: Vec<(Vec<String>, u64, u64)>,
    /// What stands out, the worst first.
    pub findings: Vec<Finding>,
}

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Most likely what the user feels (the app busy for nothing, frames dropped).
    High,
    /// Worth a look.
    Medium,
}

/// Something that stands out in a recording, with what it's about (the view says it in the
/// user's language).
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    /// The app spent CPU while the user did next to nothing.
    BusyWhileIdle { cpu: f64, inputs: f64 },
    /// A thread took much of a core: its name, whose it is, its share.
    BusyThread { thread: String, owner: Option<String>, cpu: f64 },
    /// A part kept asking for frames (not the user's input): whose, why, how many a second.
    FrameAsker { owner: String, cause: String, rate: f64 },
    /// A part took much of the interface's time: whose, what, its share, its longest call.
    SlowPart { owner: String, what: String, share: f64, longest_ms: f64 },
    /// A frame took long enough to be seen as a stutter.
    LongFrame { ms: f64 },
    /// A part kept the network or the disk busy: whose, bytes a second over the network
    /// and on the disk.
    BusyIo { owner: String, network: f64, disk: f64 },
}

impl Finding {
    pub fn severity(&self) -> Severity {
        match self {
            Finding::BusyWhileIdle { .. } => Severity::High,
            Finding::BusyThread { cpu, .. } if *cpu >= 50.0 => Severity::High,
            Finding::FrameAsker { rate, .. } if *rate >= 30.0 => Severity::High,
            Finding::SlowPart { share, .. } if *share >= 25.0 => Severity::High,
            Finding::LongFrame { ms } if *ms >= 250.0 => Severity::High,
            Finding::BusyIo { network, disk, .. } if network + disk >= 10.0 * MB => Severity::High,
            _ => Severity::Medium,
        }
    }
}

/// The CPU (percent of a core) past which an app the user hardly touches is busy for
/// nothing, and the inputs a second under which they hardly touch it.
const IDLE_CPU: f64 = 15.0;
const IDLE_INPUTS: f64 = 1.0;
/// A thread's share of a core worth saying.
const THREAD_CPU: f64 = 20.0;
/// Frames a second asked by a part (not input) worth saying.
const FRAME_RATE: f64 = 5.0;
/// A part's share of the interface's time worth saying.
const PART_SHARE: f64 = 8.0;
/// A frame long enough to be seen (milliseconds).
const LONG_FRAME_MS: f64 = 50.0;
/// A megabyte.
const MB: f64 = 1024.0 * 1024.0;
/// What a part moves a second (bytes, network and disk) worth saying.
const IO_RATE: f64 = MB;

/// The findings of a kind said at most.
const EACH: usize = 3;

/// What stands out in `s`, the worst first.
fn findings(s: &Summary) -> Vec<Finding> {
    let mut out = Vec::new();
    let inputs: f64 = s.triggers.iter().filter(|t| t.1 == "input").map(|t| t.2).sum();
    if s.cpu >= IDLE_CPU && inputs < IDLE_INPUTS {
        out.push(Finding::BusyWhileIdle { cpu: s.cpu, inputs });
    }
    for (thread, owner, cpu) in s.threads.iter().filter(|t| t.2 >= THREAD_CPU).take(EACH) {
        out.push(Finding::BusyThread { thread: thread.clone(), owner: owner.clone(), cpu: *cpu });
    }
    let askers =
        s.triggers.iter().filter(|t| t.1 != "input" && t.1 != "window" && t.2 >= FRAME_RATE);
    for (owner, cause, rate) in askers.take(EACH) {
        out.push(Finding::FrameAsker { owner: owner.clone(), cause: cause.clone(), rate: *rate });
    }
    for (owner, what, share, _, longest) in s.busy.iter().filter(|b| b.2 >= PART_SHARE).take(EACH) {
        out.push(Finding::SlowPart {
            owner: owner.clone(),
            what: what.clone(),
            share: *share,
            longest_ms: *longest,
        });
    }
    for io in s.io.iter().take(EACH) {
        let network = (io.sent + io.received) as f64 / s.seconds;
        let disk = (io.read + io.written) as f64 / s.seconds;
        if network + disk >= IO_RATE {
            out.push(Finding::BusyIo { owner: io.owner.clone(), network, disk });
        }
    }
    if s.frame_longest_ms >= LONG_FRAME_MS {
        out.push(Finding::LongFrame { ms: s.frame_longest_ms });
    }
    out.sort_by_key(Finding::severity);
    out
}

/// What `recording` says (empty: nothing recorded).
pub fn summary(recording: &Recording) -> Summary {
    let ms: u64 = recording.samples.iter().map(|s| s.span_ms).fold(0, u64::saturating_add);
    if ms == 0 {
        return Summary::default();
    }
    let seconds = ms as f64 / 1000.0;
    let cpu = percent(recording.samples.iter().map(|s| s.cpu_us).fold(0, u64::saturating_add), ms);
    let mut threads: HashMap<&str, u64> = HashMap::new();
    let mut triggers: HashMap<(&str, &str), u64> = HashMap::new();
    let mut busy: HashMap<(&str, &str), (u64, u64, u64)> = HashMap::new();
    let mut spans: HashMap<&[String], (u64, u64)> = HashMap::new();
    let mut io: Vec<crate::format::Io> = Vec::new();
    let (mut disk_read, mut disk_written) = (0u64, 0u64);
    let mut counters: HashMap<(&str, &str, &str), u64> = HashMap::new();
    let mut levels: HashMap<(&str, &str), (u64, u64)> = HashMap::new();
    let (mut frames, mut frame_us, mut frame_longest) = (0u64, 0u64, 0u64);
    let (mut memory_peak, mut gpu_peak) = (0u64, None::<u64>);
    for s in &recording.samples {
        for t in &s.threads {
            let e = threads.entry(&t.thread).or_default();
            *e = e.saturating_add(t.us);
        }
        for t in &s.triggers {
            let e = triggers.entry((&t.owner, &t.cause)).or_default();
            *e = e.saturating_add(t.count);
        }
        for b in &s.busy {
            let e = busy.entry((&b.owner, &b.what)).or_default();
            e.0 = e.0.saturating_add(b.us);
            e.1 = e.1.saturating_add(b.calls);
            e.2 = e.2.max(b.longest_us);
        }
        for span in &s.spans {
            let e = spans.entry(span.path.as_slice()).or_default();
            e.0 = e.0.saturating_add(span.calls);
            e.1 = e.1.saturating_add(span.us);
        }
        disk_read = disk_read.saturating_add(s.disk_read);
        disk_written = disk_written.saturating_add(s.disk_written);
        for i in &s.io {
            match io.iter_mut().find(|m| m.owner == i.owner) {
                Some(m) => m.add(i),
                None => io.push(i.clone()),
            }
        }
        for c in &s.counters {
            let e = counters.entry((&c.owner, &c.what, &c.unit)).or_default();
            *e = e.saturating_add(c.count);
        }
        for l in &s.levels {
            let e = levels.entry((&l.owner, &l.what)).or_default();
            e.0 = e.0.max(l.peak);
            e.1 = l.last;
        }
        frames = frames.saturating_add(s.frames);
        frame_us = frame_us.saturating_add(s.frame_us);
        frame_longest = frame_longest.max(s.frame_longest_us);
        memory_peak = memory_peak.max(s.memory).max(s.memory_peak);
        gpu_peak = s.gpu_peak.or(s.gpu).map(|g| gpu_peak.map_or(g, |p| p.max(g))).or(gpu_peak);
    }
    let mut thread_rows: Vec<(String, Option<String>, f64)> = threads
        .iter()
        .map(|(t, us)| {
            ((*t).to_owned(), recording.owners.of(t).map(str::to_owned), percent(*us, ms))
        })
        .collect();
    thread_rows.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    let mut modules: HashMap<String, f64> = HashMap::new();
    for (_, owner, p) in &thread_rows {
        *modules.entry(owner.clone().unwrap_or_else(|| "app".to_owned())).or_default() += p;
    }
    let mut modules: Vec<(String, f64)> = modules.into_iter().collect();
    modules.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut trigger_rows: Vec<(String, String, f64)> = triggers
        .into_iter()
        .map(|((o, c), n)| (o.to_owned(), c.to_owned(), n as f64 / seconds))
        .collect();
    trigger_rows.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    let mut busy_rows: Vec<(String, String, f64, u64, f64)> = busy
        .into_iter()
        .map(|((o, w), (us, calls, longest))| {
            (o.to_owned(), w.to_owned(), percent(us, ms), calls, longest as f64 / 1000.0)
        })
        .collect();
    busy_rows.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    let mut counter_rows: Vec<(String, String, String, f64, u64)> = counters
        .into_iter()
        .map(|((o, w, u), n)| (o.to_owned(), w.to_owned(), u.to_owned(), n as f64 / seconds, n))
        .collect();
    counter_rows.sort_by(|a, b| a.0.cmp(&b.0).then(b.4.cmp(&a.4)).then(a.1.cmp(&b.1)));
    let mut level_rows: Vec<(String, String, u64, u64)> = levels
        .into_iter()
        .map(|((o, w), (peak, last))| (o.to_owned(), w.to_owned(), peak, last))
        .collect();
    level_rows.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let moved = |i: &crate::format::Io| {
        i.sent.saturating_add(i.received).saturating_add(i.read).saturating_add(i.written)
    };
    io.sort_by(|a, b| moved(b).cmp(&moved(a)).then(a.owner.cmp(&b.owner)));
    let mut summary = Summary {
        seconds,
        cpu,
        threads: thread_rows,
        modules,
        triggers: trigger_rows,
        busy: busy_rows,
        fps: frames as f64 / seconds,
        frame_ms: if frames == 0 { 0.0 } else { frame_us as f64 / frames as f64 / 1000.0 },
        frame_longest_ms: frame_longest as f64 / 1000.0,
        memory_peak,
        gpu_peak,
        counters: counter_rows,
        levels: level_rows,
        io,
        disk_read,
        disk_written,
        spans: {
            let mut rows: Vec<(Vec<String>, u64, u64)> =
                spans.into_iter().map(|(p, (c, us))| (p.to_vec(), c, us)).collect();
            rows.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
            rows
        },
        findings: Vec::new(),
    };
    summary.findings = findings(&summary);
    summary
}

#[cfg(test)]
#[path = "../tests/unit/summary.rs"]
mod tests;
