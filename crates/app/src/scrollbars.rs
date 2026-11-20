//! The scrollbars' look (Settings → Appearance → "Scrollbars"): how a bar answers the pointer
//! (widening into its rail, or only its thumb thickening while the rest stays a thin line), its
//! thumb's thickness, whether the thin line shows at all, when an edge's bar shows. Declared in `settings.rs`; every bar paints with
//! it (`ui_components::scrollbar::look`), so a change applies at the next frame.

use i18n::t;
use settings_model::SettingsService;
use ui_workbench::{ScrollbarHover, ScrollbarLook, ScrollbarVisibility, scrollbar};
use workbench_model::{SettingsBlock, SettingsContribution, SettingsRow, SettingsScope};

use crate::settings::{SCROLLBAR_HOVER, SCROLLBAR_LINE, SCROLLBAR_THICKNESS, SCROLLBAR_VISIBILITY};

/// The look saved in the settings.
pub fn saved(settings: &SettingsService) -> ScrollbarLook {
    ScrollbarLook {
        hover: ScrollbarHover::parse(&settings.get(&SCROLLBAR_HOVER)).unwrap_or_default(),
        thickness: settings.get(&SCROLLBAR_THICKNESS) as f32,
        line: settings.get(&SCROLLBAR_LINE),
        visibility: ScrollbarVisibility::parse(&settings.get(&SCROLLBAR_VISIBILITY))
            .unwrap_or_default(),
    }
}

/// The choices' labels, in the order of their ids (`ScrollbarHover::ALL`, …).
const HOVERS: [&str; 2] = ["settings.scrollbar_rail", "settings.scrollbar_thumb"];
const VISIBILITIES: [&str; 3] = [
    "settings.scrollbar_by_view",
    "settings.scrollbar_always",
    "settings.scrollbar_while_scrolling",
];
const THICKNESSES: [i64; 4] = [3, 5, 6, 8];

fn choices(labels: &[&str]) -> Vec<(i64, String)> {
    labels.iter().enumerate().map(|(i, key)| (i as i64, t(key))).collect()
}

/// The place of the id `value` among `ids` (the first one when it's unknown).
fn place<T>(ids: &[(T, &str)], value: &str) -> i64 {
    ids.iter().position(|(_, id)| *id == value).unwrap_or(0) as i64
}

/// The "Scrollbars" block, in Appearance.
#[derive(Debug)]
pub struct ScrollbarSettings;

impl SettingsContribution for ScrollbarSettings {
    fn id(&self) -> &'static str {
        "scrollbars"
    }

    fn page(&self) -> &'static str {
        "appearance"
    }

    fn order(&self) -> u16 {
        170
    }

    fn apply(&self, settings: &SettingsService) {
        scrollbar::set_look(saved(settings));
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let hovers = &ScrollbarHover::ALL;
        let visibilities = &ScrollbarVisibility::ALL;
        let rows = vec![
            SettingsRow::Choices {
                key: "hover",
                detail: t("settings.scrollbar_hover_detail"),
                default: Some(place(hovers, &SCROLLBAR_HOVER.default_value())),
                label: t("settings.scrollbar_hover"),
                choices: choices(&HOVERS),
                current: place(hovers, &settings.get(&SCROLLBAR_HOVER)),
            },
            SettingsRow::Slider {
                key: "thickness",
                detail: t("settings.scrollbar_thickness_detail"),
                default: Some(SCROLLBAR_THICKNESS.default_value()),
                label: t("settings.scrollbar_thickness"),
                stops: THICKNESSES.iter().map(|w| (*w, format!("{w} px"))).collect(),
                current: settings.get(&SCROLLBAR_THICKNESS),
            },
            SettingsRow::Toggle {
                key: "line",
                detail: t("settings.scrollbar_line_detail"),
                default: Some(SCROLLBAR_LINE.default_value()),
                label: t("settings.scrollbar_line"),
                on: settings.get(&SCROLLBAR_LINE),
            },
            SettingsRow::Choices {
                key: "visibility",
                detail: t("settings.scrollbar_visibility_detail"),
                default: Some(place(visibilities, &SCROLLBAR_VISIBILITY.default_value())),
                label: t("settings.scrollbar_visibility"),
                choices: choices(&VISIBILITIES),
                current: place(visibilities, &settings.get(&SCROLLBAR_VISIBILITY)),
            },
        ];
        SettingsBlock {
            title: t("settings.scrollbars"),
            detail: t("settings.scrollbars_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let saved_it = match key {
            "hover" => settings.set(&SCROLLBAR_HOVER, id_of(&ScrollbarHover::ALL, value)),
            "thickness" => settings.set(&SCROLLBAR_THICKNESS, value),
            "line" => settings.set(&SCROLLBAR_LINE, value != 0),
            "visibility" => {
                settings.set(&SCROLLBAR_VISIBILITY, id_of(&ScrollbarVisibility::ALL, value))
            }
            _ => return,
        };
        // Applied by the app after the change (`apply`), as every block's.
        if let Err(e) = saved_it {
            tracing::warn!(error = %e, "can't save the scrollbars' setting");
        }
    }
}

/// The id saved for the choice at `value` among `ids`.
fn id_of<T>(ids: &[(T, &'static str)], value: i64) -> String {
    ids.get(value as usize).map_or_else(String::new, |(_, id)| (*id).to_owned())
}

#[cfg(test)]
#[path = "../tests/unit/scrollbars.rs"]
mod tests;
