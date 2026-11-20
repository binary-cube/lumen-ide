//! The `.env` files' settings (`[dotenv]`): `mask_secrets` — the values that look like
//! secrets drawn hidden until the pointer is over them or the caret is on their line —
//! and `unused_keys` — a key no code reads marked as a note; both off by default. Declared
//! here, handed to the module by [`apply`] at startup and after every change (what follows
//! them is told: [`follow`]), shown on the Languages page while `.env` is chosen
//! ([`DotenvSettings`]).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use i18n::t;
use settings_model::{Setting, SettingType, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsLanguage, SettingsPage, SettingsRow, SettingsScope,
};

/// `[dotenv] mask_secrets`: secret-looking values hidden until hovered.
pub const MASK_SECRETS: Setting<bool> = Setting::new("dotenv", "mask_secrets", || false);

/// `[dotenv] unused_keys`: a key no code of the project reads is a note.
pub const UNUSED_KEYS: Setting<bool> = Setting::new("dotenv", "unused_keys", || false);

static MASK: AtomicBool = AtomicBool::new(false);
static UNUSED: AtomicBool = AtomicBool::new(false);
/// Bumped whenever a value changes: what was made from the settings is made again.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Secret-looking values are hidden (the setting, as last applied).
pub fn mask_secrets() -> bool {
    MASK.load(Ordering::Relaxed)
}

/// Keys no code reads are flagged (the setting, as last applied).
pub fn unused_keys() -> bool {
    UNUSED.load(Ordering::Relaxed)
}

/// Changes whenever a setting's value does.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The setting as if applied (tests).
#[cfg(test)]
pub fn set_mask_secrets(on: bool) {
    store(&MASK, on);
}

/// The setting as if applied (tests).
#[cfg(test)]
pub fn set_unused_keys(on: bool) {
    store(&UNUSED, on);
}

/// What's told when a value changes (dropped with its owner: the project's checks).
type Follower = Weak<dyn Fn() + Send + Sync>;

static FOLLOWERS: Mutex<Vec<Follower>> = Mutex::new(Vec::new());

/// Calls `changed` (on the thread applying them) whenever a value changes, as long as it's
/// kept.
pub fn follow(changed: &Arc<dyn Fn() + Send + Sync>) {
    FOLLOWERS.lock().unwrap_or_else(PoisonError::into_inner).push(Arc::downgrade(changed));
}

fn store(flag: &AtomicBool, on: bool) {
    store_telling(flag, on, &FOLLOWERS);
}

/// `flag` set to `on`; if that changes it, `told` hears.
fn store_telling(flag: &AtomicBool, on: bool, told: &Mutex<Vec<Follower>>) {
    if flag.swap(on, Ordering::Relaxed) == on {
        return;
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
    let followers: Vec<Arc<dyn Fn() + Send + Sync>> = {
        let mut all = told.lock().unwrap_or_else(PoisonError::into_inner);
        all.retain(|f| f.strong_count() > 0);
        all.iter().filter_map(Weak::upgrade).collect()
    };
    for changed in followers {
        changed();
    }
}

/// The settings' values handed to the module.
pub fn apply(settings: &SettingsService) {
    store(&MASK, settings.get(&MASK_SECRETS));
    store(&UNUSED, settings.get(&UNUSED_KEYS));
}

fn save<T: SettingType>(settings: &SettingsService, setting: &Setting<T>, value: T) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!(error = %e, "can't save a .env setting");
    }
}

/// The ".env files" block of the settings window (Languages, while `.env` is chosen).
#[derive(Debug, Default)]
pub struct DotenvSettings;

impl SettingsContribution for DotenvSettings {
    fn id(&self) -> &'static str {
        "dotenv"
    }

    fn page(&self) -> &'static str {
        "languages"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGES)
    }

    fn order(&self) -> u16 {
        30
    }

    fn language(&self) -> Option<SettingsLanguage> {
        Some(SettingsLanguage { id: crate::LANGUAGE.to_string(), name: crate::NAME.to_owned() })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        SettingsBlock {
            title: t("dotenv.settings_title"),
            detail: t("dotenv.settings_detail"),
            rows: vec![
                SettingsRow::Toggle {
                    key: "mask_secrets",
                    label: t("dotenv.mask_secrets"),
                    detail: t("dotenv.mask_secrets_detail"),
                    on: s.get(&MASK_SECRETS),
                    default: Some(MASK_SECRETS.default_value()),
                },
                SettingsRow::Toggle {
                    key: "unused_keys",
                    label: t("dotenv.unused_keys"),
                    detail: t("dotenv.unused_keys_detail"),
                    on: s.get(&UNUSED_KEYS),
                    default: Some(UNUSED_KEYS.default_value()),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        match key {
            "mask_secrets" => save(scope.settings, &MASK_SECRETS, value != 0),
            "unused_keys" => save(scope.settings, &UNUSED_KEYS, value != 0),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
