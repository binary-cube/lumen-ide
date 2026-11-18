//! The data sources' settings (`[database] …` in `settings.toml`): how many rows a grid's
//! page shows, how long an unused session stays open, how many records a CSV file's
//! types are guessed from, and whether deleting rows is confirmed. Declared here, shown in
//! the settings window by [`DataSettings`] (in the editor's section), handed to the
//! module's parts by [`apply`] at startup and after every change — the open views follow
//! at once.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

/// Minutes an unused session stays open before it's closed (`[database] idle_close`; `0`:
/// never). Its next request opens it again.
pub const IDLE_CLOSE: Setting<i64> =
    Setting::new("database", "idle_close", || 10).valid(|m| (0..=24 * 60).contains(m));

/// The choices offered in the settings (minutes).
pub const IDLE_CHOICES: [i64; 5] = [0, 5, 10, 30, 60];

/// Rows a grid's page shows (`[database] page_size`; `0`: all of them, read as they're
/// scrolled to).
pub const PAGE_SIZE: Setting<i64> = Setting::new("database", "page_size", || 500)
    .valid(|n| *n == 0 || ui_components::data_grid::PAGE_SIZES.contains(&(*n as u32)));

/// The choices offered (`0`: all).
pub const PAGE_CHOICES: [i64; 5] = [10, 100, 500, 1000, 0];

/// Records a CSV file's column types are guessed from (`[database] csv_sample`).
pub const CSV_SAMPLE: Setting<i64> =
    Setting::new("database", "csv_sample", || 1000).valid(|n| (10..=100_000).contains(n));

/// The choices offered.
pub const CSV_SAMPLE_CHOICES: [i64; 4] = [100, 1000, 5000, 20_000];

/// Deleting rows asks to be confirmed in the preview before it's submitted
/// (`[database] confirm_delete`).
pub const CONFIRM_DELETE: Setting<bool> = Setting::new("database", "confirm_delete", || true);

/// The values in use; `u64::MAX` until configured (the settings' defaults then).
static MINUTES: AtomicU64 = AtomicU64::new(u64::MAX);
static PAGE: AtomicU64 = AtomicU64::new(u64::MAX);
static SAMPLE: AtomicU64 = AtomicU64::new(u64::MAX);
static CONFIRM: AtomicBool = AtomicBool::new(true);
/// Bumped at each change of the page size: the open grids take the new one.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Hands the idle minutes to the sessions (at startup and when it changes).
pub fn configure(minutes: i64) {
    MINUTES.store(minutes.max(0) as u64, Ordering::Relaxed);
}

/// How long an unused session stays open (`None`: never closed for that).
pub fn idle_close() -> Option<Duration> {
    let minutes = match MINUTES.load(Ordering::Relaxed) {
        u64::MAX => IDLE_CLOSE.default_value().max(0) as u64,
        minutes => minutes,
    };
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}

/// Hands the page size to the grids (`0`: all).
pub fn configure_page(rows: i64) {
    let rows = rows.max(0) as u64;
    if PAGE.swap(rows, Ordering::Relaxed) != rows {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// A grid's page size (`None`: all rows).
pub fn page_size() -> Option<u32> {
    let rows = match PAGE.load(Ordering::Relaxed) {
        u64::MAX => PAGE_SIZE.default_value().max(0) as u64,
        rows => rows,
    };
    (rows > 0).then(|| rows.min(u64::from(u32::MAX)) as u32)
}

/// Changes with each new page size (a grid compares it with the one it took).
pub fn page_generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

pub fn configure_sample(rows: i64) {
    SAMPLE.store(rows.max(1) as u64, Ordering::Relaxed);
}

/// Records a CSV file's types are guessed from.
pub fn csv_sample() -> u64 {
    match SAMPLE.load(Ordering::Relaxed) {
        u64::MAX => CSV_SAMPLE.default_value() as u64,
        rows => rows,
    }
}

pub fn configure_confirm(on: bool) {
    CONFIRM.store(on, Ordering::Relaxed);
}

/// Deleting rows is confirmed first.
pub fn confirm_delete() -> bool {
    CONFIRM.load(Ordering::Relaxed)
}

/// Every value from the saved settings (at startup and after a change): the open views
/// and the idle watch follow at once.
pub fn apply(settings: &SettingsService) {
    let before = idle_close();
    configure(settings.get(&IDLE_CLOSE));
    configure_page(settings.get(&PAGE_SIZE));
    configure_sample(settings.get(&CSV_SAMPLE));
    configure_confirm(settings.get(&CONFIRM_DELETE));
    // Closing turned on (or shortened) with sessions open: watched from now, not from
    // the next connection.
    if idle_close() != before && idle_close().is_some() {
        crate::sessions::rewatch();
    }
}

fn save<T: settings_model::SettingType>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!("can't save a database setting: {e}");
    }
}

/// The "Data" block of the settings window.
#[derive(Debug, Default)]
pub struct DataSettings;

impl SettingsContribution for DataSettings {
    fn id(&self) -> &'static str {
        "database"
    }

    fn page(&self) -> &'static str {
        "database"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::DATABASE)
    }

    fn order(&self) -> u16 {
        10
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let rows = |n: i64| if n == 0 { t("settings.database_page_all") } else { n.to_string() };
        let minutes = |n: i64| {
            if n == 0 {
                t("settings.database_idle_never")
            } else {
                i18n::tr!("settings.database_minutes", count = n)
            }
        };
        SettingsBlock {
            title: t("settings.database_title"),
            detail: t("settings.database_detail"),
            rows: vec![
                SettingsRow::Choices {
                    key: "page_size",
                    detail: t("settings.database_page_size_detail"),
                    default: Some(PAGE_SIZE.default_value()),
                    label: t("settings.database_page_size"),
                    choices: PAGE_CHOICES.iter().map(|&n| (n, rows(n))).collect(),
                    current: s.get(&PAGE_SIZE),
                },
                SettingsRow::Choices {
                    key: "idle_close",
                    detail: t("settings.database_idle_close_detail"),
                    default: Some(IDLE_CLOSE.default_value()),
                    label: t("settings.database_idle_close"),
                    choices: IDLE_CHOICES.iter().map(|&n| (n, minutes(n))).collect(),
                    current: s.get(&IDLE_CLOSE),
                },
                SettingsRow::Slider {
                    key: "csv_sample",
                    detail: t("settings.database_csv_sample_detail"),
                    default: Some(CSV_SAMPLE.default_value()),
                    label: t("settings.database_csv_sample"),
                    stops: CSV_SAMPLE_CHOICES.iter().map(|&n| (n, n.to_string())).collect(),
                    current: s.get(&CSV_SAMPLE),
                },
                SettingsRow::Toggle {
                    key: "confirm_delete",
                    detail: t("settings.database_confirm_delete_detail"),
                    default: Some(CONFIRM_DELETE.default_value()),
                    label: t("settings.database_confirm_delete"),
                    on: s.get(&CONFIRM_DELETE),
                },
                SettingsRow::Notes(vec![t("settings.database_note")]),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "page_size" => save(s, &PAGE_SIZE, value),
            "idle_close" => save(s, &IDLE_CLOSE, value),
            "csv_sample" => save(s, &CSV_SAMPLE, value),
            "confirm_delete" => save(s, &CONFIRM_DELETE, value != 0),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
