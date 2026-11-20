//! Where the notifications show (Settings → Appearance → "Notifications"): bottom right,
//! bottom center, top center (the default) or top right. Declared in `settings.rs`; every
//! window reads it when it draws them, so a change applies at once (a choice made here, or
//! the settings read again: `SettingChanged`).

use std::sync::atomic::{AtomicU8, Ordering};

use i18n::t;
use settings_model::SettingsService;
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
    ToastPosition,
};

use crate::settings::TOAST_POSITION;

/// The position in use (its place in [`ToastPosition::ALL`]).
static POSITION: AtomicU8 = AtomicU8::new(2);

/// Where the notifications show now.
pub fn position() -> ToastPosition {
    ToastPosition::ALL
        .get(usize::from(POSITION.load(Ordering::Relaxed)))
        .copied()
        .unwrap_or_default()
}

/// Shows them at `position` (what the setting saved says; a snapshot's own).
pub fn set_position(position: ToastPosition) {
    let at = ToastPosition::ALL.iter().position(|p| *p == position).unwrap_or(2);
    POSITION.store(at as u8, Ordering::Relaxed);
}

/// The position saved in `settings`.
fn saved(settings: &SettingsService) -> ToastPosition {
    settings.get(&TOAST_POSITION)
}

/// The choices' labels, in [`ToastPosition::ALL`]'s order.
fn label(position: ToastPosition) -> String {
    t(match position {
        ToastPosition::BottomRight => "settings.notifications_bottom_right",
        ToastPosition::BottomCenter => "settings.notifications_bottom_center",
        ToastPosition::TopCenter => "settings.notifications_top_center",
        ToastPosition::TopRight => "settings.notifications_top_right",
    })
}

/// The "Notifications" block, in the settings' "Appearance" section.
#[derive(Debug)]
pub struct ToastSettings;

impl SettingsContribution for ToastSettings {
    fn id(&self) -> &'static str {
        "notifications"
    }

    fn page(&self) -> &'static str {
        "notifications"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "notifications",
            label: "settings.notifications",
            icon: "bell",
            group: SettingsGroup::Interface,
            order: 30,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        set_position(saved(settings));
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let current = scope.settings.get(&TOAST_POSITION);
        let choices =
            ToastPosition::ALL.iter().enumerate().map(|(i, p)| (i as i64, label(*p))).collect();
        let current = ToastPosition::ALL.iter().position(|p| *p == current).unwrap_or(2) as i64;
        SettingsBlock {
            title: t("settings.notifications"),
            detail: t("settings.notifications_detail"),
            rows: vec![SettingsRow::Segmented {
                key: "position",
                detail: t("settings.notifications_position_detail"),
                default: ToastPosition::ALL
                    .iter()
                    .position(|p| *p == TOAST_POSITION.default_value())
                    .map(|i| i as i64),
                label: t("settings.notifications_position"),
                choices,
                current,
            }],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key != "position" {
            return;
        }
        let Some(position) = usize::try_from(value).ok().and_then(|i| ToastPosition::ALL.get(i))
        else {
            return;
        };
        if let Err(e) = scope.settings.set(&TOAST_POSITION, *position) {
            tracing::warn!(error = %e, "can't save where the notifications show");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/toasts.rs"]
mod tests;
