//! The updates' settings (`[updates]`): `check` (looked for by itself: on by default),
//! `prereleases` (betas offered too: by default whoever runs a beta takes them), `skipped`
//! (the version the user chose to skip; a newer one is offered again). Declared here,
//! read into [`config`] at start and after every change, shown on their own page.

use std::sync::RwLock;

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// `[updates] check`: new versions looked for by themselves.
pub const CHECK: Setting<bool> = Setting::new("updates", "check", || true);
/// `[updates] prereleases`: betas offered too (by default when this one is a beta).
pub const PRERELEASES: Setting<bool> = Setting::new("updates", "prereleases", running_a_beta);
/// `[updates] skipped`: the version not to offer again (empty: none).
pub const SKIPPED: Setting<String> = Setting::new("updates", "skipped", String::new);

fn running_a_beta() -> bool {
    app_updates::Version::parse(kernel::brand::version()).is_some_and(|v| v.is_pre())
}

/// What the settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub check: bool,
    pub prereleases: bool,
    pub skipped: Option<app_updates::Version>,
}

impl Config {
    fn read(settings: &SettingsService) -> Self {
        Self {
            check: settings.get(&CHECK),
            prereleases: settings.get(&PRERELEASES),
            skipped: app_updates::Version::parse(&settings.get(&SKIPPED)),
        }
    }
}

static CONFIG: RwLock<Option<Config>> = RwLock::new(None);

/// What the settings say now (their defaults before they're read).
pub fn config() -> Config {
    let read = CONFIG.read().ok().and_then(|c| c.clone());
    read.unwrap_or_else(|| Config { check: true, prereleases: running_a_beta(), skipped: None })
}

/// The settings read again (at start, after a change).
pub fn apply(settings: &SettingsService) {
    if let Ok(mut config) = CONFIG.write() {
        *config = Some(Config::read(settings));
    }
}

/// `version` skipped from now on (saved, said in the log when it can't be).
pub fn skip(settings: &SettingsService, version: &app_updates::Version) {
    if let Err(e) = settings.set(&SKIPPED, version.to_string()) {
        tracing::warn!(error = %e, "the skipped version wasn't saved");
    }
    apply(settings);
}

/// The "Updates" page (Application).
#[derive(Debug, Default)]
pub struct UpdatesSettings;

impl SettingsContribution for UpdatesSettings {
    fn id(&self) -> &'static str {
        "updates"
    }

    fn page(&self) -> &'static str {
        "updates"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "updates",
            label: "updates.settings_title",
            icon: crate::ICON,
            group: SettingsGroup::Application,
            order: 90,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let mut notes =
            vec![i18n::tr!("updates.settings_version", version = kernel::brand::version())];
        let skipped = s.get(&SKIPPED);
        if !skipped.is_empty() {
            notes.push(i18n::tr!("updates.settings_skipped", version = skipped.as_str()));
        }
        SettingsBlock {
            title: t("updates.settings_title"),
            detail: i18n::tr!("updates.settings_detail", app = kernel::brand::NAME),
            rows: vec![
                SettingsRow::Toggle {
                    key: "check",
                    label: t("updates.check"),
                    detail: t("updates.check_detail"),
                    on: s.get(&CHECK),
                    default: Some(CHECK.default_value()),
                },
                SettingsRow::Toggle {
                    key: "prereleases",
                    label: t("updates.prereleases"),
                    detail: t("updates.prereleases_detail"),
                    on: s.get(&PRERELEASES),
                    default: Some(PRERELEASES.default_value()),
                },
                SettingsRow::Notes(notes),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        let saved = match key {
            "check" => s.set(&CHECK, value != 0),
            "prereleases" => s.set(&PRERELEASES, value != 0),
            _ => return,
        };
        if let Err(e) = saved {
            tracing::warn!(error = %e, "an updates setting wasn't saved");
        }
        apply(s);
    }
}
