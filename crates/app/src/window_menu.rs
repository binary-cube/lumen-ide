//! The menu drawer's button in the window's bar (☰): always there on Windows and Linux
//! (the system gives the app no menu bar: it's the only menu), on by default on macOS too
//! (beside the system's own menu bar), where Settings → Appearance → "Menu" can turn it off. Declared in
//! `settings.rs`; every window reads it when it draws its bar, so a change applies at once.

use std::sync::atomic::{AtomicBool, Ordering};

use i18n::t;
use settings_model::SettingsService;
use workbench_model::{SettingsBlock, SettingsContribution, SettingsRow, SettingsScope};

use crate::settings::WINDOW_MENU;

/// The setting as last saved (macOS; its default until read).
static ASKED: AtomicBool = AtomicBool::new(true);

/// The window's bar has the menu button.
pub fn shown() -> bool {
    !cfg!(target_os = "macos") || ASKED.load(Ordering::Relaxed)
}

/// Shows it (macOS) or not, as the setting says (a snapshot's own).
pub fn set_asked(asked: bool) {
    ASKED.store(asked, Ordering::Relaxed);
}

/// The "Menu" block, at the end of Appearance (macOS only: elsewhere there's no choice).
#[derive(Debug)]
pub struct WindowMenuSettings;

impl SettingsContribution for WindowMenuSettings {
    fn id(&self) -> &'static str {
        "window_menu"
    }

    fn page(&self) -> &'static str {
        "appearance"
    }

    fn order(&self) -> u16 {
        200
    }

    fn apply(&self, settings: &SettingsService) {
        set_asked(settings.get(&WINDOW_MENU));
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        SettingsBlock {
            title: t("settings.window_menu"),
            detail: t("settings.window_menu_detail"),
            rows: vec![SettingsRow::Toggle {
                key: "shown",
                detail: t("settings.window_menu_toggle_detail"),
                default: Some(WINDOW_MENU.default_value()),
                label: t("settings.window_menu_toggle"),
                on: scope.settings.get(&WINDOW_MENU),
            }],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key != "shown" {
            return;
        }
        match scope.settings.set(&WINDOW_MENU, value != 0) {
            Ok(()) => set_asked(value != 0),
            Err(e) => {
                tracing::warn!(error = %e, "can't save whether the window's bar shows the menu")
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/window_menu.rs"]
mod tests;
