//! A profile recording made (plan 91): [`start`] begins it on a thread of its own
//! (`profiler`), which every interval samples this process — each thread's CPU time since
//! the last sample (by the thread's name: ours, or a library's), the memory as the system
//! counts it, the GPU memory the app's drawing holds, and what the probes gathered
//! (`kernel::probe`: the frames, who asked for them and why, the parts' time on the
//! interface's thread). It ends when asked or when its time is up (it never stays on
//! forgotten), and the recording is written as one JSON file, through a temporary file
//! renamed over it. One recording at a time.
//!
//! Nothing about the user is read: no path but the folder the file goes to, no environment
//! but which display session runs (to say `wayland` or `x11`), no names but the threads'
//! (cleaned) and the owners the app gives (`.claude/rules/privacy.md`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use profiling::{Busy, Counter, Io, Level, Owners, Recording, Sample, System, ThreadCpu, Trigger};

/// The thread's name.
pub const THREAD: &str = "profiler";

/// How often it looks while waiting for the next sample (a stop is seen this soon).
const STEP: Duration = Duration::from_millis(25);

/// What a recording is started with.
pub struct Plan {
    /// Where the file goes.
    pub dir: PathBuf,
    /// How long it records at most, and how often it samples.
    pub length: Duration,
    pub interval: Duration,
    /// What only the app knows of the system (its graphics adapter and driver, the
    /// window's scale) and of itself (its version, its build).
    pub gpu: String,
    pub gpu_driver: String,
    pub scale: f32,
    pub app_version: String,
    pub build: String,
    /// Whose each thread is (a module's id, or `app`), asked at the start and at the end.
    pub owners: Arc<dyn Fn() -> Owners + Send + Sync>,
    /// The GPU memory the app's drawing holds now, when known.
    pub gpu_memory: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
    /// This process's threads now: each one's id, name and CPU time so far; and its
    /// memory as the system counts it (read by whoever starts it: the system's own calls).
    pub threads: Arc<dyn Fn() -> Vec<(u64, String, Duration)> + Send + Sync>,
    pub memory: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
    /// What each part moved so far over the network and the disk (bytes, counting up: by
    /// its owner only), and the whole process's disk so far (read, written).
    pub io: Arc<dyn Fn() -> Vec<Io> + Send + Sync>,
    pub disk: Arc<dyn Fn() -> Option<(u64, u64)> + Send + Sync>,
    /// What the modules give of their own (`contracts::profiling`), as counters and levels:
    /// asked at each sample (whoever gives them reads them as often as it wants).
    pub readings: Arc<dyn Fn() -> (Vec<Counter>, Vec<Level>) + Send + Sync>,
    /// Told once the file is written (or couldn't be): wakes whoever waits.
    pub done: Arc<dyn Fn() + Send + Sync>,
    /// Told once a second while it records (whoever shows it follows: its time left, the
    /// resources read fresh).
    pub tick: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan").field("length", &self.length).finish_non_exhaustive()
    }
}

/// A recording under way.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    result: Mutex<Option<Receiver<Result<PathBuf, String>>>>,
    started: Instant,
    length: Duration,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder").field("length", &self.length).finish_non_exhaustive()
    }
}

/// A recording is under way (only one at a time).
static RUNNING: AtomicBool = AtomicBool::new(false);

impl Recorder {
    /// It ends now (the file is written on its thread: [`Self::finished`] says when).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// How long it has run, and how long it has left.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn remaining(&self) -> Duration {
        self.length.saturating_sub(self.started.elapsed())
    }

    /// Its file, once written (or why it couldn't be): told once, then `None`.
    pub fn finished(&self) -> Option<Result<PathBuf, String>> {
        let mut guard = self.result.lock().unwrap_or_else(PoisonError::into_inner);
        let answer = guard.as_ref()?.try_recv().ok()?;
        *guard = None;
        Some(answer)
    }
}

/// A recording begun with `plan`; refused while another runs, or if its thread can't
/// start.
pub fn start(plan: Plan) -> Result<Recorder, String> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err("a recording is already under way".to_owned());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let length = plan.length;
    let asked = Arc::clone(&stop);
    let spawned = std::thread::Builder::new().name(THREAD.into()).spawn(move || {
        let done = Arc::clone(&plan.done);
        // Whatever it calls (a module's figures) may fail: the probes go off, the slot is
        // free again and whoever waits is told, whatever happened.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| record(&plan, &asked)))
                .unwrap_or_else(|_| {
                    kernel::probe::stop();
                    Err("the recording stopped unexpectedly".to_owned())
                });
        RUNNING.store(false, Ordering::Release);
        let _ = tx.send(result);
        done();
    });
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::Release);
        return Err(e.to_string());
    }
    Ok(Recorder { stop, result: Mutex::new(Some(rx)), started: Instant::now(), length })
}

/// The recording itself, on its thread: samples until stopped or out of time, then written.
fn record(plan: &Plan, stop: &AtomicBool) -> Result<PathBuf, String> {
    let started = chrono::Utc::now();
    let mut recording = Recording::new(
        started.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        u64::try_from(plan.interval.as_millis()).unwrap_or(u64::MAX),
        system(plan),
    );
    let mut owners = clean(&(plan.owners)());
    // At least a step: a zero would sample as fast as it could.
    let interval = plan.interval.max(STEP);
    kernel::probe::start();
    let clock = Instant::now();
    let mut last = thread_times(plan, &owners);
    let mut last_io = (plan.io)();
    let mut last_disk = (plan.disk)();
    let mut at = Duration::ZERO;
    while !stop.load(Ordering::Relaxed) && clock.elapsed() < plan.length {
        let next = (at + interval).min(plan.length);
        while clock.elapsed() < next && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(STEP.min(next.saturating_sub(clock.elapsed())));
        }
        let now = clock.elapsed();
        if now.as_secs() != at.as_secs() {
            (plan.tick)();
        }
        let times = thread_times(plan, &owners);
        let mut taken = sample(at, now, &last, &times, (plan.gpu_memory)());
        // The process's memory less what this recording holds: the recorder never counts
        // itself among what the app spends.
        taken.memory = (plan.memory)().unwrap_or(0).saturating_sub(recording.weight());
        taken.memory_peak = taken.memory;
        taken.gpu_peak = taken.gpu;
        let io = (plan.io)();
        taken.io = io_since(&last_io, &io);
        last_io = io;
        let disk = (plan.disk)();
        if let (Some(before), Some(now)) = (last_disk, disk) {
            taken.disk_read = now.0.saturating_sub(before.0);
            taken.disk_written = now.1.saturating_sub(before.1);
        }
        last_disk = disk.or(last_disk);
        let (counters, levels) = (plan.readings)();
        taken.counters.extend(counters);
        taken.levels.extend(levels);
        recording.push(taken);
        last = times;
        at = now;
    }
    kernel::probe::stop();
    for (thread, owner) in clean(&(plan.owners)()).0 {
        if !owners.0.iter().any(|(t, _)| *t == thread) {
            owners.0.push((thread, owner));
        }
    }
    recording.owners = owners;
    // Its own name: one taken in the same second gets a number after it.
    let stamp = started.format("%Y%m%d-%H%M%S").to_string();
    let name = (1..100)
        .map(|n| {
            if n == 1 {
                format!("profile-{stamp}.json")
            } else {
                format!("profile-{stamp}-{n}.json")
            }
        })
        .find(|name| !plan.dir.join(name).exists())
        .unwrap_or_else(|| format!("profile-{stamp}.json"));
    write(&plan.dir, &name, &recording)
}

/// What each part moved since `before` (a part new since counts from zero; one that
/// moved nothing left out).
pub(crate) fn io_since(before: &[Io], now: &[Io]) -> Vec<Io> {
    now.iter()
        .map(|n| {
            let b = before.iter().find(|b| b.owner == n.owner).cloned().unwrap_or_default();
            Io {
                owner: profiling::name(&n.owner),
                sent: n.sent.saturating_sub(b.sent),
                received: n.received.saturating_sub(b.received),
                read: n.read.saturating_sub(b.read),
                written: n.written.saturating_sub(b.written),
            }
        })
        .filter(|i| !i.is_empty())
        .collect()
}

/// The recorder's own threads, left out of what it records (it never counts itself).
pub const OWN_THREADS: [&str; 2] = [THREAD, READ_THREAD];

/// The thread a recording is read on (its view's), the recorder's too.
pub const READ_THREAD: &str = "profiler-read";

/// The owners as the app gave them, every name cleaned.
fn clean(owners: &Owners) -> Owners {
    Owners(owners.0.iter().map(|(t, o)| (profiling::name(t), profiling::name(o))).collect())
}

/// A thread's name as a recording may keep it: one the app knows by name as it is; one
/// named after a known prefix (`lsp-` + a server's name, which may be the user's) as that
/// prefix and `*`; any other only up to its first `-` or space (a library's kind, never a
/// runtime-made rest).
pub(crate) fn anonymous_thread(name: &str, owners: &Owners) -> String {
    if owners.0.iter().any(|(t, _)| t == name) {
        return name.to_owned();
    }
    if let Some((prefix, _)) =
        owners.0.iter().find(|(t, _)| t.ends_with('-') && name.starts_with(t.as_str()))
    {
        return format!("{prefix}*");
    }
    match name.find(['-', ' ']) {
        Some(at) => format!("{}-*", &name[..at]),
        None => name.to_owned(),
    }
}

/// Each thread's CPU time now, by its id: its name (cleaned, made anonymous) and the time;
/// the recorder's own left out.
fn thread_times(plan: &Plan, owners: &Owners) -> Vec<(u64, String, Duration)> {
    (plan.threads)()
        .into_iter()
        .filter(|(_, name, _)| !OWN_THREADS.contains(&name.as_str()))
        .map(|(id, name, cpu)| {
            let name = anonymous_thread(&profiling::name(&name), owners);
            (id, if name.is_empty() { "unnamed".to_owned() } else { name }, cpu)
        })
        .collect()
}

fn micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// The interval from `from` to `to`: the threads' CPU since `before` (a thread new since
/// then counts from zero; threads of one name together), the memory, the probes' gathering.
pub(crate) fn sample(
    from: Duration,
    to: Duration,
    before: &[(u64, String, Duration)],
    now: &[(u64, String, Duration)],
    gpu: Option<u64>,
) -> Sample {
    let mut threads: Vec<ThreadCpu> = Vec::new();
    for (id, name, cpu) in now {
        let was = before.iter().find(|(i, ..)| i == id).map_or(Duration::ZERO, |b| b.2);
        let us = micros(cpu.saturating_sub(was));
        if us == 0 {
            continue;
        }
        match threads.iter_mut().find(|t| t.thread == *name) {
            Some(t) => t.us = t.us.saturating_add(us),
            None => threads.push(ThreadCpu { thread: name.clone(), us }),
        }
    }
    threads.sort_by_key(|t| std::cmp::Reverse(t.us));
    let gathered = kernel::probe::take();
    Sample {
        at_ms: millis(from),
        span_ms: millis(to.saturating_sub(from)),
        cpu_us: threads.iter().map(|t| t.us).sum(),
        memory: 0,
        gpu,
        frames: gathered.frames.count,
        frame_us: micros(gathered.frames.total),
        frame_longest_us: micros(gathered.frames.longest),
        threads,
        triggers: gathered
            .triggers
            .into_iter()
            .map(|(owner, cause, count)| Trigger {
                owner: profiling::name(owner),
                cause: cause.as_str().to_owned(),
                count,
            })
            .collect(),
        io: Vec::new(),
        disk_read: 0,
        disk_written: 0,
        memory_peak: 0,
        gpu_peak: None,
        spans: gathered
            .spans
            .iter()
            .map(|(path, calls, total)| profiling::Span {
                path: path.iter().map(|(o, w)| profiling::step(o, w)).collect(),
                calls: *calls,
                us: micros(*total),
            })
            .collect(),
        counters: gathered
            .counts
            .iter()
            .map(|&(owner, what, count)| {
                Counter::new(profiling::name(owner), profiling::name(what), count)
            })
            .collect(),
        levels: gathered
            .levels
            .iter()
            .map(|&(owner, what, peak, last)| Level {
                owner: profiling::name(owner),
                what: profiling::name(what),
                peak,
                last,
            })
            .collect(),
        busy: gathered
            .busy
            .into_iter()
            .map(|(owner, what, b)| Busy {
                owner: profiling::name(owner),
                what: profiling::name(what),
                calls: b.calls,
                us: micros(b.total),
                longest_us: micros(b.longest),
            })
            .collect(),
    }
}

/// The system's basics (nothing that names the machine or its user).
fn system(plan: &Plan) -> System {
    let mut info = sysinfo::System::new();
    info.refresh_memory();
    System {
        os: std::env::consts::OS.to_owned(),
        os_version: sysinfo::System::os_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_owned(),
        cores: std::thread::available_parallelism()
            .map_or(0, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)),
        memory: info.total_memory(),
        gpu: profiling::name(&plan.gpu),
        gpu_driver: profiling::name(&plan.gpu_driver),
        session: session().to_owned(),
        scale: plan.scale,
        app_version: profiling::name(&plan.app_version),
        build: profiling::name(&plan.build),
    }
}

/// How the window is shown: on Linux, Wayland or X11 (from which display the session has);
/// elsewhere, the system's own.
pub fn session() -> &'static str {
    if cfg!(target_os = "linux") {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            "wayland"
        } else if std::env::var_os("DISPLAY").is_some() {
            "x11"
        } else {
            "unknown"
        }
    } else {
        std::env::consts::OS
    }
}

/// `recording` written to `dir/name`, through a temporary file renamed over it.
pub fn write(dir: &Path, name: &str, recording: &Recording) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    let temporary = dir.join(format!(".{name}.tmp"));
    std::fs::write(&temporary, recording.to_json()).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        e.to_string()
    })?;
    Ok(path)
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
