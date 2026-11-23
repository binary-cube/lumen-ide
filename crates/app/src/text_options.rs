//! How text is shown in every view (Settings → Editor → "Text", the View menu's "Word
//! Wrap"): the one `TextOptions` the windows hand their views, read here from the settings
//! and numbered by a generation, so each view is told once per change (the workbench
//! compares the generation each frame: a number, nothing else).

use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use i18n::t;
use settings_model::SettingsService;
use ui_workbench::TextOptions;
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

use crate::settings::WORD_WRAP;

static CURRENT: RwLock<TextOptions> = RwLock::new(TextOptions { word_wrap: false });
/// Bumped at each change (1 from the start: a view not told yet has 0).
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Held by a test that changes the options (they're the process's: tests take turns).
#[cfg(test)]
pub static TEST_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The options now.
pub fn current() -> TextOptions {
    CURRENT.read().map_or_else(|e| *e.into_inner(), |o| *o)
}

/// Their generation: a view told another is told again.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// The options set (from the settings; a test's own): a new generation when they changed.
pub fn set(options: TextOptions) {
    let mut now = CURRENT.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if *now != options {
        *now = options;
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Word wrap turned on or off (the View menu's command), saved and handed on.
pub fn toggle_word_wrap(settings: &SettingsService) {
    let on = !settings.get(&WORD_WRAP);
    if let Err(e) = settings.set(&WORD_WRAP, on) {
        tracing::warn!(error = %e, "can't save the word wrap option");
    }
    TextSettings.apply(settings);
}

/// The "Text" block, in the settings' "Editor" section.
#[derive(Debug)]
pub struct TextSettings;

impl SettingsContribution for TextSettings {
    fn id(&self) -> &'static str {
        "text"
    }

    fn page(&self) -> &'static str {
        "editor"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::EDITOR)
    }

    fn order(&self) -> u16 {
        15
    }

    fn apply(&self, settings: &SettingsService) {
        set(TextOptions { word_wrap: settings.get(&WORD_WRAP) });
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        SettingsBlock {
            title: t("settings.text"),
            detail: t("settings.text_detail"),
            rows: vec![SettingsRow::Toggle {
                key: "word_wrap",
                label: t("settings.word_wrap"),
                detail: t("settings.word_wrap_detail"),
                on: scope.settings.get(&WORD_WRAP),
                default: Some(WORD_WRAP.default_value()),
            }],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key != "word_wrap" {
            return;
        }
        if let Err(e) = scope.settings.set(&WORD_WRAP, value != 0) {
            tracing::warn!(error = %e, "can't save the word wrap option");
        }
        self.apply(scope.settings);
    }
}

#[cfg(test)]
#[path = "../tests/unit/text_options.rs"]
mod tests;
