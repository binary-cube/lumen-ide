//! The Markdown preview's settings (`[markdown]`): `web_images` (images from `http(s)://`
//! loaded, default on), `wrap_code` (long code lines wrapped, default off: they scroll) and
//! `sync_scroll` (Split: the text and the preview scroll together, default on). Declared
//! here, handed to the preview by [`apply`] at startup and
//! after every change, shown on their own page under Editing ([`MarkdownSettings`]).

use std::sync::atomic::{AtomicBool, Ordering};

use i18n::t;
use settings_model::{Setting, SettingType, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// `[markdown] web_images`: images from the web are downloaded.
pub const WEB_IMAGES: Setting<bool> = Setting::new("markdown", "web_images", || true);

/// `[markdown] wrap_code`: long lines in code blocks wrap (otherwise the block scrolls).
pub const WRAP_CODE: Setting<bool> = Setting::new("markdown", "wrap_code", || false);

/// `[markdown] sync_scroll`: beside its text, the preview scrolls with it (both ways).
pub const SYNC_SCROLL: Setting<bool> = Setting::new("markdown", "sync_scroll", || true);

static WEB: AtomicBool = AtomicBool::new(true);
static WRAP: AtomicBool = AtomicBool::new(false);
static SYNC: AtomicBool = AtomicBool::new(true);

/// The text and the preview scroll together (the setting, as last applied).
pub fn sync_scroll() -> bool {
    SYNC.load(Ordering::Relaxed)
}

/// The setting as if applied (tests).
#[cfg(test)]
pub fn set_sync_scroll(on: bool) {
    SYNC.store(on, Ordering::Relaxed);
}

/// Long code lines wrap (the setting, as last applied).
pub fn wrap_code() -> bool {
    WRAP.load(Ordering::Relaxed)
}

/// Images from the web are loaded (the setting, as last applied).
pub fn web_images() -> bool {
    WEB.load(Ordering::Relaxed)
}

/// The setting as if applied (tests).
#[cfg(test)]
pub fn set_web_images(on: bool) {
    WEB.store(on, Ordering::Relaxed);
}

/// The setting as if applied (tests).
#[cfg(test)]
pub fn set_wrap_code(on: bool) {
    WRAP.store(on, Ordering::Relaxed);
}

/// The settings' values handed to the preview.
pub fn apply(settings: &SettingsService) {
    WEB.store(settings.get(&WEB_IMAGES), Ordering::Relaxed);
    WRAP.store(settings.get(&WRAP_CODE), Ordering::Relaxed);
    SYNC.store(settings.get(&SYNC_SCROLL), Ordering::Relaxed);
}

fn save<T: SettingType>(settings: &SettingsService, setting: &Setting<T>, value: T) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!("can't save a Markdown setting: {e}");
    }
}

/// The "Markdown" page (Editing).
#[derive(Debug, Default)]
pub struct MarkdownSettings;

impl SettingsContribution for MarkdownSettings {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn page(&self) -> &'static str {
        "markdown"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "markdown",
            label: "markdown.settings_title",
            icon: "file-text",
            group: SettingsGroup::Editing,
            order: 25,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let rows = vec![
            SettingsRow::Toggle {
                key: "web_images",
                label: t("markdown.web_images"),
                detail: t("markdown.web_images_detail"),
                on: s.get(&WEB_IMAGES),
                default: Some(WEB_IMAGES.default_value()),
            },
            SettingsRow::Toggle {
                key: "wrap_code",
                label: t("markdown.wrap_code"),
                detail: t("markdown.wrap_code_detail"),
                on: s.get(&WRAP_CODE),
                default: Some(WRAP_CODE.default_value()),
            },
            SettingsRow::Toggle {
                key: "sync_scroll",
                label: t("markdown.sync_scroll"),
                detail: t("markdown.sync_scroll_detail"),
                on: s.get(&SYNC_SCROLL),
                default: Some(SYNC_SCROLL.default_value()),
            },
        ];
        SettingsBlock {
            title: t("markdown.settings_title"),
            detail: t("markdown.settings_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        match key {
            "web_images" => save(scope.settings, &WEB_IMAGES, value != 0),
            "wrap_code" => save(scope.settings, &WRAP_CODE, value != 0),
            "sync_scroll" => save(scope.settings, &SYNC_SCROLL, value != 0),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
