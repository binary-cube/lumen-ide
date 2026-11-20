//! Keep awake's settings (`[keep_awake]`): `on` (kept as chosen, at the next start too),
//! `when` (`always` while on — the default —, or `working`: only while work runs, the
//! user's choice);
//! `screen` (the screen kept on too: it doesn't turn off nor lock; off by default);
//! what the screen does while kept awake and untouched — `dim` (after how many minutes,
//! 0: never), `level` (how dark, percent), `drift` (a mark drifting over it, so nothing
//! burns in).
//! Declared here, handed to the keeper by [`apply`] at startup and after every change
//! ([`config`]), shown on their own page under Interface ([`KeepAwakeSettings`]).

use std::sync::RwLock;

use i18n::t;
use std::time::Duration;

use power::{Mode, PowerError, ScreenCare};
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// `[keep_awake] on`: the computer is kept awake.
pub const ON: Setting<bool> = Setting::new("keep_awake", "on", || false);
/// The `when` values, in the order offered: always while on, or only while work runs.
pub const WHENS: [Mode; 2] = [Mode::Always, Mode::WhileWorking];
/// `[keep_awake] when`: `always` or `working`.
pub const WHEN: Setting<String> = Setting::new("keep_awake", "when", || "always".to_owned())
    .valid(|w| Mode::parse(w).is_some_and(|m| m != Mode::Off));
/// The minutes offered before the screen dims (0: never).
pub const DIM_MINUTES: [i64; 5] = [0, 5, 10, 15, 30];
/// `[keep_awake] dim`: minutes without input before the screen dims.
pub const DIM: Setting<i64> =
    Setting::new("keep_awake", "dim", || 10).valid(|m| DIM_MINUTES.contains(m));
/// The darkness offered (percent).
pub const LEVELS: [i64; 3] = [50, 70, 85];
/// `[keep_awake] level`: how dark it gets (percent).
pub const LEVEL: Setting<i64> =
    Setting::new("keep_awake", "level", || 70).valid(|p| LEVELS.contains(p));
/// `[keep_awake] drift`: a mark drifts over the dimmed screen.
pub const DRIFT: Setting<bool> = Setting::new("keep_awake", "drift", || true);
/// `[keep_awake] screen`: the screen kept on too (it doesn't turn off nor lock).
pub const SCREEN: Setting<bool> = Setting::new("keep_awake", "screen", || false);

/// `[keep_awake] scene`: instead of dimming, a calm scene over the whole screen (when a
/// module offers one).
pub const SCENE: Setting<bool> = Setting::new("keep_awake", "scene", || false);

/// What the settings say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub on: bool,
    /// While on: always, or only while work runs.
    pub when: Mode,
    /// The screen kept on too.
    pub screen: bool,
    /// A calm scene over the whole screen instead of dimming it.
    pub scene: bool,
    pub care: ScreenCare,
}

impl Config {
    pub const DEFAULT: Self = Self {
        on: false,
        when: Mode::Always,
        screen: false,
        scene: false,
        care: ScreenCare { dim_after: Some(Duration::from_secs(600)), level: 0.7, drift: true },
    };

    pub fn read(settings: &SettingsService) -> Self {
        let minutes = settings.get(&DIM);
        Self {
            on: settings.get(&ON),
            when: Mode::parse(&settings.get(&WHEN)).unwrap_or(Mode::Always),
            screen: settings.get(&SCREEN),
            scene: settings.get(&SCENE),
            care: ScreenCare {
                dim_after: (minutes > 0).then(|| Duration::from_secs(minutes as u64 * 60)),
                level: settings.get(&LEVEL) as f32 / 100.0,
                drift: settings.get(&DRIFT),
            },
        }
    }
}

impl Config {
    /// When the computer is kept awake now: never while off.
    pub fn mode(&self) -> Mode {
        if self.on { self.when } else { Mode::Off }
    }

    /// What a hold keeps awake.
    pub fn keep(&self) -> power::Keep {
        power::Keep { screen: self.screen }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(not(test))]
static CONFIG: RwLock<Config> = RwLock::new(Config::DEFAULT);

#[cfg(test)]
thread_local! {
    // Each test thread its own (they run side by side).
    static CONFIG: RwLock<Config> = const { RwLock::new(Config::DEFAULT) };
}

fn with_config<R>(f: impl FnOnce(&RwLock<Config>) -> R) -> R {
    #[cfg(not(test))]
    return f(&CONFIG);
    #[cfg(test)]
    CONFIG.with(f)
}

/// What the settings say now.
pub fn config() -> Config {
    with_config(|c| c.read().map_or_else(|_| Config::default(), |c| *c))
}

/// The settings' values handed to the keeper: the hold taken or let go as they say. `Err`:
/// the system wouldn't keep awake (nothing held).
pub fn apply(settings: &SettingsService) -> Result<(), PowerError> {
    let read = Config::read(settings);
    with_config(|c| {
        if let Ok(mut current) = c.write() {
            *current = read;
        }
    });
    crate::watcher::follow(read.mode())
}

/// `on` saved (said in the log when it can't be).
pub fn save_on(settings: &SettingsService, on: bool) {
    save(settings, &ON, on);
}

/// `screen` saved: the screen kept on too.
pub fn save_screen(settings: &SettingsService, screen: bool) {
    save(settings, &SCREEN, screen);
}

/// `scene` saved: a calm scene instead of dimming.
pub fn save_scene(settings: &SettingsService, scene: bool) {
    save(settings, &SCENE, scene);
}

/// `when` saved: only while work runs (`true`), or always while on.
pub fn save_while_working(settings: &SettingsService, working: bool) {
    let mode = if working { Mode::WhileWorking } else { Mode::Always };
    save(settings, &WHEN, mode.id().to_owned());
}

/// `value` saved (said in the log when it can't be).
fn save<T: settings_model::SettingType>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if !setting.accepts(&value) {
        return;
    }
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!(error = %e, "can't save a Keep awake setting");
    }
}

/// The "Keep awake" page (Interface).
#[derive(Debug, Default)]
pub struct KeepAwakeSettings;

impl SettingsContribution for KeepAwakeSettings {
    fn id(&self) -> &'static str {
        "keep_awake"
    }

    fn page(&self) -> &'static str {
        "keep_awake"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "keep_awake",
            label: "keep_awake.settings_title",
            icon: crate::ICON,
            group: SettingsGroup::Interface,
            // Right before Focus mode's (75).
            order: 72,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        if let Err(why) = apply(settings) {
            tracing::warn!(error = %why, "the computer can't be kept awake");
        }
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let minutes = DIM_MINUTES
            .into_iter()
            .map(|m| {
                let label = if m == 0 {
                    t("keep_awake.dim_never")
                } else {
                    i18n::tr!("keep_awake.minutes", count = m)
                };
                (m, label)
            })
            .collect();
        let levels = LEVELS.into_iter().map(|p| (p, format!("{p} %"))).collect();
        let mut rows = vec![
            SettingsRow::Toggle {
                key: "on",
                label: t("keep_awake.on"),
                detail: t("keep_awake.on_detail"),
                on: s.get(&ON),
                default: Some(ON.default_value()),
            },
            SettingsRow::Segmented {
                key: "when",
                label: t("keep_awake.when"),
                detail: t("keep_awake.when_detail"),
                choices: vec![(0, t("keep_awake.when_always")), (1, t("keep_awake.when_working"))],
                current: i64::from(Mode::parse(&s.get(&WHEN)) == Some(Mode::WhileWorking)),
                default: Some(0),
            },
            SettingsRow::Toggle {
                key: "screen",
                label: t("keep_awake.screen"),
                detail: t("keep_awake.screen_detail"),
                on: s.get(&SCREEN),
                default: Some(SCREEN.default_value()),
            },
            SettingsRow::Segmented {
                key: "dim",
                label: t("keep_awake.dim"),
                detail: t("keep_awake.dim_detail"),
                choices: minutes,
                current: s.get(&DIM),
                default: Some(DIM.default_value()),
            },
            SettingsRow::Segmented {
                key: "level",
                label: t("keep_awake.level"),
                detail: t("keep_awake.level_detail"),
                choices: levels,
                current: s.get(&LEVEL),
                default: Some(LEVEL.default_value()),
            },
            SettingsRow::Toggle {
                key: "drift",
                label: t("keep_awake.drift"),
                detail: t("keep_awake.drift_detail"),
                on: s.get(&DRIFT),
                default: Some(DRIFT.default_value()),
            },
            SettingsRow::Notes(vec![t("keep_awake.notes_screen"), t("keep_awake.notes_shortcut")]),
        ];
        // Only while someone has a scene to show.
        if workbench_model::presence::request::<contracts::scenes::ShowCalmScene>() {
            let at = rows.len() - 1;
            rows.insert(
                at,
                SettingsRow::Toggle {
                    key: "scene",
                    label: t("keep_awake.scene"),
                    detail: t("keep_awake.scene_detail"),
                    on: s.get(&SCENE),
                    default: Some(SCENE.default_value()),
                },
            );
        }
        SettingsBlock {
            title: t("keep_awake.settings_title"),
            detail: t("keep_awake.settings_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "when" => {
                let mode = if value == 1 { Mode::WhileWorking } else { Mode::Always };
                save(s, &WHEN, mode.id().to_owned());
            }
            "dim" => save(s, &DIM, value),
            "level" => save(s, &LEVEL, value),
            "drift" => save(s, &DRIFT, value != 0),
            "screen" => save(s, &SCREEN, value != 0),
            "scene" => save(s, &SCENE, value != 0),
            _ => {}
        }
        if matches!(key, "when" | "dim" | "level" | "drift" | "screen" | "scene") {
            let _ = apply(s);
            return;
        }
        if key == "on" {
            save_on(scope.settings, value != 0);
            // The system refusing: the switch goes back off (it shows what is).
            if let Err(why) = apply(scope.settings) {
                tracing::warn!(error = %why, "the computer can't be kept awake");
                save_on(scope.settings, false);
                let _ = apply(scope.settings);
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
