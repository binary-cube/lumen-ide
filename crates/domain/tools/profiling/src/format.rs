//! The recording's format: one JSON object, its version first.
//!
//! - `system`: the basics that explain performance (no name, no id of the machine);
//! - `owners`: which module (or `app`) each thread works for, as the app knows them;
//! - `samples`: one each interval (`interval_ms`), what happened in it.
//!
//! A sample's numbers cover its interval (CPU time, frames, triggers, busy time), so two
//! samples merge by adding them (and taking the larger longest).

use serde::{Deserialize, Serialize};

/// The format's version (a reader refuses a newer one).
pub const VERSION: u32 = 1;

/// The most samples kept: past them, the oldest are merged by twos (the interval kept per
/// sample doubles where they are), so a long recording stays small.
pub const MAX_SAMPLES: usize = 3_000;

/// The longest a name is kept (characters).
const NAME_MAX: usize = 48;

/// A name as it may be kept: ASCII letters, digits and `-_.:/ ` only, at most
/// [`NAME_MAX`] characters (a thread's name is set by code, ours or a library's: never a
/// user's text, but cleaned all the same).
pub fn name(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_.:/ ".contains(*c))
        .take(NAME_MAX)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The system it ran on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct System {
    /// `macos`, `linux`, `windows`.
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub cores: u32,
    /// The machine's memory (bytes).
    pub memory: u64,
    /// The graphics adapter and its driver, as the system names them.
    pub gpu: String,
    pub gpu_driver: String,
    /// How the window is shown: `wayland`, `x11`, `macos`, `windows`.
    pub session: String,
    /// The window's scale (2 on a Retina screen).
    pub scale: f32,
    /// The app's version and its build (`release`, `debug`).
    pub app_version: String,
    pub build: String,
}

/// Which module each thread works for (its name, or a prefix ending with `-`), and the
/// module's id (`app` for the app's own).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owners(pub Vec<(String, String)>);

impl Owners {
    /// Whose thread `thread` is (`None`: nobody said).
    pub fn of(&self, thread: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(t, _)| t == thread || (t.ends_with('-') && thread.starts_with(t.as_str())))
            .map(|(_, owner)| owner.as_str())
    }
}

/// A thread's CPU time in a sample (microseconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadCpu {
    pub thread: String,
    pub us: u64,
}

/// Who asked for frames in a sample, why, how many times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub owner: String,
    pub cause: String,
    pub count: u64,
}

/// What a part took on the interface's thread in a sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Busy {
    pub owner: String,
    /// What it did: `paint`, `poll`…
    pub what: String,
    pub calls: u64,
    pub us: u64,
    pub longest_us: u64,
}

/// What a part sent and received over the network and read and wrote on the disk in a
/// sample (bytes), as it counts them itself: whose, never what or where.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Io {
    pub owner: String,
    pub sent: u64,
    pub received: u64,
    pub read: u64,
    pub written: u64,
}

impl Io {
    pub(crate) fn add(&mut self, other: &Io) {
        self.sent = self.sent.saturating_add(other.sent);
        self.received = self.received.saturating_add(other.received);
        self.read = self.read.saturating_add(other.read);
        self.written = self.written.saturating_add(other.written);
    }

    /// Nothing moved.
    pub fn is_empty(&self) -> bool {
        self.sent == 0 && self.received == 0 && self.read == 0 && self.written == 0
    }
}

/// A counter's unit when it counts things (the default, not written).
pub const COUNT: &str = "count";

fn count_unit() -> String {
    COUNT.to_owned()
}

fn is_count(unit: &str) -> bool {
    unit == COUNT
}

/// Timed work as it nested in a sample (a flame graph's): its way down (`owner·what` from
/// the outermost), its calls, its time with what it held (microseconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub path: Vec<String>,
    pub calls: u64,
    pub us: u64,
}

/// A way down's step as kept: whose and what (`log-viewer·paint`).
pub fn step(owner: &str, what: &str) -> String {
    format!("{}·{}", name(owner), name(what))
}

/// What a part counted in a sample (`kernel::probe::count`, or its module's own figures):
/// how many, in its unit (`count`, `bytes`, `us`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counter {
    pub owner: String,
    pub what: String,
    pub count: u64,
    #[serde(default = "count_unit", skip_serializing_if = "is_count")]
    pub unit: String,
}

impl Counter {
    /// `count` of `what` of `owner`'s, counted as things.
    pub fn new(owner: impl Into<String>, what: impl Into<String>, count: u64) -> Self {
        Self { owner: owner.into(), what: what.into(), count, unit: count_unit() }
    }
}

/// What a part held in a sample (`kernel::probe::level`): its highest, its last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Level {
    pub owner: String,
    pub what: String,
    pub peak: u64,
    pub last: u64,
}

/// One interval.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Its start, from the recording's (milliseconds), and how long it covers.
    pub at_ms: u64,
    pub span_ms: u64,
    /// The process's CPU time in it (microseconds: 1 000 000 a second is one core busy).
    pub cpu_us: u64,
    /// The process's memory at its end (bytes), as the system counts it; the GPU's memory
    /// the app's drawing holds, when known.
    pub memory: u64,
    pub gpu: Option<u64>,
    /// The frames drawn, their time in all, the longest (microseconds).
    pub frames: u64,
    pub frame_us: u64,
    pub frame_longest_us: u64,
    pub threads: Vec<ThreadCpu>,
    pub triggers: Vec<Trigger>,
    pub busy: Vec<Busy>,
    /// What the parts counted and held (whatever a part chose to probe).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counters: Vec<Counter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<Level>,
    /// What the parts moved over the network and the disk, and the whole process's disk
    /// as the system counts it (bytes read, written).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub io: Vec<Io>,
    #[serde(default)]
    pub disk_read: u64,
    #[serde(default)]
    pub disk_written: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
    /// The highest memory and GPU memory in it (a merged sample keeps its parts' highest:
    /// a spike stays seen however long the recording).
    #[serde(default)]
    pub memory_peak: u64,
    #[serde(default)]
    pub gpu_peak: Option<u64>,
}

impl Sample {
    /// About how much memory it takes (bytes): what a recording kept so far holds, taken
    /// off the process's memory so the recorder never counts itself.
    pub fn weight(&self) -> u64 {
        let text = |s: &String| s.capacity() as u64;
        let mut bytes = std::mem::size_of::<Sample>() as u64;
        bytes += self.threads.iter().map(|t| 32 + text(&t.thread)).sum::<u64>();
        bytes += self.triggers.iter().map(|t| 40 + text(&t.owner) + text(&t.cause)).sum::<u64>();
        bytes += self.busy.iter().map(|b| 72 + text(&b.owner) + text(&b.what)).sum::<u64>();
        bytes += self
            .counters
            .iter()
            .map(|c| 64 + text(&c.owner) + text(&c.what) + text(&c.unit))
            .sum::<u64>();
        bytes += self.levels.iter().map(|l| 64 + text(&l.owner) + text(&l.what)).sum::<u64>();
        bytes += self.io.iter().map(|i| 56 + text(&i.owner)).sum::<u64>();
        bytes += self
            .spans
            .iter()
            .map(|s| 40 + s.path.iter().map(|p| 24 + text(p)).sum::<u64>())
            .sum::<u64>();
        bytes
    }

    /// `next` added to this one (the interval after it): numbers summed, the longest the
    /// larger, the end's memory the later one's.
    pub fn merge(&mut self, next: Sample) {
        self.span_ms = next.at_ms.saturating_add(next.span_ms).saturating_sub(self.at_ms);
        self.cpu_us = self.cpu_us.saturating_add(next.cpu_us);
        self.memory_peak = self.memory_peak.max(self.memory).max(next.memory_peak).max(next.memory);
        self.gpu_peak = match (self.gpu_peak.or(self.gpu), next.gpu_peak.or(next.gpu)) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.memory = next.memory;
        self.gpu = next.gpu.or(self.gpu);
        self.frames = self.frames.saturating_add(next.frames);
        self.frame_us = self.frame_us.saturating_add(next.frame_us);
        self.frame_longest_us = self.frame_longest_us.max(next.frame_longest_us);
        for t in next.threads {
            match self.threads.iter_mut().find(|m| m.thread == t.thread) {
                Some(m) => m.us = m.us.saturating_add(t.us),
                None => self.threads.push(t),
            }
        }
        for t in next.triggers {
            match self.triggers.iter_mut().find(|m| m.owner == t.owner && m.cause == t.cause) {
                Some(m) => m.count = m.count.saturating_add(t.count),
                None => self.triggers.push(t),
            }
        }
        for s in next.spans {
            match self.spans.iter_mut().find(|m| m.path == s.path) {
                Some(m) => {
                    m.calls = m.calls.saturating_add(s.calls);
                    m.us = m.us.saturating_add(s.us);
                }
                None => self.spans.push(s),
            }
        }
        self.disk_read = self.disk_read.saturating_add(next.disk_read);
        self.disk_written = self.disk_written.saturating_add(next.disk_written);
        for i in next.io {
            match self.io.iter_mut().find(|m| m.owner == i.owner) {
                Some(m) => m.add(&i),
                None => self.io.push(i),
            }
        }
        for c in next.counters {
            let same =
                |m: &&mut Counter| m.owner == c.owner && m.what == c.what && m.unit == c.unit;
            match self.counters.iter_mut().find(same) {
                Some(m) => m.count = m.count.saturating_add(c.count),
                None => self.counters.push(c),
            }
        }
        for l in next.levels {
            match self.levels.iter_mut().find(|m| m.owner == l.owner && m.what == l.what) {
                Some(m) => {
                    m.peak = m.peak.max(l.peak);
                    m.last = l.last;
                }
                None => self.levels.push(l),
            }
        }
        for b in next.busy {
            match self.busy.iter_mut().find(|m| m.owner == b.owner && m.what == b.what) {
                Some(m) => {
                    m.calls = m.calls.saturating_add(b.calls);
                    m.us = m.us.saturating_add(b.us);
                    m.longest_us = m.longest_us.max(b.longest_us);
                }
                None => self.busy.push(b),
            }
        }
    }
}

/// A recording.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Recording {
    pub version: u32,
    /// When it started (RFC 3339, UTC) and how long it ran (milliseconds).
    pub started: String,
    pub duration_ms: u64,
    /// The interval it sampled at (milliseconds).
    pub interval_ms: u64,
    pub system: System,
    pub owners: Owners,
    pub samples: Vec<Sample>,
    /// About how much memory its samples hold (bytes), kept as they come (never written).
    #[serde(skip)]
    weight: u64,
}

/// Two recordings are the same by what they hold (the weight is only an estimate of the
/// memory it takes, which differs with how its texts were made).
impl PartialEq for Recording {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.started == other.started
            && self.duration_ms == other.duration_ms
            && self.interval_ms == other.interval_ms
            && self.system == other.system
            && self.owners == other.owners
            && self.samples == other.samples
    }
}

impl Recording {
    pub fn new(started: String, interval_ms: u64, system: System) -> Self {
        Self { version: VERSION, started, interval_ms, system, ..Self::default() }
    }

    /// `sample` added; past [`MAX_SAMPLES`], the older half's samples merged by twos.
    /// About how much memory its samples hold (bytes): taken off the process's memory, so
    /// the recorder never counts itself.
    pub fn weight(&self) -> u64 {
        self.weight
    }

    pub fn push(&mut self, sample: Sample) {
        self.duration_ms = self.duration_ms.max(sample.at_ms.saturating_add(sample.span_ms));
        self.weight = self.weight.saturating_add(sample.weight());
        self.samples.push(sample);
        if self.samples.len() > MAX_SAMPLES {
            let half = self.samples.len() / 2;
            let mut merged: Vec<Sample> = Vec::with_capacity(self.samples.len());
            let mut old = self.samples.drain(..half - half % 2);
            while let Some(mut a) = old.next() {
                if let Some(b) = old.next() {
                    a.merge(b);
                }
                merged.push(a);
            }
            drop(old);
            merged.append(&mut self.samples);
            self.samples = merged;
            self.weight = self.samples.iter().map(Sample::weight).fold(0, u64::saturating_add);
        }
    }

    /// As JSON (one line).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Read from JSON; a newer version than [`VERSION`] (or not one) refused.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut recording: Recording = serde_json::from_str(text).map_err(|e| e.to_string())?;
        recording.weight =
            recording.samples.iter().map(Sample::weight).fold(0, u64::saturating_add);
        if recording.version == 0 || recording.version > VERSION {
            return Err(format!("format {} isn't read by this version", recording.version));
        }
        Ok(recording)
    }
}

#[cfg(test)]
#[path = "../tests/unit/format.rs"]
mod tests;
