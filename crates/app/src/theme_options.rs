//! The settings of the theme in use, its own (`[[options]]` in its file: plan 41e): shown in
//! Settings → Appearance only while a theme that has some is in use, one row each — a
//! switch, choices, a slider, a color written in hex. The values are kept per theme in
//! `[appearance] theme_options` (`"<theme>/<option>=<value>"`, declared in `settings.rs`);
//! a change makes every window apply the theme again ([`version`]). Generic: what a theme's
//! settings do is the theme's (its palette, its decorations), nothing of it here.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};

use i18n::{t, tr};
use settings_model::SettingsService;
use theming::options::{OptionKind, OptionValue, ThemeOption, effective, saved_for, with_saved};
use workbench_model::{SettingsBlock, SettingsContribution, SettingsRow, SettingsScope};

use crate::settings::THEME_OPTIONS;

/// The theme in use: its id, its name and its own settings.
#[derive(Debug, Clone, Default)]
struct Active {
    id: String,
    name: String,
    options: Vec<ThemeOption>,
}

static ACTIVE: RwLock<Option<Active>> = RwLock::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);

/// The theme a window applied (the settings show its own).
pub fn set_active(theme: &theming::ResolvedTheme) {
    let active = Active {
        id: theme.id.0.clone(),
        name: kernel::brand::named(&theme.name),
        options: theme.options().to_vec(),
    };
    if let Ok(mut slot) = ACTIVE.write() {
        *slot = Some(active);
    }
}

/// Changes whenever a value of a theme's own settings is saved here.
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// What the user chose of theme `id`'s own settings (`(option, value as text)`).
pub fn saved(settings: &SettingsService, id: &str) -> Vec<(String, String)> {
    saved_for(&settings.get(&THEME_OPTIONS), id)
}

/// An option's id as a row's key (rows have `'static` keys; a theme's options are few, so
/// each id is kept once for the process).
fn key(id: &str) -> &'static str {
    static KEYS: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut keys = KEYS.lock().unwrap_or_else(PoisonError::into_inner);
    let keys = keys.get_or_insert_with(HashSet::new);
    if let Some(k) = keys.get(id) {
        return k;
    }
    let leaked: &'static str = Box::leak(id.to_owned().into_boxed_str());
    keys.insert(leaked);
    leaked
}

fn active() -> Option<Active> {
    ACTIVE.read().ok().and_then(|a| a.clone())
}

/// A slider's stop as shown: a share as a percentage, else the number.
fn stop_label(min: f32, max: f32, v: f32) -> String {
    if min >= 0.0 && max <= 1.0 {
        format!("{}%", (v * 100.0).round())
    } else {
        theming::decorations::number_text(v)
    }
}

/// A row for one of the theme's own settings, showing `value`.
pub fn row(option: &ThemeOption, value: &OptionValue, locale: &str) -> SettingsRow {
    let label = option.label.get(locale).to_owned();
    let key = key(&option.id);
    match (&option.kind, value) {
        // A theme's settings are named by its file, without a description of their own.
        (OptionKind::Toggle, v) => SettingsRow::Toggle {
            key,
            label,
            detail: String::new(),
            on: *v == OptionValue::Bool(true),
            default: Some(option.default == OptionValue::Bool(true)),
        },
        (OptionKind::Choice(choices), v) => SettingsRow::Choices {
            key,
            label,
            detail: String::new(),
            default: choices
                .iter()
                .position(|c| OptionValue::Choice(c.id.clone()) == option.default)
                .map(|i| i as i64),
            choices: choices
                .iter()
                .enumerate()
                .map(|(i, c)| (i as i64, c.label.get(locale).to_owned()))
                .collect(),
            current: choices
                .iter()
                .position(|c| OptionValue::Choice(c.id.clone()) == *v)
                .unwrap_or(0) as i64,
        },
        (OptionKind::Slider { min, max, .. }, v) => {
            let stops = option.stops();
            let nearest = |v: &OptionValue| match v {
                OptionValue::Number(n) => (0..stops.len())
                    .min_by(|a, b| (stops[*a] - n).abs().total_cmp(&(stops[*b] - n).abs()))
                    .unwrap_or(0),
                _ => 0,
            };
            let at = nearest(v);
            SettingsRow::Slider {
                key,
                label,
                detail: String::new(),
                default: Some(nearest(&option.default) as i64),
                stops: stops
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, stop_label(*min, *max, *s)))
                    .collect(),
                current: at as i64,
            }
        }
        (OptionKind::Color { .. }, v) => SettingsRow::Text {
            key,
            label,
            detail: String::new(),
            value: theming::options::value_text(v),
            placeholder: theming::options::value_text(&option.default),
        },
    }
}

/// The value a row's choice `value` (its place) means for `option`.
pub fn chosen(option: &ThemeOption, value: i64) -> Option<OptionValue> {
    let at = usize::try_from(value).ok()?;
    Some(match &option.kind {
        OptionKind::Toggle => OptionValue::Bool(value != 0),
        OptionKind::Choice(choices) => OptionValue::Choice(choices.get(at)?.id.clone()),
        OptionKind::Slider { .. } => OptionValue::Number(*option.stops().get(at)?),
        OptionKind::Color { .. } => return None,
    })
}

/// The block of the theme in use's own settings, in the settings' "Appearance" section
/// (no rows — not shown — for a theme without any).
#[derive(Debug)]
pub struct ThemeOptionSettings;

impl ThemeOptionSettings {
    fn save(settings: &SettingsService, option: &ThemeOption, theme: &str, value: &OptionValue) {
        if !option.accepts(value) {
            return;
        }
        let list = with_saved(&settings.get(&THEME_OPTIONS), theme, &option.id, value);
        match settings.set(&THEME_OPTIONS, list) {
            Ok(()) => {
                VERSION.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => tracing::warn!(error = %e, "can't save the theme's setting"),
        }
    }
}

impl SettingsContribution for ThemeOptionSettings {
    fn id(&self) -> &'static str {
        "theme-options"
    }

    fn page(&self) -> &'static str {
        "appearance"
    }

    fn order(&self) -> u16 {
        10
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let Some(active) = active() else {
            return SettingsBlock { title: String::new(), detail: String::new(), rows: Vec::new() };
        };
        let values = effective(&active.options, &saved(scope.settings, &active.id));
        let locale = i18n::locale();
        let rows = active
            .options
            .iter()
            .zip(&values)
            .map(|(option, (_, value))| row(option, value, &locale))
            .collect();
        SettingsBlock {
            title: tr!("settings.theme_options", theme = active.name.as_str()),
            detail: t("settings.theme_options_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let Some(active) = active() else { return };
        let Some(option) = active.options.iter().find(|o| o.id == key) else { return };
        if let Some(v) = chosen(option, value) {
            Self::save(scope.settings, option, &active.id, &v);
        }
    }

    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        let Some(active) = active() else { return };
        let Some(option) = active.options.iter().find(|o| o.id == key) else { return };
        match option.parse_value(value) {
            Some(v) => Self::save(scope.settings, option, &active.id, &v),
            None => tracing::warn!(%value, %key, "not a value of the theme's option"),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/theme_options.rs"]
mod tests;
