//! The charts' settings (`[charts]` in `settings.toml`): how many of a result's rows a chart
//! has its host fetch when asked for all of them. Shown on a page of their own (Tools ›
//! Charts, gone with the module); handed to the panes by [`apply`] at startup and after a
//! change.

use std::sync::atomic::{AtomicU64, Ordering};

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

/// The most rows fetched for a chart (`[charts] fetch_rows`).
pub const FETCH_ROWS: Setting<i64> =
    Setting::new("charts", "fetch_rows", || 200_000).valid(|n| (1_000..=2_000_000).contains(n));

/// The choices offered.
pub const FETCH_CHOICES: [i64; 5] = [10_000, 50_000, 200_000, 500_000, 1_000_000];

/// [`FETCH_ROWS`] as applied.
static FETCH: AtomicU64 = AtomicU64::new(u64::MAX);

pub fn configure_fetch_rows(rows: i64) {
    FETCH.store(rows.clamp(1_000, 2_000_000) as u64, Ordering::Relaxed);
}

/// The most rows fetched for a chart.
pub fn fetch_rows() -> usize {
    match FETCH.load(Ordering::Relaxed) {
        u64::MAX => FETCH_ROWS.default_value() as usize,
        rows => rows as usize,
    }
}

/// Every value from the saved settings.
pub fn apply(settings: &SettingsService) {
    configure_fetch_rows(settings.get(&FETCH_ROWS));
}

/// A count with its thousands apart (`200 000`).
pub fn grouped(n: i64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

/// The charts' own settings page, after the databases'.
pub const PAGE: SettingsPage = SettingsPage {
    id: "charts",
    label: "settings.charts_title",
    icon: "chart",
    group: workbench_model::SettingsGroup::Tools,
    order: 25,
};

/// The charts' settings block.
#[derive(Debug, Default)]
pub struct ChartSettings;

impl SettingsContribution for ChartSettings {
    fn id(&self) -> &'static str {
        "charts"
    }

    fn page(&self) -> &'static str {
        PAGE.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(PAGE)
    }

    fn order(&self) -> u16 {
        40
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        SettingsBlock {
            title: t("settings.charts_title"),
            detail: t("settings.charts_detail"),
            rows: vec![SettingsRow::Choices {
                key: "charts_fetch_rows",
                detail: t("settings.charts_fetch_rows_detail"),
                default: Some(FETCH_ROWS.default_value()),
                label: t("settings.charts_fetch_rows"),
                choices: FETCH_CHOICES.iter().map(|&n| (n, grouped(n))).collect(),
                current: scope.settings.get(&FETCH_ROWS),
            }],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key == "charts_fetch_rows"
            && let Err(e) = scope.settings.set(&FETCH_ROWS, value)
        {
            tracing::warn!(error = %e, "can't save a chart setting");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
