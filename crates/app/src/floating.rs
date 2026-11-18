//! The floating panels' glass (Settings → Appearance → "Menu panel"): the theme's (the
//! default: `[floating]`), or the user's style, opacity and blur over every theme. Only the
//! menu drawer's panel uses it now; any floating panel may later. Declared in
//! `settings.rs`; each window's palette gets it (`Workbench::set_palette`), so a choice is
//! seen at once.

use std::sync::RwLock;

use i18n::{t, tr};
use settings_model::SettingsService;
use ui_workbench::backdrop::{BackdropKind, MAX_BLUR, SurfaceSpec, SurfaceStyle};
use workbench_model::{SettingsBlock, SettingsContribution, SettingsRow, SettingsScope};

use crate::settings::{FLOATING_BLUR, FLOATING_OPACITY, FLOATING_STYLE, FROM_THEME};

/// What the user chose over the theme's (nothing said: the theme's).
#[cfg(not(test))]
static OWN: RwLock<SurfaceSpec> =
    RwLock::new(SurfaceSpec { style: None, color: None, opacity: None, blur: None });

#[cfg(test)]
thread_local! {
    // As the settings in the tests: each test thread its own.
    static OWN: RwLock<SurfaceSpec> = const {
        RwLock::new(SurfaceSpec { style: None, color: None, opacity: None, blur: None })
    };
}

fn with_own<R>(f: impl FnOnce(&RwLock<SurfaceSpec>) -> R) -> R {
    #[cfg(not(test))]
    return f(&OWN);
    #[cfg(test)]
    OWN.with(f)
}

/// The theme's own glass (the window's last palette's), for the sliders' "from the theme".
static THEME: RwLock<Option<SurfaceStyle>> = RwLock::new(None);

/// The user's choice, as last applied.
pub fn own() -> SurfaceSpec {
    with_own(|own| own.read().map(|o| *o).unwrap_or_default())
}

/// The style a window draws with: the theme's, the user's over it, never glass where the
/// renderer can't blur (opaque then).
pub fn style(theme: &SurfaceStyle, blur: bool) -> SurfaceStyle {
    if let Ok(mut t) = THEME.write() {
        *t = Some(theme.with_user(SurfaceSpec::default()));
    }
    let style = theme.with_user(own());
    if blur { style } else { style.without_blur() }
}

/// The choice saved in the settings.
fn saved(settings: &SettingsService) -> SurfaceSpec {
    let opacity = settings.get(&FLOATING_OPACITY);
    let blur = settings.get(&FLOATING_BLUR);
    SurfaceSpec {
        style: BackdropKind::parse(&settings.get(&FLOATING_STYLE)),
        color: None,
        opacity: (opacity != FROM_THEME).then(|| opacity as f32 / 100.0),
        blur: (blur != FROM_THEME).then_some(blur as f32),
    }
}

/// The theme's own opacity (percent) and blur (logical pixels), as the sliders show them.
fn theme_values() -> (i64, i64) {
    let theme = THEME.read().ok().and_then(|t| *t).unwrap_or_default();
    ((theme.opacity * 100.0).round() as i64, theme.blur.round() as i64)
}

/// The style's choices: the theme's, then each kind.
const STYLES: [(&str, &str); 3] = [
    ("theme", "settings.backdrop_from_theme"),
    ("simple", "settings.backdrop_simple"),
    ("glass", "settings.backdrop_glass"),
];

/// The opacity's stops, percent.
const OPACITIES: [i64; 9] = [60, 65, 70, 75, 80, 85, 90, 95, 100];

/// The "Menu panel" block, in Appearance, right after the overlay backdrop's.
#[derive(Debug)]
pub struct FloatingSettings;

impl SettingsContribution for FloatingSettings {
    fn id(&self) -> &'static str {
        "floating"
    }

    fn page(&self) -> &'static str {
        "appearance"
    }

    fn order(&self) -> u16 {
        155
    }

    fn apply(&self, settings: &SettingsService) {
        with_own(|own| {
            if let Ok(mut own) = own.write() {
                *own = saved(settings);
            }
        });
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let style = settings.get(&FLOATING_STYLE);
        let (theme_opacity, theme_blur) = theme_values();
        let mark = |label: String, theirs: bool| {
            if theirs { tr!("settings.backdrop_theirs", value = label.as_str()) } else { label }
        };
        let mut opacities: Vec<i64> = OPACITIES.to_vec();
        if !opacities.contains(&theme_opacity) {
            opacities.push(theme_opacity);
            opacities.sort_unstable();
        }
        let opacities =
            opacities.into_iter().map(|o| (o, mark(format!("{o}%"), o == theme_opacity))).collect();
        let blurs = (0..=MAX_BLUR as i64)
            .map(|px| {
                let label = if px == 0 {
                    t("settings.backdrop_no_blur")
                } else {
                    tr!("settings.backdrop_px", n = px)
                };
                (px, mark(label, px == theme_blur))
            })
            .collect();
        let shown = |saved: i64, theirs: i64| if saved == FROM_THEME { theirs } else { saved };
        SettingsBlock {
            title: t("settings.floating"),
            detail: t("settings.floating_detail"),
            rows: vec![
                SettingsRow::Choices {
                    key: "style",
                    detail: t("settings.floating_style_detail"),
                    default: Some(0),
                    label: t("settings.backdrop_style"),
                    choices: STYLES
                        .iter()
                        .enumerate()
                        .map(|(i, (_, k))| (i as i64, t(k)))
                        .collect(),
                    current: STYLES.iter().position(|(id, _)| *id == style).unwrap_or(0) as i64,
                },
                SettingsRow::Slider {
                    key: "opacity",
                    detail: t("settings.floating_opacity_detail"),
                    default: Some(theme_opacity),
                    label: t("settings.floating_opacity"),
                    stops: opacities,
                    current: shown(settings.get(&FLOATING_OPACITY), theme_opacity),
                },
                SettingsRow::Slider {
                    key: "blur",
                    detail: t("settings.floating_blur_detail"),
                    default: Some(theme_blur),
                    label: t("settings.backdrop_blur"),
                    stops: blurs,
                    current: shown(settings.get(&FLOATING_BLUR), theme_blur),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let saved = match key {
            "style" => settings.set(
                &FLOATING_STYLE,
                STYLES.get(value as usize).map_or("theme", |(id, _)| id).to_owned(),
            ),
            // The theme's own value: following the theme again.
            "opacity" => settings
                .set(&FLOATING_OPACITY, if value == theme_values().0 { FROM_THEME } else { value }),
            "blur" => settings
                .set(&FLOATING_BLUR, if value == theme_values().1 { FROM_THEME } else { value }),
            _ => return,
        };
        if let Err(e) = saved {
            tracing::warn!("can't save the menu panel's setting: {e}");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/floating.rs"]
mod tests;
