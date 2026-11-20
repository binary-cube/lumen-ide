//! The remote projects' global settings (uploads / downloads at once, attempts:
//! `[remote] …` in `settings.toml`), kept here for the transfers — what every connection
//! follows unless it sets its own; their page in the settings window.

use std::sync::atomic::{AtomicBool, Ordering};

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// Uploads at once for a remote project (`[remote] uploads`), unless its connection sets its
/// own.
pub const REMOTE_UPLOADS: Setting<i64> =
    Setting::new("remote", "uploads", || 2).valid(|n| (1..=16).contains(n));

/// Downloads at once for a remote project (`[remote] downloads`).
pub const REMOTE_DOWNLOADS: Setting<i64> =
    Setting::new("remote", "downloads", || 4).valid(|n| (1..=16).contains(n));

/// Attempts of a transfer when the network fails (`[remote] retries`), the waits doubling
/// from a second.
pub const REMOTE_RETRIES: Setting<i64> =
    Setting::new("remote", "retries", || 4).valid(|n| (1..=10).contains(n));

/// The server's permissions and owner shown on a remote project's rows in the tree
/// (`[remote] show_permissions`; read once per folder shown).
pub const REMOTE_SHOW_PERMISSIONS: Setting<bool> =
    Setting::new("remote", "show_permissions", || false);

/// A warning when an uploaded file can't be read by the web server (neither its group nor
/// the others may read it: `[remote] warn_unreadable`).
pub const REMOTE_WARN_UNREADABLE: Setting<bool> =
    Setting::new("remote", "warn_unreadable", || true);

/// The choices offered in the settings.
pub const UPLOADS: [i64; 5] = [1, 2, 3, 4, 6];
pub const DOWNLOADS: [i64; 5] = [1, 2, 4, 6, 8];
pub const RETRIES: [i64; 4] = [1, 2, 4, 6];

/// The "Remote" page of the settings (the open projects' queues follow from their next
/// opening).
#[derive(Debug)]
pub(crate) struct RemoteSettings;

impl SettingsContribution for RemoteSettings {
    fn id(&self) -> &'static str {
        "remote"
    }

    fn page(&self) -> &'static str {
        "remote"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "remote",
            label: "settings.remote_section",
            icon: "cloud",
            group: SettingsGroup::Tools,
            order: 40,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        SHOW_PERMISSIONS.store(settings.get(&REMOTE_SHOW_PERMISSIONS), Ordering::Relaxed);
        WARN_UNREADABLE.store(settings.get(&REMOTE_WARN_UNREADABLE), Ordering::Relaxed);
        configure(remote::Defaults {
            limits: remote::Limits {
                uploads: settings.get(&REMOTE_UPLOADS).max(1) as usize,
                downloads: settings.get(&REMOTE_DOWNLOADS).max(1) as usize,
            },
            retry: remote::Retry {
                attempts: settings.get(&REMOTE_RETRIES).max(1) as u32,
                ..remote::Retry::default()
            },
        });
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        // Numbers in order: a slider over the choices.
        let row = |key, label: &str, stops: &[i64], setting: &Setting<i64>| SettingsRow::Slider {
            key,
            label: t(label),
            detail: t(&format!("{label}_detail")),
            stops: stops.iter().map(|&n| (n, n.to_string())).collect(),
            current: scope.settings.get(setting),
            default: Some(setting.default_value()),
        };
        SettingsBlock {
            title: t("settings.remote"),
            detail: t("settings.remote_detail"),
            rows: vec![
                row("uploads", "settings.remote_uploads", &UPLOADS, &REMOTE_UPLOADS),
                row("downloads", "settings.remote_downloads", &DOWNLOADS, &REMOTE_DOWNLOADS),
                row("retries", "settings.remote_retries", &RETRIES, &REMOTE_RETRIES),
                SettingsRow::Toggle {
                    key: "show_permissions",
                    label: t("settings.remote_show_permissions"),
                    detail: t("settings.remote_show_permissions_detail"),
                    default: Some(REMOTE_SHOW_PERMISSIONS.default_value()),
                    on: scope.settings.get(&REMOTE_SHOW_PERMISSIONS),
                },
                SettingsRow::Toggle {
                    key: "warn_unreadable",
                    label: t("settings.remote_warn_unreadable"),
                    detail: t("settings.remote_warn_unreadable_detail"),
                    default: Some(REMOTE_WARN_UNREADABLE.default_value()),
                    on: scope.settings.get(&REMOTE_WARN_UNREADABLE),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let toggle = match key {
            "show_permissions" => Some(&REMOTE_SHOW_PERMISSIONS),
            "warn_unreadable" => Some(&REMOTE_WARN_UNREADABLE),
            _ => None,
        };
        if let Some(toggle) = toggle {
            if let Err(e) = settings.set(toggle, value != 0) {
                tracing::warn!(error = %e, "can't save a remote setting");
            }
            return;
        }
        let setting = match key {
            "uploads" => &REMOTE_UPLOADS,
            "downloads" => &REMOTE_DOWNLOADS,
            "retries" => &REMOTE_RETRIES,
            _ => return,
        };
        if let Err(e) = settings.set(setting, value) {
            tracing::warn!(error = %e, "can't save a remote setting");
        }
    }
}

/// The tree's permissions and the uploads' warning, as set (read while drawing: no lock).
static SHOW_PERMISSIONS: AtomicBool = AtomicBool::new(false);
static WARN_UNREADABLE: AtomicBool = AtomicBool::new(true);

/// The server's permissions are shown in the tree.
pub fn show_permissions() -> bool {
    SHOW_PERMISSIONS.load(Ordering::Relaxed)
}

/// An uploaded file the web server can't read is warned about.
pub fn warn_unreadable() -> bool {
    WARN_UNREADABLE.load(Ordering::Relaxed)
}

/// The global settings in use (the user's), set at startup and when they change.
static DEFAULTS: std::sync::RwLock<Option<remote::Defaults>> = std::sync::RwLock::new(None);

fn configure(defaults: remote::Defaults) {
    if let Ok(mut current) = DEFAULTS.write() {
        *current = Some(defaults);
    }
}

/// What every connection follows unless it says otherwise (the defaults until set).
pub fn defaults() -> remote::Defaults {
    DEFAULTS.read().ok().and_then(|d| *d).unwrap_or_default()
}
