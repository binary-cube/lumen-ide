//! The web engine's settings (`[web_engine]`): `check` (a newer engine looked for and
//! downloaded by itself: on by default), `declined` (the offer refused: not made again).
//! Declared here, read into [`config`] at start and after every change, shown on their own
//! page with where the engine stands and what can be done with it (drawn again whenever
//! the install changes while it's open).

use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use i18n::{t, tr};
use settings_model::{Setting, SettingsService};
use web_engine::install::service::{self, Status};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// `[web_engine] check`: a newer engine looked for and downloaded by itself.
pub const CHECK: Setting<bool> = Setting::new("web_engine", "check", || true);
/// `[web_engine] declined`: the offer refused (not made again; the download stays here).
pub const DECLINED: Setting<bool> = Setting::new("web_engine", "declined", || false);

/// What the settings say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub check: bool,
    pub declined: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { check: CHECK.default_value(), declined: DECLINED.default_value() }
    }
}

static CONFIG: RwLock<Option<Config>> = RwLock::new(None);

/// What the settings say now (their defaults before they're read).
pub fn config() -> Config {
    CONFIG.read().ok().and_then(|c| *c).unwrap_or_default()
}

/// The settings read again (at start, after a change).
pub fn apply(settings: &SettingsService) {
    let read = Config { check: settings.get(&CHECK), declined: settings.get(&DECLINED) };
    if let Ok(mut config) = CONFIG.write() {
        *config = Some(read);
    }
}

/// The offer refused from now on: `true` once saved (said in the log when it can't be).
pub fn decline(settings: &SettingsService) -> bool {
    let saved = match settings.set(&DECLINED, true) {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, "the web engine's offer refused wasn't saved");
            false
        }
    };
    apply(settings);
    saved
}

/// The page's buttons (the actions [`SettingsContribution::press`] gets).
const DOWNLOAD: &str = "download";
const CHECK_NOW: &str = "check";
const REMOVE: &str = "remove";
const STOP: &str = "stop";
/// The toggle's row (what [`SettingsContribution::choose`] gets for it).
const CHECK_ROW: &str = "check";

/// The "Web Engine" page (Application).
#[derive(Debug, Default)]
pub struct WebEngineSettings {
    /// The install's state the page was last drawn from ([`service::generation`]).
    drawn: AtomicU64,
}

impl SettingsContribution for WebEngineSettings {
    fn id(&self) -> &'static str {
        "web_engine"
    }

    fn page(&self) -> &'static str {
        "web_engine"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "web_engine",
            label: "web_engine.settings_title",
            icon: crate::ICON,
            group: SettingsGroup::Application,
            // Right after the modules: what pages of the modules need.
            order: 25,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        // Taken before what it's drawn from: a change meanwhile draws it again.
        self.drawn.store(service::generation(), Ordering::Relaxed);
        let (text, buttons) = match service::status() {
            // Looking for a newer one: nothing to start or remove until it's done.
            Status::Installed { version, .. } if service::updating() => {
                (tr!("web_engine.settings_checking", version = version.as_str()), Vec::new())
            }
            Status::Installed { version, .. } => (
                tr!("web_engine.settings_installed", version = version.as_str()),
                vec![(CHECK_NOW, t("web_engine.check_now")), (REMOVE, t("web_engine.remove"))],
            ),
            Status::Downloading { received, total } => {
                (crate::downloaded(received, total), vec![(STOP, t("web_engine.stop"))])
            }
            Status::Unpacking => (t("web_engine.unpacking"), Vec::new()),
            Status::Looking => (t("web_engine.settings_looking"), Vec::new()),
            Status::Failed(failure) => (
                tr!("web_engine.settings_failed", why = crate::failure_text(failure).as_str()),
                vec![(DOWNLOAD, t("web_engine.retry"))],
            ),
            Status::Missing => (
                tr!(
                    "web_engine.settings_missing",
                    download = crate::DOWNLOAD_MB as i64,
                    installed = crate::INSTALLED_MB as i64
                ),
                vec![(DOWNLOAD, t("web_engine.download"))],
            ),
        };
        SettingsBlock {
            title: t("web_engine.settings_title"),
            detail: t("web_engine.settings_detail"),
            rows: vec![
                SettingsRow::Actions { text, buttons },
                SettingsRow::Toggle {
                    key: CHECK_ROW,
                    label: t("web_engine.check"),
                    detail: t("web_engine.check_detail"),
                    on: s.get(&CHECK),
                    default: Some(CHECK.default_value()),
                },
                SettingsRow::Notes(vec![t("web_engine.settings_note")]),
            ],
        }
    }

    fn poll(&self) -> bool {
        service::generation() != self.drawn.load(Ordering::Relaxed)
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        if key != CHECK_ROW {
            return;
        }
        if let Err(e) = s.set(&CHECK, value != 0) {
            tracing::warn!(error = %e, "a web engine setting wasn't saved");
        }
        apply(s);
    }

    fn command(&self, action: &'static str) -> Option<command_model::CommandId> {
        match action {
            DOWNLOAD => Some(crate::DOWNLOAD),
            CHECK_NOW => Some(crate::CHECK),
            REMOVE => Some(crate::REMOVE),
            STOP => Some(crate::STOP),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
