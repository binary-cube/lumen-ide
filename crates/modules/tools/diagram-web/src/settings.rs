//! The diagrams' own settings (Tools › Diagrams): whether a draw.io tab left alone falls asleep
//! (off by default: nothing is stopped unasked) and after how long. Told to the sleeping
//! (`ui_components::sleep::configure`) at startup and at each change.

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// Tools › Diagrams: the diagrams' page of the settings.
const PAGE: SettingsPage = SettingsPage {
    id: "diagrams",
    label: "settings.diagrams_section",
    icon: "file-pen",
    group: SettingsGroup::Tools,
    order: 30,
};

/// A draw.io tab left alone falls asleep: its page let go, its picture kept, brought back as
/// it was once woken.
pub const SLEEP_IDLE: Setting<bool> = Setting::new("diagram_web", "sleep_idle", || false);
/// After how many minutes untouched and quiet.
pub const SLEEP_AFTER: Setting<i64> =
    Setting::new("diagram_web", "sleep_after", || DEFAULT_SLEEP_AFTER)
        .valid(|n| SLEEP_AFTERS.contains(n));
/// The idle time it starts at (minutes): an hour.
const DEFAULT_SLEEP_AFTER: i64 = 60;
/// The idle times offered (minutes).
pub const SLEEP_AFTERS: [i64; 5] = [15, 30, 60, 120, 240];

/// Its idle time as the settings say now (`None`: its sleeping is off).
pub fn idle(settings: &SettingsService) -> Option<std::time::Duration> {
    let minutes = settings.get(&SLEEP_AFTER).max(SLEEP_AFTERS[0]);
    settings.get(&SLEEP_IDLE).then(|| std::time::Duration::from_secs(minutes as u64 * 60))
}

/// Its sleeping turned on (the hint's "Turn on") and told at once.
pub fn enable_sleep(settings: &SettingsService) {
    save(settings, &SLEEP_IDLE, true);
    told(settings);
}

/// The sleeping told what the settings say now.
fn told(settings: &SettingsService) {
    ui_components::sleep::configure(&crate::view::DIAGRAMS, idle(settings));
}

fn save<T: settings_model::SettingType>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!(error = %e, "can't save a diagram setting");
    }
}

/// An idle time offered, as shown: `15 min`, `1 h`.
fn sleep_label(minutes: i64) -> String {
    if minutes % 60 == 0 {
        i18n::tr!("settings.diagrams_sleep_hours", n = minutes / 60)
    } else {
        i18n::tr!("settings.diagrams_sleep_minutes", n = minutes)
    }
}

/// The "Sleeping diagrams" block.
#[derive(Debug)]
pub(crate) struct SleepSettings;

impl SettingsContribution for SleepSettings {
    fn id(&self) -> &'static str {
        "diagram-web-sleep"
    }

    fn page(&self) -> &'static str {
        PAGE.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(PAGE)
    }

    fn apply(&self, settings: &SettingsService) {
        told(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let mut rows = vec![SettingsRow::Toggle {
            key: "sleep_idle",
            detail: t("settings.diagrams_sleep_idle_detail"),
            default: Some(SLEEP_IDLE.default_value()),
            label: t("settings.diagrams_sleep_idle"),
            on: settings.get(&SLEEP_IDLE),
        }];
        if settings.get(&SLEEP_IDLE) {
            rows.push(SettingsRow::Dropdown {
                key: "sleep_after",
                label: t("settings.diagrams_sleep_after"),
                detail: t("settings.diagrams_sleep_after_detail"),
                icon: None,
                choices: SLEEP_AFTERS.iter().map(|&m| (m, sleep_label(m))).collect(),
                current: settings.get(&SLEEP_AFTER),
                default: Some(SLEEP_AFTER.default_value()),
            });
        }
        SettingsBlock {
            title: t("settings.diagrams_sleep"),
            detail: t("settings.diagrams_sleep_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        // Told to the sleeping by `apply`, as every change is.
        match key {
            "sleep_idle" => save(settings, &SLEEP_IDLE, value != 0),
            "sleep_after" if SLEEP_AFTERS.contains(&value) => save(settings, &SLEEP_AFTER, value),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
