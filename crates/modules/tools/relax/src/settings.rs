//! Relax mode's settings (`[relax]`): `scene` (a scene's id, or `rotate`: each in turn every
//! `rotate_minutes`), `speed` and `density` (percent: 50, 100, 150), `theme_colors` (the
//! theme's, else each scene's own), `clock`, `motion` (`system`: less when the system asks,
//! `reduced`, `full`), `fps` (30, or 60) and `idle_minutes` (started after that long without
//! input; 0: never, the default). Declared here, handed to the views by [`apply`] at startup
//! and after every change ([`config`]), shown in their own page under Interface
//! ([`RelaxSettings`]).

use std::sync::RwLock;

use i18n::t;
use settings_model::{Setting, SettingType, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsCard, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow,
    SettingsScope,
};

use crate::scene::SCENES;

/// `scene`'s value for "each in turn".
pub const ROTATE: &str = "rotate";
/// The minutes offered between two scenes.
pub const ROTATE_CHOICES: [i64; 4] = [1, 5, 15, 30];
/// The speeds and densities offered (percent).
pub const PERCENTS: [i64; 3] = [50, 100, 150];
/// The frame rates offered.
pub const FPS_CHOICES: [i64; 2] = [60, 30];
/// The idle minutes offered (0: never).
pub const IDLE_CHOICES: [i64; 5] = [0, 5, 10, 15, 30];

/// `[relax] scene`: a scene's id, or [`ROTATE`].
pub const SCENE: Setting<String> = Setting::new("relax", "scene", || SCENES[0].id.to_owned())
    .valid(|s| s == ROTATE || SCENES.iter().any(|i| i.id == s));
/// `[relax] rotate_minutes`.
pub const ROTATE_MINUTES: Setting<i64> =
    Setting::new("relax", "rotate_minutes", || 5).valid(|m| ROTATE_CHOICES.contains(m));
/// `[relax] speed` (percent).
pub const SPEED: Setting<i64> =
    Setting::new("relax", "speed", || 100).valid(|p| PERCENTS.contains(p));
/// `[relax] density` (percent).
pub const DENSITY: Setting<i64> =
    Setting::new("relax", "density", || 100).valid(|p| PERCENTS.contains(p));
/// `[relax] theme_colors`.
pub const THEME_COLORS: Setting<bool> = Setting::new("relax", "theme_colors", || true);
/// `[relax] clock`.
pub const CLOCK: Setting<bool> = Setting::new("relax", "clock", || false);
/// `[relax] motion`: `system`, `reduced` or `full`.
pub const MOTION: Setting<String> =
    Setting::new("relax", "motion", || "system".to_owned()).valid(|m| Motion::parse(m).is_some());
/// `[relax] fps`: 30 by default (the scenes move slowly: half the frames, the same calm;
/// 60 for those who want it smoother).
pub const FPS: Setting<i64> =
    Setting::new("relax", "fps", || 30).valid(|f| FPS_CHOICES.contains(f));
/// `[relax] idle_minutes` (0: never).
pub const IDLE_MINUTES: Setting<i64> =
    Setting::new("relax", "idle_minutes", || 0).valid(|m| IDLE_CHOICES.contains(m));

/// Which scene shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneChoice {
    /// Always this one (a [`crate::scene::SCENES`] id).
    Fixed(&'static str),
    /// Each in turn, every [`Config::rotate_minutes`].
    Rotate,
}

/// How much things move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Less when the system asks for less.
    System,
    /// Always less (slower, fewer frames, no fades).
    Reduced,
    /// As chosen.
    Full,
}

impl Motion {
    pub const ALL: [Self; 3] = [Self::System, Self::Reduced, Self::Full];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Reduced => "reduced",
            Self::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == s)
    }
}

/// What the views follow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub scene: SceneChoice,
    pub rotate_minutes: u32,
    /// The scenes' pace (1: as designed).
    pub speed: f32,
    /// How much they show (1: as designed).
    pub density: f32,
    /// The theme's colors (else each scene's own).
    pub theme_colors: bool,
    /// A large clock over the scene.
    pub clock: bool,
    pub motion: Motion,
    /// The most frames a second.
    pub fps: u32,
    /// Started after these minutes without input (`None`: never).
    pub idle_minutes: Option<u32>,
}

impl Config {
    pub const DEFAULT: Self = Self {
        scene: SceneChoice::Fixed("stars"),
        rotate_minutes: 5,
        speed: 1.0,
        density: 1.0,
        theme_colors: true,
        clock: false,
        motion: Motion::System,
        fps: 30,
        idle_minutes: None,
    };

    /// What `settings` say.
    pub fn read(settings: &SettingsService) -> Self {
        let scene = settings.get(&SCENE);
        let percent = |p: i64| p as f32 / 100.0;
        let idle = settings.get(&IDLE_MINUTES);
        Self {
            scene: match SCENES.iter().find(|i| i.id == scene) {
                Some(info) => SceneChoice::Fixed(info.id),
                None if scene == ROTATE => SceneChoice::Rotate,
                None => Self::DEFAULT.scene,
            },
            rotate_minutes: settings.get(&ROTATE_MINUTES) as u32,
            speed: percent(settings.get(&SPEED)),
            density: percent(settings.get(&DENSITY)),
            theme_colors: settings.get(&THEME_COLORS),
            clock: settings.get(&CLOCK),
            motion: Motion::parse(&settings.get(&MOTION)).unwrap_or(Motion::System),
            fps: settings.get(&FPS) as u32,
            idle_minutes: (idle > 0).then_some(idle as u32),
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

/// Uses `config` (what the settings say; a test's own).
pub fn set_config(config: Config) {
    if let Ok(mut current) = CONFIG.write() {
        *current = config;
    }
}

/// The settings' values handed to the views (and to the idle start).
pub fn apply(settings: &SettingsService) {
    set_config(Config::read(settings));
}

/// `value` saved (said in the log when it can't be).
fn save<T: SettingType>(settings: &SettingsService, setting: &Setting<T>, value: T) {
    if !setting.accepts(&value) {
        return;
    }
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!(error = %e, "can't save a Relax mode setting");
    }
}

/// The scene choice's values: each scene by its place, then "in turn".
fn scene_value(saved: &str) -> i64 {
    SCENES.iter().position(|i| i.id == saved).map_or(SCENES.len() as i64, |i| i as i64)
}

/// The motion choice's values (their place in [`Motion::ALL`]).
fn motion_value(saved: &str) -> i64 {
    Motion::ALL.iter().position(|m| m.id() == saved).unwrap_or(0) as i64
}

/// The "Relax mode" page (Interface).
#[derive(Debug, Default)]
pub struct RelaxSettings;

impl SettingsContribution for RelaxSettings {
    fn id(&self) -> &'static str {
        "relax"
    }

    fn page(&self) -> &'static str {
        "relax"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "relax",
            label: "relax.settings_title",
            icon: crate::ICON,
            group: SettingsGroup::Interface,
            order: 80,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let mut cards: Vec<SettingsCard> = SCENES
            .iter()
            .enumerate()
            .map(|(i, info)| SettingsCard {
                value: i as i64,
                label: t(info.name_key),
                tag: info.three_d.then(|| t("relax.tag_3d")),
            })
            .collect();
        cards.push(SettingsCard {
            value: SCENES.len() as i64,
            label: t("relax.scene_rotate"),
            tag: None,
        });
        let minutes = |m: i64| i18n::tr!("relax.minutes", count = m);
        let percents = |keys: [&str; 3]| -> Vec<(i64, String)> {
            PERCENTS.into_iter().zip(keys).map(|(p, k)| (p, t(k))).collect()
        };
        let motions = vec![
            (0, t("relax.motion_system")),
            (1, t("relax.motion_on")),
            (2, t("relax.motion_off")),
        ];
        let mut rows = vec![SettingsRow::Cards {
            key: "scene",
            label: t("relax.scene"),
            detail: t("relax.scene_detail"),
            cards,
            current: scene_value(&s.get(&SCENE)),
            default: Some(scene_value(&SCENE.default_value())),
            preview: crate::preview::PREVIEW,
        }];
        if s.get(&SCENE) == ROTATE {
            rows.push(SettingsRow::Segmented {
                key: "rotate_minutes",
                label: t("relax.rotate"),
                detail: t("relax.rotate_detail"),
                choices: ROTATE_CHOICES.into_iter().map(|m| (m, minutes(m))).collect(),
                current: s.get(&ROTATE_MINUTES),
                default: Some(ROTATE_MINUTES.default_value()),
            });
        }
        rows.extend([
            SettingsRow::Segmented {
                key: "speed",
                label: t("relax.speed"),
                detail: t("relax.speed_detail"),
                choices: percents(["relax.speed_slow", "relax.speed_normal", "relax.speed_fast"]),
                current: s.get(&SPEED),
                default: Some(SPEED.default_value()),
            },
            SettingsRow::Segmented {
                key: "density",
                label: t("relax.density"),
                detail: t("relax.density_detail"),
                choices: percents([
                    "relax.density_low",
                    "relax.density_normal",
                    "relax.density_high",
                ]),
                current: s.get(&DENSITY),
                default: Some(DENSITY.default_value()),
            },
            SettingsRow::Toggle {
                key: "theme_colors",
                label: t("relax.colors"),
                detail: t("relax.colors_detail"),
                on: s.get(&THEME_COLORS),
                default: Some(THEME_COLORS.default_value()),
            },
            SettingsRow::Toggle {
                key: "clock",
                label: t("relax.clock"),
                detail: t("relax.clock_detail"),
                on: s.get(&CLOCK),
                default: Some(CLOCK.default_value()),
            },
            SettingsRow::Segmented {
                key: "motion",
                label: t("relax.motion"),
                detail: t("relax.motion_detail"),
                choices: motions,
                current: motion_value(&s.get(&MOTION)),
                default: Some(motion_value(&MOTION.default_value())),
            },
            SettingsRow::Segmented {
                key: "fps",
                label: t("relax.fps"),
                detail: t("relax.fps_detail"),
                choices: FPS_CHOICES.into_iter().map(|f| (f, f.to_string())).collect(),
                current: s.get(&FPS),
                default: Some(FPS.default_value()),
            },
            SettingsRow::Choices {
                key: "idle_minutes",
                label: t("relax.idle"),
                detail: t("relax.idle_detail"),
                choices: IDLE_CHOICES
                    .into_iter()
                    .map(|m| (m, if m == 0 { t("relax.idle_never") } else { minutes(m) }))
                    .collect(),
                current: s.get(&IDLE_MINUTES),
                default: Some(IDLE_MINUTES.default_value()),
            },
            SettingsRow::Actions {
                text: t("relax.preview_detail"),
                buttons: vec![("preview", t("relax.preview"))],
            },
        ]);
        SettingsBlock { title: t("relax.settings_title"), detail: t("relax.settings_detail"), rows }
    }

    /// "Preview": Relax mode over the window, as chosen (Esc returns to the settings).
    fn command(&self, action: &'static str) -> Option<command_model::CommandId> {
        (action == "preview").then_some(crate::START)
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "scene" => {
                let id = SCENES.get(value as usize).map_or(ROTATE, |i| i.id);
                save(s, &SCENE, id.to_owned());
            }
            "rotate_minutes" => save(s, &ROTATE_MINUTES, value),
            "speed" => save(s, &SPEED, value),
            "density" => save(s, &DENSITY, value),
            "theme_colors" => save(s, &THEME_COLORS, value != 0),
            "clock" => save(s, &CLOCK, value != 0),
            "motion" => {
                let motion = Motion::ALL.get(value as usize).copied().unwrap_or(Motion::System);
                save(s, &MOTION, motion.id().to_owned());
            }
            "fps" => save(s, &FPS, value),
            "idle_minutes" => save(s, &IDLE_MINUTES, value),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
