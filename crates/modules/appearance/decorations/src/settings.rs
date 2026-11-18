//! The decorations' settings (`[appearance]`): whether they're drawn — `decorations`: `on`
//! (moving while the window is in use), `static` (one still frame) or `off` (nothing drawn,
//! nothing kept) — and the user's own, drawn with any theme: `my_decorations` (`none`, a
//! preset's id, or `custom`: the layers saved from the studio, `my_decorations_layers`, one
//! a line) over the theme's or instead of them (`my_decorations_with`: `add` / `replace`).
//! How they move: `decorations_fps` (the most frames a second: 10, 15, 20 or 30),
//! `decorations_idle` (seconds without input before they pause: 20, 60, 300, or 0: never;
//! 10, offered before, still read) and `decorations_follow_system` (still when the system asks for less motion or
//! saves power). Shown in Settings → Appearance ("Decorations") and in the studio's
//! "Performance and accessibility"; every window follows a change at once (its decorations
//! are made again: [`version`]).

use std::sync::RwLock;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use i18n::t;
use settings_model::{Setting, SettingsService};
use theming::decorations::{LayerSpec, MAX_LAYERS, OverlayMode, decode, presets};
use ui_components::decorations::{DecorationMode, FPS, FPS_CHOICES};
use workbench_model::{SettingsBlock, SettingsContribution, SettingsRow, SettingsScope};

/// `[appearance] decorations`.
pub const DECORATIONS: Setting<String> =
    Setting::new("appearance", "decorations", || DecorationMode::On.id().to_owned())
        .valid(|s| DecorationMode::parse(s).is_some());

/// `[appearance] my_decorations`: `none`, a preset's id or `custom`.
pub const MY_DECORATIONS: Setting<String> =
    Setting::new("appearance", "my_decorations", || NONE.to_owned())
        .valid(|s| Mine::parse(s).is_some());

/// `[appearance] my_decorations_with`: `add` (over the theme's) or `replace`.
pub const MY_DECORATIONS_WITH: Setting<String> =
    Setting::new("appearance", "my_decorations_with", || OverlayMode::Add.id().to_owned())
        .valid(|s| OverlayMode::parse(s).is_some());

/// `[appearance] my_decorations_layers`: the user's layers, one a line
/// (`theming::decorations::encode`).
pub const MY_DECORATIONS_LAYERS: Setting<Vec<String>> =
    Setting::new("appearance", "my_decorations_layers", Vec::new)
        .valid(|list| list.len() <= MAX_LAYERS && list.iter().all(|l| decode(l).is_ok()));

/// `[appearance] decorations_fps`: the most frames a second.
pub const DECORATIONS_FPS: Setting<i64> =
    Setting::new("appearance", "decorations_fps", || i64::from(FPS))
        .valid(|v| FPS_CHOICES.iter().any(|f| i64::from(*f) == *v));

/// The idle times offered (seconds; 0: never).
pub use ui_components::decorations::IDLE_CHOICES;

/// The idle times read (those offered, and 10 s offered before).
const IDLE_READ: [i64; 5] = [10, 20, 60, 300, 0];

/// `[appearance] decorations_idle`: seconds without input before they pause (0: never).
pub const DECORATIONS_IDLE: Setting<i64> =
    Setting::new("appearance", "decorations_idle", || 20).valid(|v| IDLE_READ.contains(v));

/// `[appearance] decorations_follow_system`: still when the system asks for less motion or
/// saves power.
pub const DECORATIONS_FOLLOW_SYSTEM: Setting<bool> =
    Setting::new("appearance", "decorations_follow_system", || true);

const NONE: &str = "none";
const CUSTOM: &str = "custom";

/// Which of the user's own decorations are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mine {
    /// None: only the theme's.
    #[default]
    None,
    /// A preset's (its id).
    Preset(&'static str),
    /// The layers saved from the studio.
    Custom,
}

impl Mine {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            NONE => Some(Self::None),
            CUSTOM => Some(Self::Custom),
            id => presets::ALL.iter().find(|p| **p == id).map(|p| Self::Preset(p)),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::None => NONE,
            Self::Preset(id) => id,
            Self::Custom => CUSTOM,
        }
    }
}

/// The user's own decorations, as chosen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pub mine: Mine,
    pub with: OverlayMode,
    /// The layers saved from the studio.
    pub custom: Vec<LayerSpec>,
}

impl Overlay {
    /// The layers to draw for a dark or a light theme (none: only the theme's).
    pub fn layers(&self, dark: bool) -> Option<Vec<LayerSpec>> {
        match self.mine {
            Mine::None => None,
            Mine::Preset(id) => presets::layers(id, dark),
            Mine::Custom => (!self.custom.is_empty()).then(|| self.custom.clone()),
        }
    }
}

/// The mode in use (its place in [`DecorationMode::ALL`]), the user's own and how many
/// times either changed.
static MODE: AtomicU8 = AtomicU8::new(0);
static OVERLAY: RwLock<Option<Overlay>> = RwLock::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);
/// How they move (the frame rate, the idle seconds — 0: never —, following the system).
static PACE: RwLock<(u32, i64, bool)> = RwLock::new((FPS, 20, true));

/// The most frames a second.
pub fn fps() -> u32 {
    PACE.read().map_or(FPS, |p| p.0)
}

/// How long without input before they stand still (`None`: never).
pub fn idle() -> Option<std::time::Duration> {
    let secs = PACE.read().map_or(20, |p| p.1);
    (secs > 0).then(|| std::time::Duration::from_secs(secs as u64))
}

/// Whether they stand still when the system asks for less motion or saves power.
pub fn follows_system() -> bool {
    PACE.read().map_or(true, |p| p.2)
}

/// Uses this pace (what the settings say; a test's own).
pub fn set_pace(fps: u32, idle_secs: i64, follow_system: bool) {
    let Ok(mut pace) = PACE.write() else { return };
    let new = (fps.clamp(1, 30), idle_secs.max(0), follow_system);
    if *pace != new {
        *pace = new;
        VERSION.fetch_add(1, Ordering::Relaxed);
    }
}

/// What the user chose.
pub fn mode() -> DecorationMode {
    DecorationMode::ALL.get(usize::from(MODE.load(Ordering::Relaxed))).copied().unwrap_or_default()
}

/// The user's own decorations.
pub fn overlay() -> Overlay {
    OVERLAY.read().ok().and_then(|o| o.clone()).unwrap_or_default()
}

/// Changes whenever the mode or the user's own decorations do.
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// Uses `mode` (what the setting says; a snapshot's or a test's own).
pub fn set_mode(mode: DecorationMode) {
    let at = DecorationMode::ALL.iter().position(|m| *m == mode).unwrap_or(0) as u8;
    if MODE.swap(at, Ordering::Relaxed) != at {
        VERSION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Uses `overlay` (what the settings say; a snapshot's or a test's own).
pub fn set_overlay(overlay: Overlay) {
    let Ok(mut current) = OVERLAY.write() else { return };
    if current.as_ref() != Some(&overlay) {
        *current = Some(overlay);
        VERSION.fetch_add(1, Ordering::Relaxed);
    }
}

fn saved(settings: &SettingsService) -> DecorationMode {
    DecorationMode::parse(&settings.get(&DECORATIONS)).unwrap_or_default()
}

fn saved_overlay(settings: &SettingsService) -> Overlay {
    Overlay {
        mine: Mine::parse(&settings.get(&MY_DECORATIONS)).unwrap_or_default(),
        with: OverlayMode::parse(&settings.get(&MY_DECORATIONS_WITH)).unwrap_or_default(),
        custom: settings
            .get(&MY_DECORATIONS_LAYERS)
            .iter()
            .filter_map(|l| decode(l).ok())
            .collect(),
    }
}

fn label(mode: DecorationMode) -> String {
    t(match mode {
        DecorationMode::On => "settings.decorations_on",
        DecorationMode::Static => "settings.decorations_static",
        DecorationMode::Off => "settings.decorations_off",
    })
}

/// An idle time's name (`never` for 0).
pub use ui_components::decorations::idle_label;

/// A preset's name (`decorations.preset_<id>`).
pub fn preset_name(id: &str) -> String {
    t(&format!("decorations.preset_{}", id.replace('-', "_")))
}

/// The choices of "My decorations": none, each preset, the studio's (when saved).
fn mine_choices(custom: bool) -> Vec<Mine> {
    let mut out = vec![Mine::None];
    out.extend(presets::ALL.iter().map(|p| Mine::Preset(p)));
    if custom {
        out.push(Mine::Custom);
    }
    out
}

fn mine_label(mine: Mine) -> String {
    match mine {
        Mine::None => t("settings.decorations_theme_only"),
        Mine::Preset(id) => preset_name(id),
        Mine::Custom => t("settings.decorations_custom"),
    }
}

/// The requests changing the decorations: as if chosen on their settings page.
pub(crate) fn answers() -> [workbench_model::Answers; 2] {
    use contracts::decorations::{DecorationPace, SetDecorationPace, SetOwnDecorations};
    let scope = |settings| SettingsScope {
        settings,
        project: None,
        project_data: None,
        language: None,
        wake: None,
    };
    [
        workbench_model::Answers::of(move |asked: SetOwnDecorations, host| {
            if let Some(settings) = host.user_settings() {
                DecorationSettings.set_text(scope(settings), "custom", &asked.layers);
                DecorationSettings.apply(settings);
            }
        }),
        workbench_model::Answers::of(move |asked: SetDecorationPace, host| {
            let key = match asked.what {
                DecorationPace::Mode => "mode",
                DecorationPace::Fps => "fps",
                DecorationPace::Idle => "idle",
                DecorationPace::FollowSystem => "follow_system",
            };
            if let Some(settings) = host.user_settings() {
                DecorationSettings.choose(scope(settings), key, asked.value);
                DecorationSettings.apply(settings);
            }
        }),
    ]
}

/// The "Decorations" block, in the settings' "Appearance" section.
#[derive(Debug)]
pub struct DecorationSettings;

impl SettingsContribution for DecorationSettings {
    fn id(&self) -> &'static str {
        "decorations"
    }

    fn page(&self) -> &'static str {
        "appearance"
    }

    fn order(&self) -> u16 {
        20
    }

    fn apply(&self, settings: &SettingsService) {
        set_mode(saved(settings));
        set_overlay(saved_overlay(settings));
        set_pace(
            u32::try_from(settings.get(&DECORATIONS_FPS)).unwrap_or(FPS),
            settings.get(&DECORATIONS_IDLE),
            settings.get(&DECORATIONS_FOLLOW_SYSTEM),
        );
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let current = saved(scope.settings);
        let choices =
            DecorationMode::ALL.iter().enumerate().map(|(i, m)| (i as i64, label(*m))).collect();
        let current = DecorationMode::ALL.iter().position(|m| *m == current).unwrap_or(0) as i64;
        let overlay = saved_overlay(scope.settings);
        let mine = mine_choices(!overlay.custom.is_empty());
        let mut rows = vec![
            SettingsRow::Segmented {
                key: "mode",
                detail: t("settings.decorations_mode_detail"),
                default: DecorationMode::ALL
                    .iter()
                    .position(|m| Some(*m) == DecorationMode::parse(&DECORATIONS.default_value()))
                    .map(|i| i as i64),
                label: t("settings.decorations_mode"),
                choices,
                current,
            },
            SettingsRow::Choices {
                key: "mine",
                detail: t("settings.decorations_mine_detail"),
                default: mine
                    .iter()
                    .position(|m| Some(*m) == Mine::parse(&MY_DECORATIONS.default_value()))
                    .map(|i| i as i64),
                label: t("settings.decorations_mine"),
                choices: mine.iter().enumerate().map(|(i, m)| (i as i64, mine_label(*m))).collect(),
                current: mine.iter().position(|m| *m == overlay.mine).unwrap_or(0) as i64,
            },
        ];
        if overlay.mine != Mine::None {
            rows.push(SettingsRow::Segmented {
                key: "with",
                detail: t("settings.decorations_with_detail"),
                default: OverlayMode::ALL
                    .iter()
                    .position(|m| {
                        Some(*m) == OverlayMode::parse(&MY_DECORATIONS_WITH.default_value())
                    })
                    .map(|i| i as i64),
                label: t("settings.decorations_with"),
                choices: vec![
                    (0, t("settings.decorations_add")),
                    (1, t("settings.decorations_replace")),
                ],
                current: OverlayMode::ALL.iter().position(|m| *m == overlay.with).unwrap_or(0)
                    as i64,
            });
        }
        rows.push(SettingsRow::Segmented {
            key: "fps",
            detail: t("settings.decorations_fps_detail"),
            default: Some(DECORATIONS_FPS.default_value()),
            label: t("settings.decorations_fps"),
            choices: FPS_CHOICES.iter().map(|f| (i64::from(*f), f.to_string())).collect(),
            current: scope.settings.get(&DECORATIONS_FPS),
        });
        rows.push(SettingsRow::Segmented {
            key: "idle",
            detail: t("settings.decorations_idle_detail"),
            default: Some(DECORATIONS_IDLE.default_value()),
            label: t("settings.decorations_idle"),
            choices: IDLE_CHOICES.iter().map(|s| (*s, idle_label(*s))).collect(),
            current: scope.settings.get(&DECORATIONS_IDLE),
        });
        rows.push(SettingsRow::Toggle {
            key: "follow_system",
            detail: t("settings.decorations_follow_system_detail"),
            default: Some(DECORATIONS_FOLLOW_SYSTEM.default_value()),
            label: t("settings.decorations_follow_system"),
            on: scope.settings.get(&DECORATIONS_FOLLOW_SYSTEM),
        });
        SettingsBlock {
            title: t("settings.decorations"),
            detail: t("settings.decorations_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let at = usize::try_from(value).ok();
        let saved = match key {
            "mode" => {
                let Some(mode) = at.and_then(|i| DecorationMode::ALL.get(i)) else { return };
                let saved = settings.set(&DECORATIONS, mode.id().to_owned());
                if saved.is_ok() {
                    set_mode(*mode);
                }
                saved
            }
            "mine" => {
                let custom = !saved_overlay(settings).custom.is_empty();
                let Some(mine) = at.and_then(|i| mine_choices(custom).get(i).copied()) else {
                    return;
                };
                settings.set(&MY_DECORATIONS, mine.id().to_owned())
            }
            "with" => {
                let Some(with) = at.and_then(|i| OverlayMode::ALL.get(i)) else { return };
                settings.set(&MY_DECORATIONS_WITH, with.id().to_owned())
            }
            "fps" | "idle" | "follow_system" => {
                let saved = match key {
                    "fps" => settings.set(&DECORATIONS_FPS, value),
                    "idle" => settings.set(&DECORATIONS_IDLE, value),
                    _ => settings.set(&DECORATIONS_FOLLOW_SYSTEM, value != 0),
                };
                match saved {
                    Ok(()) => self.apply(settings),
                    Err(e) => tracing::warn!("can't save the decorations' pace: {e}"),
                }
                return;
            }
            _ => return,
        };
        match saved {
            Ok(()) => set_overlay(saved_overlay(settings)),
            Err(e) => tracing::warn!("can't save the decorations' setting: {e}"),
        }
    }

    /// `custom`: the studio's layers (one a line), saved as the user's own and drawn with
    /// every theme; nothing: none any more.
    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        if key != "custom" {
            return;
        }
        let settings = scope.settings;
        let layers: Vec<String> = value
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .filter(|l| {
                decode(l).inspect_err(|e| tracing::warn!("a decoration layer refused: {e}")).is_ok()
            })
            .take(MAX_LAYERS)
            .map(str::to_owned)
            .collect();
        let mine = if layers.is_empty() { NONE } else { CUSTOM };
        let saved = settings
            .set(&MY_DECORATIONS_LAYERS, layers)
            .and_then(|()| settings.set(&MY_DECORATIONS, mine.to_owned()));
        match saved {
            Ok(()) => set_overlay(saved_overlay(settings)),
            Err(e) => tracing::warn!("can't save your decorations: {e}"),
        }
    }
}
