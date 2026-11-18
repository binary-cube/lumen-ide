//! The indexing module's settings: semantic search (`[search] semantic` in `settings.toml`),
//! handed to the model ([`crate::semantic`]); its block in the settings' "Search" section —
//! the switch, what the model is doing (downloading, ready…) and what it costs.

use std::sync::Mutex;

use i18n::{t, tr};
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
    StatusTone,
};

use crate::semantic::{self, SearchMode};

/// Semantic search (`[search] semantic`); with no choice, on when the model is installed.
pub const SEMANTIC_SEARCH: Setting<bool> =
    Setting::new("search", "semantic", semantic::is_installed);

const ON: &str = "on";

/// Saves the choice (from the project search's button).
pub(crate) fn save_semantic(settings: &SettingsService, on: bool) {
    if let Err(e) = settings.set(&SEMANTIC_SEARCH, on) {
        tracing::warn!("can't save the semantic search option: {e}");
    }
}

/// The block in the settings' "Search" section.
#[derive(Debug, Default)]
pub(crate) struct SemanticSettings {
    /// The model's state the block showed last (a change draws it again: the download's
    /// progress).
    shown: Mutex<Option<SearchMode>>,
}

impl SettingsContribution for SemanticSettings {
    fn id(&self) -> &'static str {
        "semantic"
    }

    fn page(&self) -> &'static str {
        "search"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "search",
            label: "settings.search",
            icon: "search",
            group: SettingsGroup::Tools,
            order: 50,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        semantic::configure(settings.get(&SEMANTIC_SEARCH));
    }

    fn poll(&self) -> bool {
        let mode = semantic::mode();
        let Ok(mut shown) = self.shown.lock() else { return false };
        if shown.as_ref() == Some(&mode) {
            return false;
        }
        *shown = Some(mode);
        true
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let on = scope.settings.get(&SEMANTIC_SEARCH);
        let mode = semantic::mode();
        let (status, tone) = match (&mode, on) {
            (_, false) => (t("settings.semantic_off"), StatusTone::Neutral),
            (SearchMode::Semantic, true) => (t("settings.semantic_active"), StatusTone::Accent),
            (SearchMode::Downloading(got, total), true) => (
                tr!("search.downloading", got = got >> 20, total = (*total).max(1) >> 20),
                StatusTone::Neutral,
            ),
            (SearchMode::Failed(_), true) => (t("settings.semantic_failed"), StatusTone::Danger),
            (SearchMode::Loading | SearchMode::Lexical, true) => {
                (t("settings.semantic_loading"), StatusTone::Neutral)
            }
            (SearchMode::Standby, true) => (t("settings.semantic_standby"), StatusTone::Neutral),
        };
        if let Ok(mut shown) = self.shown.lock() {
            *shown = Some(mode);
        }
        SettingsBlock {
            title: t("settings.semantic"),
            detail: t("settings.semantic_detail"),
            rows: vec![
                SettingsRow::SwitchCard {
                    key: ON,
                    label: t("settings.semantic_toggle"),
                    detail: t("settings.semantic_toggle_detail"),
                    on,
                    status,
                    tone,
                },
                // What it does and what it costs.
                SettingsRow::Notes(vec![
                    t("settings.semantic_note_1"),
                    t("settings.semantic_note_2"),
                ]),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key != ON {
            return;
        }
        let on = value != 0;
        save_semantic(scope.settings, on);
        semantic::set_enabled(on, scope.wake.cloned());
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
