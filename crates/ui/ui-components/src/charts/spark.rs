//! The small charts made of quads only, without state (the renderer draws no paths): what
//! a table's cell, a card or a screen's widget shows, hundreds a frame, for nothing — a
//! [`sparkline`]: bars, or an area in steps (a faint fill and a 1.5 px step on top), with
//! its reading under the mouse ([`Plot::reading`]). A chart someone reads (axes, legend,
//! tooltips of every series, zoom) is [`super::Chart`].
//!
//! The values come as they're kept (a ring's two slices: [`Samples`]), the newest at the
//! right of `slots` places (an empty history leaves the left blank). More values than
//! pixel columns are aggregated, one bucket a column ([`Reduce::Max`] keeps a spike,
//! [`Reduce::Last`] a level such as memory), in a [`Plot`] built only when the series'
//! generation or the room changed ([`PlotCache`]): a frame that redraws the same chart
//! computes nothing and allocates nothing. A chart never draws more than two quads per
//! physical pixel column.
//!
//! The series' colors come from [`crate::tag_colors`], in the theme's shade
//! ([`series_color`]).

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};

use crate::ds::Ctx;
use crate::palette::Palette;

/// What a series measures (its color).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Series {
    Cpu,
    Memory,
    Gpu,
    Network,
    Disk,
}

impl Series {
    pub const ALL: [Series; 5] =
        [Series::Cpu, Series::Memory, Series::Gpu, Series::Network, Series::Disk];

    /// Its named color ([`crate::tag_colors::NAMES`]).
    pub fn tag(self) -> &'static str {
        match self {
            Series::Cpu => "blue",
            Series::Memory => "green",
            Series::Gpu => "purple",
            Series::Network => "orange",
            Series::Disk => "teal",
        }
    }
}

/// Series `series`'s color in `palette`'s shade (it reads on the theme's background).
pub fn series_color(palette: &Palette, series: Series) -> Rgba {
    crate::tag_colors::named(series.tag(), palette).unwrap_or(palette.accent)
}

/// How a bucket of several values becomes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reduce {
    /// The highest (a rate, the CPU: a spike stays visible).
    Max,
    /// The newest (a level: memory).
    Last,
}

/// A series' values, oldest first, in (at most) two pieces — a ring's slices, not copied.
#[derive(Debug, Clone, Copy, Default)]
pub struct Samples<'a> {
    pub head: &'a [f32],
    pub tail: &'a [f32],
}

impl<'a> Samples<'a> {
    pub fn new(values: &'a [f32]) -> Self {
        Self { head: values, tail: &[] }
    }

    pub fn from_slices((head, tail): (&'a [f32], &'a [f32])) -> Self {
        Self { head, tail }
    }

    pub fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, i: usize) -> Option<f32> {
        match self.head.get(i) {
            Some(v) => Some(*v),
            None => self.tail.get(i - self.head.len()).copied(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        self.head.iter().chain(self.tail).copied()
    }

    /// The newest value.
    pub fn last(&self) -> Option<f32> {
        self.tail.last().or(self.head.last()).copied()
    }
}

/// A series laid on columns: one value per bucket (`None`: no value there yet).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plot {
    pub buckets: Vec<Option<f32>>,
    /// The places of the history (the newest at the right).
    pub slots: usize,
    /// The values it holds (at most `slots`).
    pub len: usize,
    /// The highest bucket.
    pub peak: f32,
}

impl Plot {
    /// `samples` in `slots` places (at least their number), on at most `columns` buckets.
    pub fn build(samples: Samples<'_>, slots: usize, columns: usize, reduce: Reduce) -> Self {
        let mut plot = Self::default();
        plot.fill(samples, slots, columns, reduce);
        plot
    }

    /// The same, into its own room (reused: nothing allocated once it's large enough).
    pub fn fill(&mut self, samples: Samples<'_>, slots: usize, columns: usize, reduce: Reduce) {
        let slots = slots.max(samples.len()).max(1);
        let count = columns.clamp(1, slots);
        self.buckets.clear();
        self.buckets.resize(count, None);
        self.slots = slots;
        self.len = samples.len();
        self.peak = 0.0;
        let first_slot = slots - samples.len();
        for (j, value) in samples.iter().enumerate() {
            let value = if value.is_finite() { value.max(0.0) } else { 0.0 };
            let bucket = (first_slot + j) * count / slots;
            let cell = &mut self.buckets[bucket];
            *cell = Some(match (reduce, *cell) {
                (Reduce::Max, Some(old)) => old.max(value),
                _ => value,
            });
        }
        self.peak = self.buckets.iter().flatten().copied().fold(0.0, f32::max);
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The newest value's bucket.
    pub fn last(&self) -> Option<f32> {
        self.buckets.iter().rev().flatten().next().copied()
    }

    /// Bucket `i`'s left edge and width in `rect` (pixels rounded: buckets never overlap).
    pub fn bucket_x(&self, rect: Rect, i: usize) -> (f32, f32) {
        let n = self.buckets.len().max(1) as f32;
        let at = |k: usize| (rect.min_x() + rect.size.width * k as f32 / n).round();
        let (x0, x1) = (at(i), at(i + 1));
        (x0, x1 - x0)
    }

    /// What's under `x` in `rect`: the bucket, its value, how many places back its newest
    /// value is (0: the newest); `None` outside, or where there's no value.
    pub fn reading(&self, rect: Rect, x: f32) -> Option<Reading> {
        if self.buckets.is_empty() || x < rect.min_x() || x >= rect.max_x() {
            return None;
        }
        let n = self.buckets.len();
        let at = ((x - rect.min_x()) / rect.size.width * n as f32).floor() as usize;
        let bucket = at.min(n - 1);
        let value = self.buckets[bucket]?;
        // The newest slot of the bucket (its last place before the next one's first).
        let last_slot =
            ((bucket + 1) * self.slots).div_ceil(n).saturating_sub(1).min(self.slots - 1);
        let ago = self.slots - 1 - last_slot;
        let (x0, w) = self.bucket_x(rect, bucket);
        Some(Reading { bucket, value, ago, x: (x0 + w / 2.0).floor() })
    }
}

/// A chart's value under the mouse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub bucket: usize,
    pub value: f32,
    /// How many places before the newest (times the step between values: how long ago).
    pub ago: usize,
    /// The bucket's middle (where the line is drawn).
    pub x: f32,
}

/// A plot kept for a series and the room it was built for: built again only when the
/// series has new values (its generation), the room or the history's length changed.
#[derive(Debug, Clone, Default)]
pub struct PlotCache {
    key: Option<(u64, usize, usize, Reduce)>,
    plot: Plot,
}

impl PlotCache {
    /// The plot of `samples` (generation `generation`) in `slots` places on `columns`.
    pub fn get(
        &mut self,
        generation: u64,
        samples: Samples<'_>,
        slots: usize,
        columns: usize,
        reduce: Reduce,
    ) -> &Plot {
        let key = Some((generation, slots, columns, reduce));
        if self.key != key {
            self.plot.fill(samples, slots, columns, reduce);
            self.key = key;
        }
        &self.plot
    }

    /// Whether `get` with these would build it again.
    pub fn is_stale(&self, generation: u64, slots: usize, columns: usize, reduce: Reduce) -> bool {
        self.key != Some((generation, slots, columns, reduce))
    }

    /// The last plot built (whatever it was for).
    pub fn plot(&self) -> &Plot {
        &self.plot
    }
}

impl kernel::Purge for PlotCache {
    fn purge(&mut self) {
        *self = Self::default();
    }
}

/// The smallest "nice" number at least `v` (1, 2, 2.5 or 5 times a power of ten): a scale's
/// top.
pub fn nice_ceiling(v: f32) -> f32 {
    if !v.is_finite() || v <= 0.0 {
        return 1.0;
    }
    let magnitude = 10f32.powf(v.log10().floor());
    for step in [1.0, 2.0, 2.5, 5.0, 10.0] {
        let nice = step * magnitude;
        if nice >= v * (1.0 - 1e-6) {
            return nice;
        }
    }
    10.0 * magnitude
}

/// A scale's top for `peak`: at least `floor` (an idle series stays low), rounded up nicely.
pub fn scale_for(peak: f32, floor: f32) -> f32 {
    nice_ceiling(peak.max(floor).max(f32::MIN_POSITIVE))
}

/// How a sparkline draws its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// A bar a bucket (very small sparklines).
    Bars,
    /// An area in steps: a faint fill and a step on top.
    Steps,
}

/// How tall `value` stands in `h` on a scale up to `max` (a value above 0 at least a pixel).
fn height(value: f32, max: f32, h: f32, line: f32) -> f32 {
    let max = max.max(f32::MIN_POSITIVE);
    let raw = (value / max).clamp(0.0, 1.0) * h;
    if value > 0.0 { raw.max(line).round() } else { 0.0 }
}

/// `plot` drawn in `rect` on a scale up to `max`, in `color`.
pub fn sparkline(
    layer: &mut Layer,
    ds: &Ctx<'_>,
    rect: Rect,
    plot: &Plot,
    color: Rgba,
    style: Style,
    max: f32,
) {
    if rect.size.width < 1.0 || rect.size.height < 1.0 {
        return;
    }
    match style {
        Style::Bars => bars(layer, ds, rect, plot, color, max),
        Style::Steps => steps(layer, ds, rect, plot, color, max),
    }
}

fn bars(layer: &mut Layer, ds: &Ctx<'_>, rect: Rect, plot: &Plot, color: Rgba, max: f32) {
    let line = ds.line();
    let newest = plot.buckets.iter().rposition(Option::is_some);
    for (i, value) in plot.buckets.iter().enumerate() {
        let Some(value) = *value else { continue };
        let h = height(value, max, rect.size.height, line);
        if h <= 0.0 {
            continue;
        }
        let (x, w) = plot.bucket_x(rect, i);
        // A pixel apart when there's room for one.
        let w = if w >= 3.0 { w - line } else { w.max(1.0) };
        let bar = Rect::new(x, rect.max_y() - h, w, h);
        let alpha = if Some(i) == newest { 0.95 } else { 0.6 };
        layer.quad(Quad::filled(bar, ds.fade(color.with_alpha(color.a * alpha))));
    }
}

fn steps(layer: &mut Layer, ds: &Ctx<'_>, rect: Rect, plot: &Plot, color: Rgba, max: f32) {
    let line = ds.px(1.5).max(1.0);
    let fill = ds.fade(color.with_alpha(color.a * 0.16));
    let edge = ds.fade(color);
    let n = plot.buckets.len();
    let mut i = 0;
    // Wide enough buckets join their steps with a riser (dense ones read as an area).
    let risers = n > 0 && rect.size.width / n as f32 >= 3.0;
    let mut previous: Option<f32> = None;
    while i < n {
        let Some(value) = plot.buckets[i] else {
            previous = None;
            i += 1;
            continue;
        };
        let h = height(value, max, rect.size.height, line);
        // A run of buckets standing as tall: one fill, one step.
        let mut end = i + 1;
        while end < n
            && plot.buckets[end].is_some_and(|v| height(v, max, rect.size.height, line) == h)
        {
            end += 1;
        }
        let (x0, _) = plot.bucket_x(rect, i);
        let (last_x, last_w) = plot.bucket_x(rect, end - 1);
        let w = last_x + last_w - x0;
        let top = rect.max_y() - h;
        if h > 0.0 {
            layer.quad(Quad::filled(Rect::new(x0, top, w, h), fill));
        }
        let step_y = (top - line / 2.0).clamp(rect.min_y(), rect.max_y() - line).round();
        layer.quad(Quad::filled(Rect::new(x0, step_y, w, line), edge));
        if risers && let Some(before) = previous {
            let (a, b) = (before.min(step_y), before.max(step_y));
            if b - a > line {
                layer.quad(Quad::filled(Rect::new(x0, a, line, b - a + line), edge));
            }
        }
        previous = Some(step_y);
        i = end;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/charts/spark.rs"]
mod tests;
