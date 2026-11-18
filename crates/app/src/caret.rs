//! The caret's own look (Settings → Editor → "Cursor"): the theme's (the default), or the
//! user's — its shape, thickness, blinking, gliding, halo and trail — over every theme.
//! Declared in `settings.rs`; each window draws with it (`Workbench::caret_look`).

use std::sync::RwLock;

use i18n::{t, tr};
use settings_model::SettingsService;
use theming::caret::{Blink, CaretLook, CaretShape, Motion};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

use crate::settings::{
    CARET_BLINK, CARET_GLOW, CARET_MOTION, CARET_OWN, CARET_SHAPE, CARET_TRAIL, CARET_WIDTH,
};

/// The user's look, when they chose their own (`None`: the theme's).
static OWN: RwLock<Option<CaretLook>> = RwLock::new(None);

/// The user's caret, if they chose their own over the theme's.
pub fn own() -> Option<CaretLook> {
    OWN.read().ok().and_then(|o| *o)
}

/// The look saved in the settings (whether or not it's used).
fn saved(settings: &SettingsService) -> CaretLook {
    CaretLook {
        shape: CaretShape::parse(&settings.get(&CARET_SHAPE)).unwrap_or_default(),
        width: settings.get(&CARET_WIDTH) as f32,
        blink: Blink::parse(&settings.get(&CARET_BLINK)).unwrap_or_default(),
        glow: settings.get(&CARET_GLOW) as f32 / 100.0,
        trail_ms: settings.get(&CARET_TRAIL) as u32,
        motion: Motion::parse(&settings.get(&CARET_MOTION)).unwrap_or_default(),
    }
}

const SHAPES: [(&str, &str); 3] = [
    ("bar", "settings.caret_bar"),
    ("block", "settings.caret_block"),
    ("underline", "settings.caret_underline"),
];
const BLINKS: [(&str, &str); 3] = [
    ("none", "settings.caret_blink_none"),
    ("classic", "settings.caret_blink_classic"),
    ("smooth", "settings.caret_blink_smooth"),
];
const MOTIONS: [(&str, &str); 2] =
    [("jump", "settings.caret_jump"), ("glide", "settings.caret_glide")];
const GLOWS: [i64; 5] = [0, 25, 50, 75, 100];
const TRAILS: [i64; 5] = [0, 120, 220, 400, 700];

/// Choices from `(saved value, its label's key)`: their place is the value chosen.
fn choices(options: &[(&str, &str)]) -> Vec<(i64, String)> {
    options.iter().enumerate().map(|(i, (_, key))| (i as i64, t(key))).collect()
}

fn place(options: &[(&str, &str)], value: &str) -> i64 {
    options.iter().position(|(v, _)| *v == value).unwrap_or(0) as i64
}

/// The "Cursor" block, in the settings' "Editor" section.
#[derive(Debug)]
pub struct CaretSettings;

impl SettingsContribution for CaretSettings {
    fn id(&self) -> &'static str {
        "caret"
    }

    fn page(&self) -> &'static str {
        "editor"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::EDITOR)
    }

    fn order(&self) -> u16 {
        10
    }

    fn apply(&self, settings: &SettingsService) {
        let look = settings.get(&CARET_OWN).then(|| saved(settings));
        if let Ok(mut own) = OWN.write() {
            *own = look;
        }
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let own = settings.get(&CARET_OWN);
        let mut rows = vec![SettingsRow::Choices {
            key: "own",
            detail: t("settings.caret_source_detail"),
            default: Some(i64::from(CARET_OWN.default_value())),
            label: t("settings.caret_source"),
            choices: vec![(0, t("settings.caret_theme")), (1, t("settings.caret_own"))],
            current: i64::from(own),
        }];
        if own {
            rows.extend([
                SettingsRow::Choices {
                    key: "shape",
                    detail: t("settings.caret_shape_detail"),
                    default: Some(place(&SHAPES, &CARET_SHAPE.default_value())),
                    label: t("settings.caret_shape"),
                    choices: choices(&SHAPES),
                    current: place(&SHAPES, &settings.get(&CARET_SHAPE)),
                },
                SettingsRow::Slider {
                    key: "width",
                    detail: t("settings.caret_width_detail"),
                    default: Some(CARET_WIDTH.default_value()),
                    label: t("settings.caret_width"),
                    stops: (1..=4).map(|w| (w, format!("{w} px"))).collect(),
                    current: settings.get(&CARET_WIDTH),
                },
                SettingsRow::Choices {
                    key: "blink",
                    detail: t("settings.caret_blink_detail"),
                    default: Some(place(&BLINKS, &CARET_BLINK.default_value())),
                    label: t("settings.caret_blink"),
                    choices: choices(&BLINKS),
                    current: place(&BLINKS, &settings.get(&CARET_BLINK)),
                },
                SettingsRow::Choices {
                    key: "motion",
                    detail: t("settings.caret_motion_detail"),
                    default: Some(place(&MOTIONS, &CARET_MOTION.default_value())),
                    label: t("settings.caret_motion"),
                    choices: choices(&MOTIONS),
                    current: place(&MOTIONS, &settings.get(&CARET_MOTION)),
                },
                SettingsRow::Slider {
                    key: "glow",
                    detail: t("settings.caret_glow_detail"),
                    default: Some(CARET_GLOW.default_value()),
                    label: t("settings.caret_glow"),
                    stops: GLOWS.iter().map(|g| (*g, format!("{g}%"))).collect(),
                    current: settings.get(&CARET_GLOW),
                },
                SettingsRow::Slider {
                    key: "trail",
                    detail: t("settings.caret_trail_detail"),
                    default: Some(CARET_TRAIL.default_value()),
                    label: t("settings.caret_trail"),
                    stops: TRAILS
                        .iter()
                        .map(|ms| {
                            (
                                *ms,
                                if *ms == 0 {
                                    t("settings.caret_none")
                                } else {
                                    tr!("settings.caret_ms", ms = *ms)
                                },
                            )
                        })
                        .collect(),
                    current: settings.get(&CARET_TRAIL),
                },
            ]);
        }
        SettingsBlock { title: t("settings.caret"), detail: t("settings.caret_detail"), rows }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let pick = |options: &[(&str, &str)]| {
            options.get(value as usize).map_or_else(String::new, |(v, _)| (*v).to_owned())
        };
        let saved = match key {
            "own" => settings.set(&CARET_OWN, value != 0),
            "shape" => settings.set(&CARET_SHAPE, pick(&SHAPES)),
            "width" => settings.set(&CARET_WIDTH, value),
            "blink" => settings.set(&CARET_BLINK, pick(&BLINKS)),
            "motion" => settings.set(&CARET_MOTION, pick(&MOTIONS)),
            "glow" => settings.set(&CARET_GLOW, value),
            "trail" => settings.set(&CARET_TRAIL, value),
            _ => return,
        };
        if let Err(e) = saved {
            tracing::warn!("can't save the caret's setting: {e}");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/caret.rs"]
mod tests;
