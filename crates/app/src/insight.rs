//! What the editor shows in the code beside the text (Settings → Editor → "Code
//! insight"): the parameters' names at calls (inlay hints), the usages / implementations
//! after declarations (code lenses), the actions' bulb, the document's links followed with
//! ⌘-click — all on by default. Declared in `settings.rs`; read
//! here, every window looking at them when it works them out.

use std::sync::atomic::{AtomicBool, Ordering};

use i18n::t;
use settings_model::SettingsService;
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

use crate::settings::{ACTION_BULB, CODE_LENS, DOCUMENT_LINKS, INLAY_HINTS};

static HINTS: AtomicBool = AtomicBool::new(true);
static LENSES: AtomicBool = AtomicBool::new(true);
static BULB: AtomicBool = AtomicBool::new(true);
static LINKS: AtomicBool = AtomicBool::new(true);

/// The actions' bulb shows in the gutter.
pub fn bulb() -> bool {
    BULB.load(Ordering::Relaxed)
}

/// ⌘-click follows the document's links.
pub fn links() -> bool {
    LINKS.load(Ordering::Relaxed)
}

/// The parameters' names are shown at calls.
pub fn hints() -> bool {
    HINTS.load(Ordering::Relaxed)
}

/// The usages and implementations are written after declarations.
pub fn lenses() -> bool {
    LENSES.load(Ordering::Relaxed)
}

/// Shows them or not (what the settings say; a test's own).
pub fn set(hints: bool, lenses: bool) {
    HINTS.store(hints, Ordering::Relaxed);
    LENSES.store(lenses, Ordering::Relaxed);
}

/// The "Code insight" block, in the settings' "Editor" section.
#[derive(Debug)]
pub struct InsightSettings;

impl SettingsContribution for InsightSettings {
    fn id(&self) -> &'static str {
        "code_insight"
    }

    fn page(&self) -> &'static str {
        "editor"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::EDITOR)
    }

    fn order(&self) -> u16 {
        20
    }

    fn apply(&self, settings: &SettingsService) {
        set(settings.get(&INLAY_HINTS), settings.get(&CODE_LENS));
        BULB.store(settings.get(&ACTION_BULB), Ordering::Relaxed);
        LINKS.store(settings.get(&DOCUMENT_LINKS), Ordering::Relaxed);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        SettingsBlock {
            title: t("settings.insight"),
            detail: t("settings.insight_detail"),
            rows: vec![
                SettingsRow::Toggle {
                    key: "inlay_hints",
                    detail: t("settings.insight_hints_detail"),
                    default: Some(INLAY_HINTS.default_value()),
                    label: t("settings.insight_hints"),
                    on: scope.settings.get(&INLAY_HINTS),
                },
                SettingsRow::Toggle {
                    key: "code_lens",
                    detail: t("settings.insight_lenses_detail"),
                    default: Some(CODE_LENS.default_value()),
                    label: t("settings.insight_lenses"),
                    on: scope.settings.get(&CODE_LENS),
                },
                SettingsRow::Toggle {
                    key: "action_bulb",
                    label: t("settings.insight_bulb"),
                    detail: t("settings.insight_bulb_detail"),
                    on: scope.settings.get(&ACTION_BULB),
                    default: Some(ACTION_BULB.default_value()),
                },
                SettingsRow::Toggle {
                    key: "document_links",
                    label: t("settings.insight_links"),
                    detail: t("settings.insight_links_detail"),
                    on: scope.settings.get(&DOCUMENT_LINKS),
                    default: Some(DOCUMENT_LINKS.default_value()),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let saved = match key {
            "inlay_hints" => scope.settings.set(&INLAY_HINTS, value != 0),
            "code_lens" => scope.settings.set(&CODE_LENS, value != 0),
            "action_bulb" => scope.settings.set(&ACTION_BULB, value != 0),
            "document_links" => scope.settings.set(&DOCUMENT_LINKS, value != 0),
            _ => return,
        };
        if let Err(e) = saved {
            tracing::warn!("can't save the code insight option: {e}");
        }
        self.apply(scope.settings);
    }
}

#[cfg(test)]
#[path = "../tests/unit/insight.rs"]
mod tests;
