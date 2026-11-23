//! The view's texts, written once per look (or change of what it shows), never while
//! drawing: its labels in the user's language, the cards' values and lines, the status
//! line, the selected part's figures, the widest text of each column (so the numbers don't
//! dance) and the toolbar's widths (for the layout).

use i18n::{t, tr};
use text_layout::TextSystem;
use ui_components::charts::spark::scale_for;
use ui_components::ds::{self, Ctx};

use super::{Card, TaskManager};
use crate::collector::{Frame, History, Metric, Ring};
use crate::layout::{COLUMNS, ColumnId, ToolWidths};
use crate::model::{Column, Item, ViewModel, percent, rate};

/// The labels, in the user's language.
#[derive(Debug, Default)]
pub(crate) struct Labels {
    pub scope: [String; 2],
    /// By kind, by project, a list.
    pub grouping: [String; 3],
    pub filter: String,
    pub filter_open: String,
    pub filter_clear: String,
    pub pause: String,
    pub resume: String,
    pub paused: String,
    pub more: String,
    /// The columns' headers and what they measure, by [`COLUMNS`].
    pub columns: [String; COLUMNS.len()],
    /// Under each header, muted: what it counts in (by [`COLUMNS`]).
    pub subtitles: [String; COLUMNS.len()],
    pub tips: [String; COLUMNS.len()],
    /// The cards' labels, by [`Card`].
    pub cards: [String; 4],
    pub network: String,
    pub disk: String,
    pub collecting: String,
    pub nothing_matches: String,
    pub clear_filter: String,
    pub columns_tip: String,
    pub no_room: String,
    pub pinned: String,
    pub columns_reset: String,
    /// The popovers' headings: how often, how far back, sort by, the columns.
    pub headings: [String; 4],
    /// The intervals and histories offered.
    pub intervals: [String; 3],
    pub histories: [String; 3],
    pub back: String,
    pub close: String,
    pub running: String,
    pub paused_state: String,
    pub gone_state: String,
    pub gone: String,
    pub avg: String,
    pub max: String,
    pub threads: String,
    pub programs: String,
    pub operations: String,
    pub no_threads: String,
    /// A thread the system gave no name (written in its place, muted).
    pub thread_unnamed: String,
    /// The actions' labels, by [`super::actions::kind_index`].
    pub actions: [String; 7],
}

impl Labels {
    fn new() -> Self {
        let col = |c: ColumnId| match c {
            ColumnId::Name => t("tasks.col_name"),
            ColumnId::Cpu => t("tasks.cpu"),
            ColumnId::Memory => t("tasks.memory"),
            ColumnId::Network => t("tasks.network"),
            ColumnId::Disk => t("tasks.disk"),
            ColumnId::Project => t("tasks.col_project"),
            ColumnId::Gpu => t("tasks.gpu"),
            ColumnId::Trend => t("tasks.col_trend"),
            ColumnId::Threads => t("tasks.col_threads"),
            ColumnId::Detail => t("tasks.col_detail"),
        };
        let tip = |c: ColumnId| t(&format!("tasks.tip_{}", c.id()));
        let sub = |c: ColumnId| t(&format!("tasks.sub_{}", c.id()));
        Self {
            scope: [t("tasks.scope_this"), t("tasks.scope_all")],
            grouping: [
                t("tasks.grouping_kind"),
                t("tasks.grouping_project"),
                t("tasks.grouping_list"),
            ],
            filter: t("tasks.filter"),
            filter_open: t("tasks.filter_open"),
            filter_clear: t("tasks.filter_clear"),
            pause: t("tasks.pause"),
            resume: t("tasks.resume"),
            paused: t("tasks.paused"),
            more: t("tasks.more"),
            columns: COLUMNS.map(col),
            subtitles: COLUMNS.map(sub),
            tips: COLUMNS.map(tip),
            cards: [t("tasks.cpu"), t("tasks.memory"), t("tasks.gpu"), t("tasks.card_io")],
            network: t("tasks.network"),
            disk: t("tasks.disk"),
            collecting: t("tasks.collecting"),
            nothing_matches: t("tasks.nothing_matches"),
            clear_filter: t("tasks.clear_filter"),
            columns_tip: t("tasks.columns_tip"),
            no_room: t("tasks.column_no_room"),
            pinned: t("tasks.column_pinned"),
            columns_reset: t("tasks.columns_reset"),
            headings: [
                t("tasks.interval_heading"),
                t("tasks.history_heading"),
                t("tasks.sort_by"),
                t("tasks.columns_title"),
            ],
            intervals: crate::settings::INTERVALS.map(|s| tr!("settings.tasks_seconds", count = s)),
            histories: crate::settings::HISTORIES
                .map(|s| tr!("settings.tasks_minutes", count = s / 60)),
            back: t("tasks.back"),
            close: t("tasks.close_detail"),
            running: t("tasks.state_running"),
            paused_state: t("tasks.state_paused"),
            gone_state: t("tasks.state_gone"),
            gone: t("tasks.gone"),
            avg: t("tasks.stat_avg"),
            max: t("tasks.stat_max"),
            threads: t("tasks.threads_title"),
            programs: t("tasks.programs_title"),
            operations: t("tasks.operations"),
            no_threads: t("tasks.no_threads"),
            thread_unnamed: t("tasks.thread_unnamed"),
            actions: [
                t("tasks.action_stop"),
                t("tasks.action_close"),
                t("tasks.action_restart"),
                t("tasks.action_release"),
                t("tasks.action_disconnect"),
                t("tasks.action_show_log"),
                t("tasks.action_settings"),
            ],
        }
    }

    /// The header of column `column`.
    pub fn column(&self, column: ColumnId) -> &str {
        &self.columns[column.index()]
    }

    pub fn subtitle(&self, column: ColumnId) -> &str {
        &self.subtitles[column.index()]
    }
}

/// A card's texts.
#[derive(Debug, Default, Clone)]
pub(crate) struct CardText {
    pub value: String,
    pub sub: String,
    /// A threshold passed (its line warns, with a sign).
    pub warning: bool,
    /// Its values over the history: min, avg, max.
    pub tip: String,
}

/// A figure of the selected part: now, on average, at most.
#[derive(Debug, Default, Clone)]
pub(crate) struct Stat {
    pub metric: Option<Metric>,
    pub label: String,
    pub now: String,
    pub avg: String,
    pub max: String,
}

/// The selected part's texts.
#[derive(Debug, Default)]
pub(crate) struct DetailTexts {
    /// The metrics it reports.
    pub stats: Vec<Stat>,
    /// Each chart's scale top, by [`Metric::ALL`].
    pub scales: [f32; 5],
    /// Owner · project · group.
    pub owner: String,
    pub group: String,
    /// Its threads with their CPU (`indexer  9.1%`).
    pub threads: Vec<(String, String)>,
    /// The programs it started.
    pub programs: Vec<crate::model::ProcessTexts>,
}

/// Everything written for the last look.
#[derive(Debug, Default)]
pub(crate) struct Texts {
    stamp: Option<(u64, u64, u64, u32, bool, u64)>,
    pub labels: Labels,
    pub status: String,
    /// The minimal page's summary line.
    pub line: String,
    pub cards: [CardText; 4],
    /// How many the filter finds.
    pub matches: String,
    pub interval_pill: String,
    pub sort_pill: String,
    /// Each column's widest text (physical pixels), by [`COLUMNS`].
    pub widths: [f32; COLUMNS.len()],
    pub tools: ToolWidths,
    pub detail: DetailTexts,
    /// The cards' scale tops, by [`Metric::ALL`].
    pub scales: [f32; 5],
}

/// A metric's value written (percent, bytes, a rate).
pub(crate) fn value(metric: Metric, v: f32) -> String {
    match metric {
        Metric::Cpu => percent(v),
        Metric::Memory | Metric::Gpu => i18n::bytes(v.max(0.0) as u64),
        Metric::Net | Metric::Disk => rate(v),
    }
}

/// The lowest a chart's scale goes for `metric` (an idle series stays low).
pub(crate) fn floor(metric: Metric) -> f32 {
    match metric {
        Metric::Cpu => 5.0,
        Metric::Memory | Metric::Gpu => 1024.0 * 1024.0,
        Metric::Net | Metric::Disk => 4096.0,
    }
}

/// A ring's least, average and highest values.
fn spread(ring: &Ring) -> Option<(f32, f32, f32)> {
    if ring.is_empty() {
        return None;
    }
    let (mut lo, mut hi, mut sum) = (f32::MAX, 0.0_f32, 0.0);
    for v in ring.iter() {
        lo = lo.min(v);
        hi = hi.max(v);
        sum += v;
    }
    Some((lo, sum / ring.len() as f32, hi))
}

/// CPU over 80% of a core for the last 10 values.
fn cpu_hot(ring: &Ring) -> bool {
    ring.len() >= 10 && ring.iter().skip(ring.len() - 10).all(|v| v > 80.0)
}

/// Memory growing all along its last 60 values.
fn memory_growing(ring: &Ring) -> bool {
    if ring.len() < 60 {
        return false;
    }
    let values: Vec<f32> = ring.iter().skip(ring.len() - 60).collect();
    values.windows(2).all(|w| w[1] >= w[0]) && values[59] > values[0]
}

impl TaskManager {
    /// The texts written again when the look, the state, the history or the scale changed.
    pub(super) fn refresh_texts(&mut self, text: &mut TextSystem, ds: &Ctx<'_>, model: &ViewModel) {
        let history_generation = self.with_history(History::generation).unwrap_or(0);
        let stamp = Some((
            model.stamp.0,
            model.stamp.1,
            history_generation,
            ds.scale.to_bits(),
            self.paused,
            self.selection_epoch,
        ));
        if self.texts.stamp == stamp {
            return;
        }
        self.texts.stamp = stamp;
        self.texts.labels = Labels::new();
        let frame = Arc::clone(&model.frame);
        self.write_summary(&frame);
        self.write_detail();
        self.measure(text, ds, model);
    }

    fn write_summary(&mut self, frame: &Frame) {
        let preferences = super::preferences();
        let every = tr!("tasks.every", count = preferences.interval);
        let reach = tr!("tasks.reach", count = (preferences.history / 60).max(1));
        self.texts.status =
            tr!("tasks.status", threads = frame.threads, interval = every, history = reach);
        self.texts.interval_pill = tr!(
            "tasks.interval_pill",
            interval = preferences.interval,
            history = (preferences.history / 60).max(1)
        );
        let memory = i18n::bytes(frame.memory);
        self.texts.line =
            tr!("tasks.summary_line", cpu = percent(frame.cpu), memory = memory.clone());
        let sorted = super::column_of(self.state.sort.0);
        self.texts.sort_pill =
            tr!("tasks.sort_pill", column = self.texts.labels.column(sorted).to_owned());
        self.texts.matches = if self.state.filter.trim().is_empty() {
            String::new()
        } else {
            self.model.shown.len().to_string()
        };
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let unattributed = frame
            .lines
            .iter()
            .rev()
            .find(|l| l.owner == workbench_model::resources::APP && l.threads == 0 && l.cpu == 0.0)
            .and_then(|l| l.memory)
            .unwrap_or(0);
        let tip = |ring: Option<&Ring>, metric: Metric| match ring.and_then(spread) {
            Some((lo, avg, hi)) => tr!(
                "tasks.card_tip",
                min = value(metric, lo),
                avg = value(metric, avg),
                max = value(metric, hi)
            ),
            None => String::new(),
        };
        let (cards, scales) = self
            .with_history(|h| {
                let total = h.total();
                let ring = |m: Metric| total.get(m);
                let peak = |m: Metric| ring(m).iter().fold(0.0_f32, f32::max);
                let cpu_max = percent(peak(Metric::Cpu));
                let cards = [
                    CardText {
                        value: percent(frame.cpu),
                        sub: tr!("tasks.cpu_sub", cores = cores, max = cpu_max),
                        warning: cpu_hot(ring(Metric::Cpu)),
                        tip: tip(Some(ring(Metric::Cpu)), Metric::Cpu),
                    },
                    CardText {
                        value: memory.clone(),
                        sub: tr!("tasks.memory_sub", memory = i18n::bytes(unattributed)),
                        warning: memory_growing(ring(Metric::Memory)),
                        tip: tip(Some(ring(Metric::Memory)), Metric::Memory),
                    },
                    CardText {
                        value: frame.gpu.map(i18n::bytes).unwrap_or_default(),
                        sub: t("tasks.gpu_sub"),
                        warning: false,
                        tip: tip(Some(ring(Metric::Gpu)), Metric::Gpu),
                    },
                    CardText {
                        value: format!("↓↑ {}", rate(frame.net)),
                        sub: tr!("tasks.io_sub", disk = rate(frame.disk.unwrap_or(0.0))),
                        warning: false,
                        tip: tip(Some(ring(Metric::Net)), Metric::Net),
                    },
                ];
                let scales = Metric::ALL.map(|m| scale_for(peak(m), floor(m)));
                (cards, scales)
            })
            .unwrap_or_else(|| {
                let cards = [
                    CardText { value: percent(frame.cpu), ..CardText::default() },
                    CardText { value: memory.clone(), ..CardText::default() },
                    CardText {
                        value: frame.gpu.map(i18n::bytes).unwrap_or_default(),
                        ..CardText::default()
                    },
                    CardText {
                        value: format!("↓↑ {}", rate(frame.net)), ..CardText::default()
                    },
                ];
                (cards, Metric::ALL.map(floor))
            });
        self.texts.cards = cards;
        self.texts.scales = scales;
    }

    /// The selected part's figures (the metrics it reports) and its charts' scales.
    fn write_detail(&mut self) {
        let Some(selected) = self.selection() else {
            self.texts.detail = DetailTexts::default();
            return;
        };
        let line = &selected.line;
        let labels = &self.texts.labels;
        let reports = [
            (Metric::Cpu, line.threads > 0 || line.cpu > 0.0, &labels.cards[0]),
            (Metric::Memory, line.memory.is_some(), &labels.cards[1]),
            (Metric::Gpu, line.gpu.is_some(), &labels.cards[2]),
            (Metric::Net, line.net.is_some(), &labels.network),
            (Metric::Disk, line.disk.is_some(), &labels.disk),
        ];
        let now_of = |m: Metric| match m {
            Metric::Cpu => line.cpu,
            Metric::Memory => line.memory.unwrap_or(0) as f32,
            Metric::Gpu => line.gpu.unwrap_or(0) as f32,
            Metric::Net => line.net.unwrap_or(0.0),
            Metric::Disk => line.disk.unwrap_or(0.0),
        };
        let key = line.key.clone();
        let (stats, scales) = self
            .with_history(|h| {
                let trace = h.part(&key);
                let mut stats = Vec::new();
                let mut scales = Metric::ALL.map(floor);
                for (metric, reported, label) in reports {
                    let spread = trace.and_then(|t| spread(t.get(metric)));
                    if let Some((_, _, hi)) = spread {
                        scales[metric as usize] = scale_for(hi, floor(metric));
                    }
                    // Reported once (its history has values): kept, at 0 while it costs
                    // nothing, so its figure and chart don't come and go.
                    if !reported && spread.is_none() {
                        continue;
                    }
                    let (_, avg, hi) = spread.unwrap_or((0.0, now_of(metric), now_of(metric)));
                    stats.push(Stat {
                        metric: Some(metric),
                        label: label.clone(),
                        now: value(metric, now_of(metric)),
                        avg: value(metric, avg),
                        max: value(metric, hi),
                    });
                }
                (stats, scales)
            })
            .unwrap_or_default();
        let project = line
            .project
            .and_then(|id| self.frame.projects.iter().find(|p| p.id == id))
            .map(|p| p.name.clone());
        let owner = match project {
            Some(project) => format!("{} · {project}", line.owner),
            None => line.owner.clone(),
        };
        let mut threads: Vec<(String, String)> =
            line.thread_cpu.iter().map(|(n, c)| (n.clone(), percent(*c))).collect();
        // By name, the unnamed ones last.
        threads.sort_by(|a, b| (a.0.trim().is_empty(), &a.0).cmp(&(b.0.trim().is_empty(), &b.0)));
        self.texts.detail = DetailTexts {
            stats,
            scales,
            owner,
            group: crate::model::groups::group_name(line.group),
            threads,
            programs: line.processes.iter().map(crate::model::ProcessTexts::of).collect(),
        };
    }

    /// The columns' widest texts and the toolbar's widths.
    fn measure(&mut self, text: &mut TextSystem, ds: &Ctx<'_>, model: &ViewModel) {
        let font = ds.code_font(-1.0);
        let mut widths = [0.0_f32; COLUMNS.len()];
        let mut widest = |column: ColumnId, value: Option<&str>| {
            if let Some(value) = value {
                let w = text.shape_line(value, &font).width.ceil();
                widths[column.index()] = widths[column.index()].max(w);
            }
        };
        for item in &model.items {
            match item {
                Item::Part { texts, .. } => {
                    widest(ColumnId::Cpu, texts.cpu.as_deref());
                    widest(ColumnId::Memory, texts.memory.as_deref());
                    widest(ColumnId::Network, texts.net.as_deref());
                    widest(ColumnId::Disk, texts.disk.as_deref());
                    widest(ColumnId::Gpu, texts.gpu.as_deref());
                    widest(ColumnId::Threads, texts.threads.as_deref());
                }
                Item::Group { cpu_text, memory_text, .. } => {
                    widest(ColumnId::Cpu, Some(cpu_text));
                    widest(ColumnId::Memory, memory_text.as_deref());
                }
                Item::Process { .. } | Item::Operation(_) => {}
            }
        }
        // The headers too (with the sorting arrow).
        let head = ds.ui_font(-1.5).with_weight(text_layout::Font::SEMIBOLD);
        for column in COLUMNS.into_iter().filter(|c| c.numeric()) {
            let w = text.shape_line(self.texts.labels.column(column), &head).width + ds.px(12.0);
            widths[column.index()] = widths[column.index()].max(w.ceil());
        }
        self.texts.widths = widths;
        let labels = &self.texts.labels;
        let scope: [&str; 2] = [&labels.scope[0], &labels.scope[1]];
        let grouping: Vec<&str> = self.grouping_labels();
        let pill = |text: &mut TextSystem, label: &str| {
            (text.shape_line(label, &ds.ui_font(-1.0)).width + ds.px(30.0)).round()
        };
        self.texts.tools = ToolWidths {
            scope: ds::segmented_width(text, ds, &scope),
            grouping: ds::segmented_width(text, ds, &grouping),
            interval: pill(text, &self.texts.interval_pill),
            sort: pill(text, &self.texts.sort_pill),
        };
    }

    /// The grouping's labels offered (by project only in every project's view).
    pub(super) fn grouping_labels(&self) -> Vec<&str> {
        let g = &self.texts.labels.grouping;
        if self.all { vec![&g[0], &g[1], &g[2]] } else { vec![&g[0], &g[2]] }
    }

    /// Whether the table sorts by `card`'s metric.
    pub(super) fn sorts_by(&self, card: Card) -> bool {
        self.state.sort.0 == card.column()
            || (card == Card::Io && self.state.sort.0 == Column::Disk)
    }
}

use std::sync::Arc;
