//! Focus mode's settings (`[focus]`): `strength` (how gray the rest turns, percent: 50, 75,
//! 100), `dim` (how much darker it is too, percent: 0, 10, 20) and `follow` (`keyboard`,
//! `mouse`, `both`: which area stays in color). Declared here, handed to the policy by
//! [`apply`] at startup and after every change ([`config`]), shown on their own page under
//! Interface ([`FocusSettings`]).

use std::sync::RwLock;

use i18n::t;
use settings_model::{Setting, SettingType, SettingsService};
use workbench_model::{
    Follow, SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow,
    SettingsScope,
};

/// The strengths offered (percent).
pub const STRENGTHS: [i64; 3] = [50, 75, 100];
/// The dims offered (percent).
pub const DIMS: [i64; 3] = [0, 10, 20];
/// The `follow` values, in the order offered.
pub const FOLLOWS: [(Follow, &str); 3] =
    [(Follow::Keyboard, "keyboard"), (Follow::Pointer, "mouse"), (Follow::Both, "both")];

/// `[focus] strength` (percent).
pub const STRENGTH: Setting<i64> =
    Setting::new("focus", "strength", || 100).valid(|p| STRENGTHS.contains(p));
/// `[focus] dim` (percent).
pub const DIM: Setting<i64> = Setting::new("focus", "dim", || 0).valid(|p| DIMS.contains(p));
/// `[focus] follow`: `keyboard`, `mouse` or `both`.
pub const FOLLOW: Setting<String> =
    Setting::new("focus", "follow", || "both".to_owned()).valid(|f| follow_of(f).is_some());

/// The `follow` value `saved` names.
pub fn follow_of(saved: &str) -> Option<Follow> {
    FOLLOWS.iter().find(|(_, id)| *id == saved).map(|(f, _)| *f)
}

/// What the policy follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// How far the rest goes to gray (0..1).
    pub gray: f32,
    /// How much darker it is (0..1).
    pub dim: f32,
    pub follow: Follow,
}

impl Config {
    pub const DEFAULT: Self = Self { gray: 1.0, dim: 0.0, follow: Follow::Both };

    /// What `settings` say.
    pub fn read(settings: &SettingsService) -> Self {
        let percent = |p: i64| p as f32 / 100.0;
        Self {
            gray: percent(settings.get(&STRENGTH)),
            dim: percent(settings.get(&DIM)),
            follow: follow_of(&settings.get(&FOLLOW)).unwrap_or(Self::DEFAULT.follow),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::DEFAULT
    }
}

static CONFIG: RwLock<Config> = RwLock::new(Config::DEFAULT);

/// What the settings say now.
pub fn config() -> Config {
    CONFIG.read().map_or(Config::DEFAULT, |c| *c)
}

/// The settings' values handed to the policy.
pub fn apply(settings: &SettingsService) {
    if let Ok(mut current) = CONFIG.write() {
        *current = Config::read(settings);
    }
}

/// `value` saved (said in the log when it can't be).
fn save<T: SettingType>(settings: &SettingsService, setting: &Setting<T>, value: T) {
    if !setting.accepts(&value) {
        return;
    }
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!("can't save a Focus mode setting: {e}");
    }
}

/// The follow choice's values: their place in [`FOLLOWS`].
fn follow_value(saved: &str) -> i64 {
    FOLLOWS.iter().position(|(_, id)| *id == saved).unwrap_or(2) as i64
}

/// The "Focus mode" page (Interface).
#[derive(Debug, Default)]
pub struct FocusSettings;

impl SettingsContribution for FocusSettings {
    fn id(&self) -> &'static str {
        "focus"
    }

    fn page(&self) -> &'static str {
        "focus"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "focus",
            label: "focus.settings_title",
            icon: crate::ICON,
            group: SettingsGroup::Interface,
            // Right before Relax mode's (80).
            order: 75,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let percents = |values: [i64; 3]| -> Vec<(i64, String)> {
            values
                .into_iter()
                .map(|p| (p, if p == 0 { t("focus.dim_off") } else { format!("{p} %") }))
                .collect()
        };
        let follows = ["focus.follow_keyboard", "focus.follow_mouse", "focus.follow_both"]
            .into_iter()
            .enumerate()
            .map(|(i, key)| (i as i64, t(key)))
            .collect();
        let rows = vec![
            SettingsRow::Segmented {
                key: "strength",
                label: t("focus.strength"),
                detail: t("focus.strength_detail"),
                choices: percents(STRENGTHS),
                current: s.get(&STRENGTH),
                default: Some(STRENGTH.default_value()),
            },
            SettingsRow::Segmented {
                key: "dim",
                label: t("focus.dim"),
                detail: t("focus.dim_detail"),
                choices: percents(DIMS),
                current: s.get(&DIM),
                default: Some(DIM.default_value()),
            },
            SettingsRow::Segmented {
                key: "follow",
                label: t("focus.follow"),
                detail: t("focus.follow_detail"),
                choices: follows,
                current: follow_value(&s.get(&FOLLOW)),
                default: Some(follow_value(&FOLLOW.default_value())),
            },
            SettingsRow::Notes(vec![t("focus.notes_effects"), t("focus.notes_shortcut")]),
        ];
        SettingsBlock { title: t("focus.settings_title"), detail: t("focus.settings_detail"), rows }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "strength" => save(s, &STRENGTH, value),
            "dim" => save(s, &DIM, value),
            "follow" => {
                if let Some((_, id)) = FOLLOWS.get(value as usize) {
                    save(s, &FOLLOW, (*id).to_owned());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
