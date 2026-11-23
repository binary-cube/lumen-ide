//! An axis' scale: from values to a place along it, its ticks at "nice" steps (1, 2, 5 ×
//! 10ⁿ for numbers; a second, a minute, an hour, a day, a month, a year for moments) and
//! their labels, as short as their step lets them be.

/// The ticks an axis aims for (a few more or less as the steps fall).
pub const TICKS: usize = 5;

/// Seconds in a day.
const DAY: f64 = 86_400.0;

/// An axis' range and how values are placed on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub lo: f64,
    pub hi: f64,
    /// Logarithmic (base 10): only what's above zero shows.
    pub log: bool,
}

impl Scale {
    /// Value `v`'s place along the axis, 0 at `lo` and 1 at `hi`.
    pub fn unit(&self, v: f64) -> f64 {
        let (lo, hi, v) = if self.log {
            (
                self.lo.max(f64::MIN_POSITIVE).log10(),
                self.hi.log10(),
                v.max(f64::MIN_POSITIVE).log10(),
            )
        } else {
            (self.lo, self.hi, v)
        };
        if hi > lo { (v - lo) / (hi - lo) } else { 0.5 }
    }

    /// The value at place `t` (0 at `lo`, 1 at `hi`).
    pub fn value(&self, t: f64) -> f64 {
        if self.log {
            let (lo, hi) = (self.lo.max(f64::MIN_POSITIVE).log10(), self.hi.log10());
            10f64.powf(lo + t * (hi - lo))
        } else {
            self.lo + t * (self.hi - self.lo)
        }
    }
}

/// An axis' ticks: where they are and what they say.
#[derive(Debug, Clone, PartialEq)]
pub struct Ticks {
    pub scale: Scale,
    pub values: Vec<f64>,
    pub labels: Vec<String>,
}

/// The step of about `span / count` rounded to 1, 2 or 5 × 10ⁿ.
pub fn nice_step(span: f64, count: usize) -> f64 {
    if !(span.is_finite() && span > 0.0) {
        return 1.0;
    }
    let rough = span / count.max(1) as f64;
    let power = 10f64.powf(rough.log10().floor());
    let fraction = rough / power;
    let nice = if fraction < 1.5 {
        1.0
    } else if fraction < 3.5 {
        2.0
    } else if fraction < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * power
}

/// About `count` ticks within `lo..=hi` (never widened past them), at the first of 1, 2,
/// 2.5, 5 × 10ⁿ at least as long as a `count`th of the span: a document's chart (Mermaid's
/// `xychart`) keeps the range it was given.
pub fn ticks_within(lo: f64, hi: f64, count: usize) -> Vec<f64> {
    let span = (hi - lo).abs().max(f64::EPSILON);
    let raw = span / count.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step =
        [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(raw);
    let mut out = Vec::new();
    let mut t = (lo / step).ceil() * step;
    while t <= hi + step * 1e-9 && out.len() < MAX_TICKS {
        out.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
    }
    out
}

/// The most ticks [`ticks_within`] and [`number_ticks`] give.
const MAX_TICKS: usize = 64;

/// The largest value a scale reaches either way: a quarter of the float's range, so the
/// span between two such values and the whole steps around them stay finite.
const MOST_TICKED: f64 = f64::MAX / 4.0;

/// Ticks for values from `lo` to `hi` (the range widened to whole steps; a single value
/// gets room around it; `zero` keeps 0 in it, as bars need).
pub fn number_ticks(lo: f64, hi: f64, zero: bool) -> Ticks {
    let (mut lo, mut hi) = if lo.is_finite() && hi.is_finite() { (lo, hi) } else { (0.0, 1.0) };
    // Values near the float's ends: held within a range whose span (and its whole steps)
    // stays finite.
    (lo, hi) = (lo.clamp(-MOST_TICKED, MOST_TICKED), hi.clamp(-MOST_TICKED, MOST_TICKED));
    if zero {
        (lo, hi) = (lo.min(0.0), hi.max(0.0));
    }
    if hi <= lo {
        let pad = if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
        (lo, hi) = (lo - pad, hi + pad);
    }
    let step = nice_step(hi - lo, TICKS);
    let (from, to) = ((lo / step).floor() * step, (hi / step).ceil() * step);
    // Never more than a scale can show (a step's float error can't make thousands).
    let count = (((to - from) / step).round() as usize).min(MAX_TICKS);
    let values: Vec<f64> = (0..=count).map(|i| clean(from + i as f64 * step)).collect();
    let labels = values.iter().map(|&v| number_label(v, step)).collect();
    Ticks { scale: Scale { lo: from, hi: to, log: false }, values, labels }
}

/// Ticks on a logarithmic scale: the powers of ten from below `lo` to above `hi` (both above
/// zero; what isn't is left out).
pub fn log_ticks(lo: f64, hi: f64) -> Ticks {
    let lo = if lo > 0.0 { lo } else { 1.0 };
    let hi = hi.max(lo * 10.0);
    let (from, to) = (lo.log10().floor() as i32, hi.log10().ceil() as i32);
    let values: Vec<f64> = (from..=to).map(|p| 10f64.powi(p)).collect();
    let labels = values.iter().map(|&v| number_label(v, v)).collect();
    Ticks { scale: Scale { lo: 10f64.powi(from), hi: 10f64.powi(to), log: true }, values, labels }
}

/// A value's float noise taken off (0.30000000000000004 → 0.3), -0 made 0.
fn clean(v: f64) -> f64 {
    let scaled = v * 1e9;
    // Too large to carry noise worth taking off (and to scale without overflowing).
    if !scaled.is_finite() {
        return v;
    }
    let rounded = scaled.round() / 1e9;
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// A number as its axis says it: as many decimals as its step needs, thousands as k, M, G
/// past ten thousand.
pub fn number_label(v: f64, step: f64) -> String {
    let abs = v.abs();
    for (unit, suffix) in [(1e12, "T"), (1e9, "G"), (1e6, "M"), (1e4, "k")] {
        if abs >= unit && step.abs() >= unit / 1000.0 {
            let unit = if suffix == "k" { 1e3 } else { unit };
            return format!("{}{suffix}", trimmed(v / unit, step / unit));
        }
    }
    trimmed(v, step)
}

/// `v` with the decimals `step` needs (0.25 → 2, 2.5 → 1), no trailing zeros.
fn trimmed(v: f64, step: f64) -> String {
    let decimals = decimals_of(step);
    let text = format!("{v:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// How many decimals write `step` whole (at most [`MAX_DECIMALS`]).
fn decimals_of(step: f64) -> usize {
    let step = step.abs();
    (0..MAX_DECIMALS)
        .find(|&d| {
            let scaled = step * 10f64.powi(d as i32);
            (scaled - scaled.round()).abs() <= 1e-6 * scaled.max(1.0)
        })
        .unwrap_or(MAX_DECIMALS)
}

/// The most decimals a label shows.
const MAX_DECIMALS: usize = 6;

/// A moment's step: its length in seconds and how its labels read.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TimeStep {
    seconds: f64,
    unit: TimeUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeUnit {
    Second,
    Minute,
    Day,
    Month,
    Year,
}

/// The steps a time axis takes, shortest first.
const TIME_STEPS: [TimeStep; 20] = {
    use TimeUnit::*;
    [
        TimeStep { seconds: 1.0, unit: Second },
        TimeStep { seconds: 2.0, unit: Second },
        TimeStep { seconds: 5.0, unit: Second },
        TimeStep { seconds: 10.0, unit: Second },
        TimeStep { seconds: 15.0, unit: Second },
        TimeStep { seconds: 30.0, unit: Second },
        TimeStep { seconds: 60.0, unit: Minute },
        TimeStep { seconds: 120.0, unit: Minute },
        TimeStep { seconds: 300.0, unit: Minute },
        TimeStep { seconds: 600.0, unit: Minute },
        TimeStep { seconds: 1_800.0, unit: Minute },
        TimeStep { seconds: 3_600.0, unit: Minute },
        TimeStep { seconds: 3.0 * 3_600.0, unit: Minute },
        TimeStep { seconds: 6.0 * 3_600.0, unit: Minute },
        TimeStep { seconds: 12.0 * 3_600.0, unit: Minute },
        TimeStep { seconds: DAY, unit: Day },
        TimeStep { seconds: 7.0 * DAY, unit: Day },
        TimeStep { seconds: 30.0 * DAY, unit: Month },
        TimeStep { seconds: 91.0 * DAY, unit: Month },
        TimeStep { seconds: 365.0 * DAY, unit: Year },
    ]
};

/// Ticks for moments from `lo` to `hi` (seconds since the epoch, UTC): the step among
/// [`TIME_STEPS`] nearest a [`TICKS`]th of the span (years beyond, by nice steps of years).
pub fn time_ticks(lo: f64, hi: f64) -> Ticks {
    let (lo, hi) = if hi > lo { (lo, hi) } else { (lo - 1.0, lo + 1.0) };
    let rough = (hi - lo) / TICKS as f64;
    let step = TIME_STEPS.iter().copied().find(|s| s.seconds >= rough).unwrap_or(TimeStep {
        seconds: nice_step(rough / (365.0 * DAY), 1).max(1.0) * 365.0 * DAY,
        unit: TimeUnit::Year,
    });
    let values: Vec<f64> = match step.unit {
        TimeUnit::Month | TimeUnit::Year => calendar_ticks(lo, hi, step),
        _ => {
            let from = (lo / step.seconds).ceil() * step.seconds;
            let count = ((hi - from) / step.seconds).floor().max(0.0) as usize;
            (0..=count).map(|i| from + i as f64 * step.seconds).collect()
        }
    };
    let labels = values.iter().map(|&v| time_label(v, step.unit)).collect();
    Ticks { scale: Scale { lo, hi, log: false }, values, labels }
}

/// Month or year starts from `lo` to `hi`, every so many of them.
fn calendar_ticks(lo: f64, hi: f64, step: TimeStep) -> Vec<f64> {
    let months = match step.unit {
        TimeUnit::Year => ((step.seconds / (365.0 * DAY)).round() as i64).max(1) * 12,
        _ => ((step.seconds / (30.0 * DAY)).round() as i64).max(1),
    };
    let (y, m, _) = civil((lo / DAY).floor() as i64);
    let mut index = y * 12 + (m as i64 - 1);
    index = index.div_euclid(months) * months;
    let mut out = Vec::new();
    loop {
        let at = days_from_civil(index.div_euclid(12), (index.rem_euclid(12) + 1) as u32, 1) as f64
            * DAY;
        if at > hi {
            break;
        }
        if at >= lo {
            out.push(at);
        }
        index += months;
        if out.len() > 64 {
            break;
        }
    }
    out
}

/// A moment as an axis with steps of `unit` says it (UTC).
fn time_label(at: f64, unit: TimeUnit) -> String {
    let days = (at / DAY).floor() as i64;
    let secs = (at - days as f64 * DAY).round() as i64;
    let (y, m, d) = civil(days);
    let (h, min, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    match unit {
        TimeUnit::Second => format!("{h:02}:{min:02}:{s:02}"),
        TimeUnit::Minute if secs == 0 => format!("{y:04}-{m:02}-{d:02}"),
        TimeUnit::Minute => format!("{h:02}:{min:02}"),
        TimeUnit::Day => format!("{y:04}-{m:02}-{d:02}"),
        TimeUnit::Month => format!("{y:04}-{m:02}"),
        TimeUnit::Year => format!("{y:04}"),
    }
}

/// A moment written whole (a tooltip's).
pub fn moment_label(at: f64) -> String {
    let days = (at / DAY).floor() as i64;
    let secs = (at - days as f64 * DAY).round() as i64;
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}

/// The civil date (year, month, day) of a day counted from 1970-01-01 (proleptic
/// Gregorian).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The day counted from 1970-01-01 of a civil date.
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
#[path = "../tests/unit/scale.rs"]
mod tests;
