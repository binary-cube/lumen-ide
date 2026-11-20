//! What a recording's page shows, made once on the worker that read it: its texts in the
//! user's language, its tables' rows, its charts' values per sample, the flame graph's
//! nodes, the summary to copy. The recording itself (every sample) is let go once this is
//! made: the tab holds only what it shows, and a frame only draws it.

use i18n::{t, tr};
use profiling::{Recording, Summary};
use ui_components::IcicleNode;
use ui_components::ds::Tone;
use ui_components::severity::Level;

use super::page::Cell;
use super::{flame, words};

/// A table as the page draws it: its title and hint, its columns (name, subtitle, share of
/// the width, numbers on the right), its rows.
pub struct Table {
    pub title: String,
    pub hint: String,
    pub cols: Vec<(String, String, f32, bool)>,
    pub rows: Vec<Vec<Cell>>,
}

/// A metric card's texts and its chart's series (by its place in [`Prepared::series`]).
pub struct CardText {
    pub label: String,
    pub value: String,
    pub sub: String,
    pub series: usize,
    pub tone: Tone,
}

/// A chart's values, per sample: its label, unit, tone, and the top of its scale.
pub struct Series {
    pub label: String,
    pub unit: &'static str,
    pub tone: Tone,
    pub values: Vec<f32>,
    pub top: f32,
}

/// The page, ready to draw.
pub struct Prepared {
    pub title: String,
    pub meta: String,
    pub findings: Vec<(Level, String, String)>,
    pub cards: Vec<CardText>,
    /// CPU, frames, memory, GPU, network, disk — the cards' and the timeline's.
    pub series: Vec<Series>,
    pub seconds: f64,
    pub before_flame: Vec<Table>,
    pub flame_depth: usize,
    pub flame_nodes: Vec<IcicleNode>,
    pub after_flame: Vec<Table>,
    /// What "Copy Summary" copies (Markdown).
    pub text: String,
}

/// The places of the series in [`Prepared::series`].
pub const CPU: usize = 0;
pub const FPS: usize = 1;
pub const MEMORY: usize = 2;
pub const GPU: usize = 3;
pub const NETWORK: usize = 4;
pub const DISK: usize = 5;

const MB: f32 = 1024.0 * 1024.0;

/// A table's columns from their keys (`profiler.<key>` and `<key>_sub`), shares and sides.
fn cols(keys: &[(&str, f32, bool)]) -> Vec<(String, String, f32, bool)> {
    keys.iter()
        .map(|(k, share, right)| {
            (t(&format!("profiler.{k}")), t(&format!("profiler.{k}_sub")), *share, *right)
        })
        .collect()
}

/// The largest of `values` (never zero: a share of it is never a division by zero).
fn largest(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(f64::MIN_POSITIVE, f64::max)
}

/// A recording's started moment (RFC 3339, UTC) in the user's zone, to the minute.
pub fn local(started: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(started).map_or_else(
        |_| started.to_owned(),
        |at| at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
    )
}

/// The page of `recording` summed up as `s`.
pub fn prepare(recording: &Recording, s: &Summary) -> Prepared {
    let per = |f: &dyn Fn(&profiling::Sample) -> f32| -> Vec<f32> {
        recording.samples.iter().map(f).collect()
    };
    let seconds_of = |s: &profiling::Sample| s.span_ms.max(1) as f32 / 1000.0;
    let series_of = |label: &str, unit: &'static str, tone: Tone, values: Vec<f32>, floor: f32| {
        let top = values.iter().copied().fold(floor, f32::max);
        Series { label: t(label), unit, tone, values, top }
    };
    let series = vec![
        series_of(
            "profiler.cpu",
            "%",
            Tone::Accent,
            per(&|s| s.cpu_us as f32 / (s.span_ms.max(1) as f32 * 10.0)),
            100.0,
        ),
        series_of(
            "profiler.fps",
            "",
            Tone::Success,
            per(&|s| s.frames as f32 / seconds_of(s)),
            60.0,
        ),
        series_of(
            "profiler.memory_peak",
            " MB",
            Tone::Warning,
            per(&|s| s.memory as f32 / MB),
            1.0,
        ),
        series_of(
            "profiler.gpu_peak",
            " MB",
            Tone::Accent,
            per(&|s| s.gpu.unwrap_or(0) as f32 / MB),
            1.0,
        ),
        series_of(
            "profiler.network",
            " MB/s",
            Tone::Warning,
            per(&|s| {
                let bytes =
                    s.io.iter()
                        .map(|i| i.sent.saturating_add(i.received))
                        .fold(0, u64::saturating_add);
                bytes as f32 / MB / seconds_of(s)
            }),
            0.1,
        ),
        series_of(
            "profiler.disk",
            " MB/s",
            Tone::Danger,
            per(&|s| s.disk_read.saturating_add(s.disk_written) as f32 / MB / seconds_of(s)),
            0.1,
        ),
    ];
    let cards = vec![
        CardText {
            label: t("profiler.cpu"),
            value: format!("{:.1}%", s.cpu),
            sub: t("profiler.cpu_sub"),
            series: CPU,
            tone: Tone::Accent,
        },
        CardText {
            label: t("profiler.fps"),
            value: format!("{:.1}", s.fps),
            sub: tr!("profiler.fps_sub", ms = format!("{:.1}", s.frame_longest_ms).as_str()),
            series: FPS,
            tone: Tone::Success,
        },
        CardText {
            label: t("profiler.memory_peak"),
            value: i18n::bytes(s.memory_peak),
            sub: String::new(),
            series: MEMORY,
            tone: Tone::Warning,
        },
        CardText {
            label: t("profiler.gpu_peak"),
            value: s.gpu_peak.map_or_else(|| "—".to_owned(), i18n::bytes),
            sub: String::new(),
            series: GPU,
            tone: Tone::Accent,
        },
    ];
    let top = largest(s.modules.iter().map(|m| m.1));
    let modules = Table {
        title: t("profiler.modules"),
        hint: t("profiler.modules_hint"),
        cols: cols(&[("col_module", 0.4, false), ("col_cpu", 0.6, false)]),
        rows: s
            .modules
            .iter()
            .map(|(m, c)| {
                vec![
                    Cell::Text(m.clone()),
                    Cell::Bar((*c / top) as f32, format!("{c:.1}%"), Tone::Accent),
                ]
            })
            .collect(),
    };
    let top = largest(s.triggers.iter().map(|t| t.2));
    let triggers = Table {
        title: t("profiler.triggers"),
        hint: t("profiler.triggers_hint"),
        cols: cols(&[("col_who", 0.3, false), ("col_why", 0.2, false), ("col_rate", 0.5, false)]),
        rows: s
            .triggers
            .iter()
            .map(|(o, c, n)| {
                vec![
                    Cell::Text(o.clone()),
                    Cell::Muted(words::cause(c)),
                    Cell::Bar((*n / top) as f32, format!("{n:.1} / s"), Tone::Warning),
                ]
            })
            .collect(),
    };
    let top = largest(s.busy.iter().map(|b| b.2));
    let busy = Table {
        title: t("profiler.busy"),
        hint: t("profiler.busy_hint"),
        cols: cols(&[
            ("col_part", 0.26, false),
            ("col_does", 0.12, false),
            ("col_share", 0.38, false),
            ("col_calls", 0.1, true),
            ("col_longest", 0.14, true),
        ]),
        rows: s
            .busy
            .iter()
            .map(|(o, w, share, calls, longest)| {
                vec![
                    Cell::Text(o.clone()),
                    Cell::Muted(w.clone()),
                    Cell::Bar((*share / top) as f32, format!("{share:.1}%"), Tone::Danger),
                    Cell::Text(calls.to_string()),
                    Cell::Text(format!("{longest:.1} ms")),
                ]
            })
            .collect(),
    };
    let moved = |i: &profiling::Io| {
        i.sent.saturating_add(i.received).saturating_add(i.read).saturating_add(i.written)
    };
    let top = largest(s.io.iter().map(|i| moved(i) as f64));
    let disk = format!(
        "{} {} · {} {}",
        t("profiler.read"),
        i18n::bytes(s.disk_read),
        t("profiler.written"),
        i18n::bytes(s.disk_written)
    );
    let io = Table {
        title: t("profiler.io"),
        hint: tr!("profiler.io_hint", disk = disk.as_str()),
        cols: cols(&[
            ("col_module", 0.24, false),
            ("col_moved", 0.28, false),
            ("col_sent", 0.12, true),
            ("col_received", 0.12, true),
            ("col_read", 0.12, true),
            ("col_written", 0.12, true),
        ]),
        rows: s
            .io
            .iter()
            .map(|i| {
                vec![
                    Cell::Text(i.owner.clone()),
                    Cell::Bar((moved(i) as f64 / top) as f32, i18n::bytes(moved(i)), Tone::Warning),
                    Cell::Text(i18n::bytes(i.sent)),
                    Cell::Text(i18n::bytes(i.received)),
                    Cell::Text(i18n::bytes(i.read)),
                    Cell::Text(i18n::bytes(i.written)),
                ]
            })
            .collect(),
    };
    let top = largest(s.threads.iter().map(|t| t.2));
    let threads = Table {
        title: t("profiler.threads"),
        hint: String::new(),
        cols: cols(&[
            ("col_thread", 0.3, false),
            ("col_whose", 0.2, false),
            ("col_cpu", 0.5, false),
        ]),
        rows: s
            .threads
            .iter()
            .map(|(name, owner, c)| {
                vec![
                    Cell::Text(name.clone()),
                    Cell::Muted(owner.clone().unwrap_or_else(|| "—".to_owned())),
                    Cell::Bar((*c / top) as f32, format!("{c:.1}%"), Tone::Accent),
                ]
            })
            .collect(),
    };
    let mut after_flame = vec![io, threads];
    if !s.counters.is_empty() || !s.levels.is_empty() {
        let mut rows: Vec<Vec<Cell>> = s
            .counters
            .iter()
            .map(|(o, w, unit, rate, total)| {
                vec![
                    Cell::Text(o.clone()),
                    Cell::Muted(w.clone()),
                    Cell::Text(words::rate(*rate, unit)),
                    Cell::Text(words::amount(*total, unit)),
                ]
            })
            .collect();
        rows.extend(s.levels.iter().map(|(o, w, peak, last)| {
            vec![
                Cell::Text(o.clone()),
                Cell::Muted(w.clone()),
                Cell::Text(format!("≤ {peak}")),
                Cell::Text(last.to_string()),
            ]
        }));
        after_flame.push(Table {
            title: t("profiler.probes"),
            hint: t("profiler.probes_hint"),
            cols: cols(&[
                ("col_part", 0.3, false),
                ("col_what", 0.3, false),
                ("col_value", 0.2, true),
                ("col_total", 0.2, true),
            ]),
            rows,
        });
    }
    Prepared {
        title: t("profiler.view_title"),
        meta: format!(
            "{} · {} · {}",
            local(&recording.started),
            words::length(s.seconds),
            words::ran_on(recording)
        ),
        findings: s
            .findings
            .iter()
            .map(|f| (words::level(f), words::finding(f), words::advice(f)))
            .collect(),
        cards,
        series,
        seconds: s.seconds,
        before_flame: vec![modules, triggers, busy],
        flame_depth: s.spans.iter().map(|w| w.0.len()).max().unwrap_or(0),
        flame_nodes: flame::nodes(s),
        after_flame,
        text: words::as_text(recording, s),
    }
}
