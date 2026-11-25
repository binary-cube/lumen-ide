//! Settings → Editing → Files: saving automatically and the final line break, each a block of
//! its own (declared in `settings.rs`, read by `autosave` and `final_newline`), on the page
//! the local history's block shares.

use i18n::t;
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

use crate::settings::{AUTOSAVE, FINAL_NEWLINE, SESSIONS_KEPT, SESSIONS_REOPEN};

/// "Autosave".
#[derive(Debug)]
pub struct AutosaveSettings;

impl SettingsContribution for AutosaveSettings {
    fn id(&self) -> &'static str {
        "autosave"
    }

    fn page(&self) -> &'static str {
        SettingsPage::FILES.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::FILES)
    }

    fn order(&self) -> u16 {
        10
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        SettingsBlock {
            title: t("settings.autosave"),
            detail: t("settings.autosave_detail"),
            rows: vec![
                SettingsRow::Toggle {
                    key: "autosave",
                    detail: t("settings.autosave_toggle_detail"),
                    default: Some(AUTOSAVE.default_value()),
                    label: t("settings.autosave_toggle"),
                    on: scope.settings.get(&AUTOSAVE),
                },
                SettingsRow::Notes(vec![
                    t("settings.autosave_note_1"),
                    t("settings.autosave_note_2"),
                ]),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key == "autosave"
            && let Err(e) = scope.settings.set(&AUTOSAVE, value != 0)
        {
            tracing::warn!(error = %e, "can't save the autosave option");
        }
    }
}

/// "Final newline".
#[derive(Debug)]
pub struct FinalNewlineSettings;

impl SettingsContribution for FinalNewlineSettings {
    fn id(&self) -> &'static str {
        "final_newline"
    }

    fn page(&self) -> &'static str {
        SettingsPage::FILES.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::FILES)
    }

    fn order(&self) -> u16 {
        20
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        SettingsBlock {
            title: t("settings.final_newline"),
            detail: t("settings.final_newline_detail"),
            rows: vec![
                SettingsRow::Toggle {
                    key: "final_newline",
                    detail: t("settings.final_newline_toggle_detail"),
                    default: Some(FINAL_NEWLINE.default_value()),
                    label: t("settings.final_newline_toggle"),
                    on: scope.settings.get(&FINAL_NEWLINE),
                },
                SettingsRow::Notes(vec![t("settings.final_newline_note")]),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key == "final_newline"
            && let Err(e) = scope.settings.set(&FINAL_NEWLINE, value != 0)
        {
            tracing::warn!(error = %e, "can't save the final newline option");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/file_settings.rs"]
mod tests;

/// "Previous sessions": how many are kept, how many windows "Reopen" opens.
#[derive(Debug)]
pub struct SessionSettings;

impl SettingsContribution for SessionSettings {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn page(&self) -> &'static str {
        SettingsPage::FILES.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::FILES)
    }

    fn order(&self) -> u16 {
        30
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let stops = |values: &[i64]| {
            values
                .iter()
                .map(|v| (*v, if *v == 0 { t("settings.sessions_all") } else { v.to_string() }))
                .collect()
        };
        SettingsBlock {
            title: t("settings.sessions"),
            detail: t("settings.sessions_detail"),
            rows: vec![
                SettingsRow::Slider {
                    key: "sessions_kept",
                    label: t("settings.sessions_kept"),
                    detail: t("settings.sessions_kept_detail"),
                    stops: stops(&[3, 6, 10, 20, 30]),
                    current: scope.settings.get(&SESSIONS_KEPT),
                    default: Some(SESSIONS_KEPT.default_value()),
                },
                SettingsRow::Slider {
                    key: "sessions_reopen",
                    label: t("settings.sessions_reopen"),
                    detail: t("settings.sessions_reopen_detail"),
                    stops: stops(&[3, 5, 10, 15, 20, 0]),
                    current: scope.settings.get(&SESSIONS_REOPEN),
                    default: Some(SESSIONS_REOPEN.default_value()),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let setting = match key {
            "sessions_kept" => &SESSIONS_KEPT,
            "sessions_reopen" => &SESSIONS_REOPEN,
            _ => return,
        };
        if let Err(e) = scope.settings.set(setting, value.max(0)) {
            tracing::warn!(error = %e, "can't save the sessions option");
        }
    }
}
