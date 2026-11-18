//! What the graphs draw: the whole app's and each part's values over the last minute (or
//! five, or fifteen: the `[tasks] history` setting), kept by the hub — one per scope — in
//! rings of a fixed capacity, allocated once (`f32`: five series a part). Each look adds a
//! value to each ring (the oldest goes); nothing is copied for the views, which read the
//! rings' two slices under the hub's short lock when they draw them. The whole is held
//! under [`BUDGET`]: past it, a value is kept every few looks instead of every one (the
//! step grows, the rings shrink) — and it comes back when parts go. A part gone is
//! forgotten [`FORGET`] after it was last seen.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::frame::Frame;

/// The most the rings of one history hold together (bytes).
pub const BUDGET: usize = 1 << 20;
/// How long a part gone is kept (its graphs still drawn).
pub const FORGET: Duration = Duration::from_secs(30);
/// The series a part has ([`Metric::ALL`]).
pub const SERIES: usize = 5;

/// What a series measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Metric {
    /// Percent of one core.
    Cpu,
    /// Bytes.
    Memory,
    /// Bytes.
    Gpu,
    /// Bytes a second, sent and received together.
    Net,
    /// Bytes a second, read and written together.
    Disk,
}

impl Metric {
    pub const ALL: [Metric; SERIES] =
        [Metric::Cpu, Metric::Memory, Metric::Gpu, Metric::Net, Metric::Disk];

    fn index(self) -> usize {
        self as usize
    }
}

/// The last values of one series, oldest first, at most its capacity (allocated at once).
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    values: VecDeque<f32>,
    capacity: usize,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self { values: VecDeque::with_capacity(capacity), capacity }
    }

    /// `value` added; the oldest gone when it's full.
    pub fn push(&mut self, value: f32) {
        if self.values.len() >= self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    /// Its values, oldest first, in (at most) two pieces — nothing copied.
    pub fn as_slices(&self) -> (&[f32], &[f32]) {
        self.values.as_slices()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The newest value.
    pub fn last(&self) -> Option<f32> {
        self.values.back().copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        self.values.iter().copied()
    }

    /// Its values written into `out` (cleared first: its room reused).
    pub fn copy_into(&self, out: &mut Vec<f32>) {
        out.clear();
        let (a, b) = self.as_slices();
        out.extend_from_slice(a);
        out.extend_from_slice(b);
    }

    /// Holds `capacity` values from now (the oldest dropped when fewer).
    pub fn set_capacity(&mut self, capacity: usize) {
        let capacity = capacity.max(1);
        while self.values.len() > capacity {
            self.values.pop_front();
        }
        if capacity > self.values.capacity() {
            self.values.reserve_exact(capacity - self.values.len());
        } else {
            self.values.shrink_to(capacity);
        }
        self.capacity = capacity;
    }
}

/// A part's (or the whole app's) series, and when it was last seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    rings: [Ring; SERIES],
    seen: Instant,
}

impl Trace {
    fn new(capacity: usize, now: Instant) -> Self {
        Self { rings: std::array::from_fn(|_| Ring::new(capacity)), seen: now }
    }

    /// Series `metric`.
    pub fn get(&self, metric: Metric) -> &Ring {
        &self.rings[metric.index()]
    }

    /// The values of a look, by [`Metric::ALL`]; one not known (a part without GPU) leaves
    /// its series as it is.
    fn push(&mut self, values: [Option<f32>; SERIES]) {
        for (ring, value) in self.rings.iter_mut().zip(values) {
            if let Some(value) = value {
                ring.push(value);
            }
        }
    }

    fn set_capacity(&mut self, capacity: usize) {
        for ring in &mut self.rings {
            ring.set_capacity(capacity);
        }
    }
}

/// One scope's values over time: the whole app's and each part's (by its key).
#[derive(Debug, Clone)]
pub struct History {
    total: Trace,
    parts: HashMap<String, Trace>,
    /// The values the window wants (its length over the interval).
    length: usize,
    /// The values each ring keeps (`length / step`).
    capacity: usize,
    /// A value kept every `step` looks (1 unless the budget asks for fewer).
    step: usize,
    looks: u64,
    generation: u64,
    window: Duration,
    every: Duration,
}

impl History {
    /// Over `window`, a look every `every`.
    pub fn new(window: Duration, every: Duration) -> Self {
        let length = length(window, every);
        Self {
            total: Trace::new(length, Instant::now()),
            parts: HashMap::new(),
            length,
            capacity: length,
            step: 1,
            looks: 0,
            generation: 0,
            window,
            every,
        }
    }

    /// Over another window or interval (the settings changed): the rings resized.
    pub fn set_window(&mut self, window: Duration, every: Duration) {
        if (window, every) == (self.window, self.every) {
            return;
        }
        (self.window, self.every) = (window, every);
        self.length = length(window, every);
        self.rebalance();
    }

    /// A look's values added (every `step` looks); the parts not seen for [`FORGET`]
    /// forgotten.
    pub fn record(&mut self, frame: &Frame, now: Instant) {
        self.looks += 1;
        for line in &frame.lines {
            match self.parts.get_mut(&line.key) {
                Some(trace) => trace.seen = now,
                None => {
                    self.parts.insert(line.key.clone(), Trace::new(self.capacity, now));
                }
            }
        }
        self.parts.retain(|_, t| now.saturating_duration_since(t.seen) < FORGET);
        self.rebalance();
        if !(self.looks - 1).is_multiple_of(self.step as u64) {
            return;
        }
        let bytes = |b: u64| b as f32;
        self.total.seen = now;
        self.total.push([
            Some(frame.cpu),
            Some(bytes(frame.memory)),
            frame.gpu.map(bytes),
            Some(frame.net),
            frame.disk,
        ]);
        for line in &frame.lines {
            if let Some(trace) = self.parts.get_mut(&line.key) {
                trace.push([
                    Some(line.cpu),
                    line.memory.map(bytes),
                    line.gpu.map(bytes),
                    line.net,
                    line.disk,
                ]);
            }
        }
        self.generation += 1;
    }

    /// The step and the rings' capacity that keep it under [`BUDGET`].
    fn rebalance(&mut self) {
        let rings = (self.parts.len() + 1) * SERIES;
        let most = (BUDGET / (rings * std::mem::size_of::<f32>())).max(1);
        let step = self.length.div_ceil(most).max(1);
        let capacity = self.length.div_ceil(step).max(1);
        self.step = step;
        if capacity != self.capacity {
            self.capacity = capacity;
            self.total.set_capacity(capacity);
            for trace in self.parts.values_mut() {
                trace.set_capacity(capacity);
            }
        }
    }

    /// The whole app's series.
    pub fn total(&self) -> &Trace {
        &self.total
    }

    /// Part `key`'s series (`None`: never seen, or forgotten).
    pub fn part(&self, key: &str) -> Option<&Trace> {
        self.parts.get(key)
    }

    pub fn parts(&self) -> usize {
        self.parts.len()
    }

    /// A value is kept every this many looks.
    pub fn step(&self) -> usize {
        self.step
    }

    /// The values each ring keeps.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// What the rings may hold together (bytes, by their capacity).
    pub fn bytes(&self) -> usize {
        (self.parts.len() + 1) * SERIES * self.capacity * std::mem::size_of::<f32>()
    }

    /// Bumped by each value kept (what was drawn from it is old).
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(Duration::from_secs(60), Duration::from_secs(1))
    }
}

/// The values a window of `window` keeps, a look every `every`.
fn length(window: Duration, every: Duration) -> usize {
    let every = every.as_secs_f64().max(0.001);
    ((window.as_secs_f64() / every).round() as usize).max(1)
}

#[cfg(test)]
#[path = "../../tests/unit/collector/history.rs"]
mod tests;
