//! The Task Manager's settings: `[tasks] history` (how long its graphs reach back, seconds:
//! 60, 300 or 900), `[tasks] interval` (how often it looks, seconds: 1, 2 or 5),
//! `[tasks] columns` (the table's columns the user pinned or hid: `cpu,+gpu,-threads`),
//! `[tasks] widths` (the widths the user dragged them to, logical pixels: `name:240,cpu:72`)
//! and `[tasks] detail_ratio` (the detail panel's share of the page, percent: 15–60). Declared
//! here, handed to the collector by [`apply`] at startup and after every change, shown in
//! the "Task Manager" block of the Appearance section ([`TaskSettings`]).

use std::time::Duration;

use i18n::t;
use settings_model::{Setting, SettingsService};
use ui_components::columns::Choice;
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// The history's lengths offered (seconds).
pub const HISTORIES: [i64; 3] = [60, 300, 900];
/// The intervals offered (seconds).
pub const INTERVALS: [i64; 3] = [1, 2, 5];

/// How long the graphs reach back (`[tasks] history`, seconds).
pub const HISTORY: Setting<i64> =
    Setting::new("tasks", "history", || 60).valid(|s| HISTORIES.contains(s));

/// How often it looks (`[tasks] interval`, seconds).
pub const INTERVAL: Setting<i64> =
    Setting::new("tasks", "interval", || 1).valid(|s| INTERVALS.contains(s));

/// The columns pinned (`+`) or hidden (`-`), by id (`[tasks] columns`; empty: by priority).
pub const COLUMNS: Setting<String> =
    Setting::new("tasks", "columns", String::new).valid(|s| columns_of(s).is_some());

/// The columns' widths the user dragged them to, by id (`[tasks] widths`, logical pixels;
/// empty: as their specs say).
pub const WIDTHS: Setting<String> =
    Setting::new("tasks", "widths", String::new).valid(|s| widths_of(s).is_some());

/// The widths a column may be given (logical pixels).
pub const WIDTH_RANGE: std::ops::RangeInclusive<f32> = 20.0..=4000.0;

/// The detail panel's share of the page (`[tasks] detail_ratio`, percent).
pub const DETAIL_RATIO: Setting<i64> =
    Setting::new("tasks", "detail_ratio", || 30).valid(|r| (15..=60).contains(r));

/// The table's columns, by id.
pub const COLUMN_IDS: [&str; 9] =
    ["cpu", "memory", "network", "disk", "project", "gpu", "trend", "threads", "detail"];

/// What the user said of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnChoice {
    /// As its priority says (it leaves when there's no room).
    Auto,
    /// Kept (it pushes others out).
    Pinned,
    /// Never shown.
    Hidden,
}

/// The choices `text` says (`cpu,+gpu,-threads`: each id at most once, known); `None`: it
/// can't be read.
pub fn columns_of(text: &str) -> Option<Vec<(&'static str, ColumnChoice)>> {
    let mut out: Vec<(&'static str, ColumnChoice)> = Vec::new();
    for item in text.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let (choice, id) = match item.as_bytes()[0] {
            b'+' => (ColumnChoice::Pinned, &item[1..]),
            b'-' => (ColumnChoice::Hidden, &item[1..]),
            _ => (ColumnChoice::Auto, item),
        };
        let id = COLUMN_IDS.into_iter().find(|known| *known == id)?;
        if out.iter().any(|(seen, _)| *seen == id) {
            return None;
        }
        out.push((id, choice));
    }
    Some(out)
}

/// The widths `text` says (`name:240,cpu:72`: each column known, at most once, its width in
/// [`WIDTH_RANGE`]), by [`crate::layout::COLUMNS`] (0: not said); `None`: it can't be read.
pub fn widths_of(text: &str) -> Option<[f32; crate::layout::COLUMNS.len()]> {
    let mut out = [0.0; crate::layout::COLUMNS.len()];
    for item in text.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let (id, width) = item.split_once(':')?;
        let column = crate::layout::ColumnId::from_id(id.trim())?;
        let width: f32 = width.trim().parse().ok()?;
        if !WIDTH_RANGE.contains(&width) || out[column.index()] > 0.0 {
            return None;
        }
        out[column.index()] = width.round();
    }
    Some(out)
}

/// The widths written as `[tasks] widths` says them (the ones not dragged, not written).
pub fn widths_text(widths: &[f32; crate::layout::COLUMNS.len()]) -> String {
    crate::layout::COLUMNS
        .into_iter()
        .filter_map(|c| {
            let w = widths[c.index()];
            WIDTH_RANGE.contains(&w).then(|| format!("{}:{}", c.id(), w.round() as i64))
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The settings' values handed to the collector and to the views.
pub fn apply(settings: &SettingsService) {
    let secs = |s: i64| Duration::from_secs(s.max(1) as u64);
    let (interval, history) = (settings.get(&INTERVAL), settings.get(&HISTORY));
    crate::collector::configure(secs(interval), secs(history));
    crate::set_preferences(crate::Preferences {
        choices: choices_of(&settings.get(&COLUMNS)),
        widths: widths_of(&settings.get(&WIDTHS)).unwrap_or_default(),
        ratio: settings.get(&DETAIL_RATIO) as f32 / 100.0,
        interval,
        history,
    });
}

/// The columns' choices `text` says, by [`crate::layout::COLUMNS`] (one not said: as its
/// priority says).
pub fn choices_of(text: &str) -> [Choice; crate::layout::COLUMNS.len()] {
    let mut out = [Choice::Auto; crate::layout::COLUMNS.len()];
    for (id, choice) in columns_of(text).unwrap_or_default() {
        if let Some(column) = crate::layout::ColumnId::from_id(id) {
            out[column.index()] = match choice {
                ColumnChoice::Auto => Choice::Auto,
                ColumnChoice::Pinned => Choice::Pinned,
                ColumnChoice::Hidden => Choice::Hidden,
            };
        }
    }
    out
}

/// The columns' choices written as `[tasks] columns` says them (`+gpu,-threads`; the ones
/// left as their priority says, not written).
pub fn columns_text(choices: &[Choice; crate::layout::COLUMNS.len()]) -> String {
    let mut out: Vec<String> = Vec::new();
    for column in crate::layout::COLUMNS {
        let mark = match choices[column.index()] {
            Choice::Auto => continue,
            Choice::Pinned => '+',
            Choice::Hidden => '-',
        };
        if COLUMN_IDS.contains(&column.id()) {
            out.push(format!("{mark}{}", column.id()));
        }
    }
    out.join(",")
}

/// A view's preferences written to the settings.
pub fn save_preferences(settings: &SettingsService, preferences: &crate::Preferences) {
    let columns = columns_text(&preferences.choices);
    if settings.get(&COLUMNS) != columns
        && let Err(e) = settings.set(&COLUMNS, columns)
    {
        tracing::warn!("can't save the Task Manager's columns: {e}");
    }
    let widths = widths_text(&preferences.widths);
    if settings.get(&WIDTHS) != widths
        && let Err(e) = settings.set(&WIDTHS, widths)
    {
        tracing::warn!("can't save the Task Manager's column widths: {e}");
    }
    let ratio = ((preferences.ratio * 100.0).round() as i64).clamp(15, 60);
    if settings.get(&DETAIL_RATIO) != ratio {
        save(settings, &DETAIL_RATIO, ratio);
    }
    if settings.get(&INTERVAL) != preferences.interval {
        save(settings, &INTERVAL, preferences.interval);
    }
    if settings.get(&HISTORY) != preferences.history {
        save(settings, &HISTORY, preferences.history);
    }
}

/// `value` saved (said in the log when it can't be).
fn save(settings: &SettingsService, setting: &Setting<i64>, value: i64) {
    if !setting.accepts(&value) {
        return;
    }
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!("can't save a Task Manager setting: {e}");
    }
}

/// The "Task Manager" block of the settings window (Appearance).
#[derive(Debug, Default)]
pub struct TaskSettings;

impl SettingsContribution for TaskSettings {
    fn id(&self) -> &'static str {
        "tasks"
    }

    fn page(&self) -> &'static str {
        "tasks"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "tasks",
            label: "settings.tasks_title",
            icon: "cpu",
            group: SettingsGroup::Tools,
            order: 30,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let histories = HISTORIES
            .into_iter()
            .map(|secs| (secs, i18n::tr!("settings.tasks_minutes", count = secs / 60)))
            .collect();
        let intervals = INTERVALS
            .into_iter()
            .map(|secs| (secs, i18n::tr!("settings.tasks_seconds", count = secs)))
            .collect();
        SettingsBlock {
            title: t("settings.tasks_title"),
            detail: t("settings.tasks_detail"),
            rows: vec![
                SettingsRow::Segmented {
                    key: "history",
                    detail: t("settings.tasks_history_detail"),
                    default: Some(HISTORY.default_value()),
                    label: t("settings.tasks_history"),
                    choices: histories,
                    current: s.get(&HISTORY),
                },
                SettingsRow::Segmented {
                    key: "interval",
                    detail: t("settings.tasks_interval_detail"),
                    default: Some(INTERVAL.default_value()),
                    label: t("settings.tasks_interval"),
                    choices: intervals,
                    current: s.get(&INTERVAL),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        match key {
            "history" => save(scope.settings, &HISTORY, value),
            "interval" => save(scope.settings, &INTERVAL, value),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
