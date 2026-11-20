//! The backdrop under the windows over the editor (Settings → Appearance → "Overlay
//! backdrop"): the theme's (the default), or the user's style, tint opacity and blur over
//! every theme. Declared in `settings.rs`; each window's palette gets it
//! (`Workbench::set_palette`), so a choice is seen at once — under the settings themselves.

use std::sync::RwLock;

use i18n::{t, tr};
use settings_model::SettingsService;
use ui_workbench::backdrop::{BackdropKind, BackdropRole, BackdropSpec, BackdropStyle, MAX_BLUR};
use workbench_model::{SettingsBlock, SettingsContribution, SettingsRow, SettingsScope};

use crate::settings::{BACKDROP_BLUR, BACKDROP_OPACITY, BACKDROP_STYLE, FROM_THEME};

/// What the user chose over the theme's (nothing said: the theme's).
#[cfg(not(test))]
static OWN: RwLock<BackdropSpec> =
    RwLock::new(BackdropSpec { style: None, color: None, opacity: None, blur: None });

#[cfg(test)]
thread_local! {
    // As the settings in the tests (`config::settings`): each test thread its own, so other
    // tests loading the modules (applying their settings) never change this one's.
    static OWN: RwLock<BackdropSpec> = const {
        RwLock::new(BackdropSpec { style: None, color: None, opacity: None, blur: None })
    };
}

/// `f` with the user's choice (the process's; a test thread's own in the tests).
fn with_own<R>(f: impl FnOnce(&RwLock<BackdropSpec>) -> R) -> R {
    #[cfg(not(test))]
    return f(&OWN);
    #[cfg(test)]
    OWN.with(f)
}

/// The theme's own backdrop (the window's last palette's), for the sliders' "from the
/// theme" place.
static THEME: RwLock<Option<BackdropStyle>> = RwLock::new(None);

/// The user's choice, as last applied.
pub fn own() -> BackdropSpec {
    with_own(|own| own.read().map(|o| *o).unwrap_or_default())
}

/// Sets the user's choice (a snapshot's own, not saved).
#[cfg(feature = "snapshot")]
pub fn set_own(spec: BackdropSpec) {
    with_own(|own| {
        if let Ok(mut own) = own.write() {
            *own = spec;
        }
    });
}

/// A choice as written for a snapshot: `<style>[:<blur>[:<tint percent>]]`.
#[cfg(any(test, feature = "snapshot"))]
pub fn parse_own(text: &str) -> BackdropSpec {
    let mut parts = text.split(':');
    BackdropSpec {
        style: parts.next().and_then(BackdropKind::parse),
        blur: parts.next().and_then(|b| b.parse::<f32>().ok()).map(|b| b.clamp(0.0, MAX_BLUR)),
        opacity: parts
            .next()
            .and_then(|o| o.parse::<f32>().ok())
            .map(|o| (o / 100.0).clamp(0.0, 0.85)),
        color: None,
    }
}

/// The style a window draws with: the theme's, the user's over it, never blurred where the
/// renderer can't blur.
pub fn style(theme: &BackdropStyle, blur: bool) -> BackdropStyle {
    if let Ok(mut t) = THEME.write() {
        *t = Some(theme.with_user(BackdropSpec::default()));
    }
    let style = theme.with_user(own());
    if blur { style } else { style.without_blur() }
}

/// The choice saved in the settings.
fn saved(settings: &SettingsService) -> BackdropSpec {
    let style = settings.get(&BACKDROP_STYLE);
    let opacity = settings.get(&BACKDROP_OPACITY);
    let blur = settings.get(&BACKDROP_BLUR);
    BackdropSpec {
        style: BackdropKind::parse(&style),
        color: None,
        opacity: (opacity != FROM_THEME).then(|| opacity as f32 / 100.0),
        blur: (blur != FROM_THEME).then_some(blur as f32),
    }
}

/// The theme's own tint (a dialog's, percent) and blur (logical pixels), as the sliders show
/// them.
fn theme_values() -> (i64, i64) {
    let theme = THEME.read().ok().and_then(|t| *t).unwrap_or_default();
    let opacity = (theme.opacity(BackdropRole::Dialog) * 100.0).round() as i64;
    (opacity, theme.blur.round() as i64)
}

/// The style's choices: the theme's, then each kind.
const STYLES: [(&str, &str); 3] = [
    ("theme", "settings.backdrop_from_theme"),
    ("simple", "settings.backdrop_simple"),
    ("glass", "settings.backdrop_glass"),
];

/// The tint's stops, percent.
const OPACITIES: [i64; 13] = [0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60];

/// The "Overlay backdrop" block, in Appearance.
#[derive(Debug)]
pub struct BackdropSettings;

impl SettingsContribution for BackdropSettings {
    fn id(&self) -> &'static str {
        "backdrop"
    }

    fn page(&self) -> &'static str {
        "appearance"
    }

    fn order(&self) -> u16 {
        150
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
        let style = settings.get(&BACKDROP_STYLE);
        let (theme_opacity, theme_blur) = theme_values();
        // The theme's own value is a stop like the others, said so; "from the theme" is
        // shown where it is.
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
            title: t("settings.backdrop"),
            detail: t("settings.backdrop_detail"),
            rows: vec![
                SettingsRow::Choices {
                    key: "style",
                    detail: t("settings.backdrop_style_detail"),
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
                    detail: t("settings.backdrop_opacity_detail"),
                    default: Some(theme_opacity),
                    label: t("settings.backdrop_opacity"),
                    stops: opacities,
                    current: shown(settings.get(&BACKDROP_OPACITY), theme_opacity),
                },
                SettingsRow::Slider {
                    key: "blur",
                    detail: t("settings.backdrop_blur_detail"),
                    default: Some(theme_blur),
                    label: t("settings.backdrop_blur"),
                    stops: blurs,
                    current: shown(settings.get(&BACKDROP_BLUR), theme_blur),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let saved = match key {
            "style" => settings.set(
                &BACKDROP_STYLE,
                STYLES.get(value as usize).map_or("theme", |(id, _)| id).to_owned(),
            ),
            // The theme's own value: following the theme again.
            "opacity" => settings
                .set(&BACKDROP_OPACITY, if value == theme_values().0 { FROM_THEME } else { value }),
            "blur" => settings
                .set(&BACKDROP_BLUR, if value == theme_values().1 { FROM_THEME } else { value }),
            _ => return,
        };
        if let Err(e) = saved {
            tracing::warn!(error = %e, "can't save the overlay backdrop's setting");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/backdrop.rs"]
mod tests;
