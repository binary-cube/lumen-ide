//! The theme in use's own settings in Settings → Appearance: a row each (the switch, the
//! choices, the slider's stops, the color in hex), what a row's choice means, a value saved
//! for the theme only (invalid ones refused), and no rows for a theme without any.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};
use theming::options::{Label, OptionChoice};

use super::*;

#[derive(Debug, Default)]
struct Memory(Mutex<HashMap<String, Section>>);

impl SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<Section> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        change(self.0.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

fn lights() -> ThemeOption {
    ThemeOption {
        id: "lights".into(),
        label: Label(vec![("en".into(), "Lights".into()), ("ro".into(), "Luminițe".into())]),
        kind: OptionKind::Choice(vec![
            OptionChoice { id: "warm".into(), label: Label::plain("Warm"), palette: Vec::new() },
            OptionChoice { id: "cold".into(), label: Label::plain("Cold"), palette: Vec::new() },
        ]),
        default: OptionValue::Choice("warm".into()),
    }
}

fn snow() -> ThemeOption {
    ThemeOption {
        id: "snow".into(),
        label: Label::plain("Snow"),
        kind: OptionKind::Slider { min: 0.0, max: 0.8, step: 0.2 },
        default: OptionValue::Number(0.4),
    }
}

#[test]
fn each_kind_has_its_row() {
    match row(&lights(), &OptionValue::Choice("cold".into()), "ro") {
        SettingsRow::Choices { key, label, choices, current, .. } => {
            assert_eq!((key, label.as_str(), current), ("lights", "Luminițe", 1));
            assert_eq!(choices.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    match row(&snow(), &OptionValue::Number(0.6), "en") {
        SettingsRow::Slider { stops, current, .. } => {
            let labels: Vec<&str> = stops.iter().map(|(_, l)| l.as_str()).collect();
            assert_eq!(labels, ["0%", "20%", "40%", "60%", "80%"]);
            assert_eq!(current, 3);
        }
        other => panic!("{other:?}"),
    }
    let toggle = ThemeOption {
        id: "bows".into(),
        label: Label::plain("Bows"),
        kind: OptionKind::Toggle,
        default: OptionValue::Bool(true),
    };
    assert!(matches!(
        row(&toggle, &OptionValue::Bool(true), "en"),
        SettingsRow::Toggle { on: true, .. }
    ));
    let color = ThemeOption {
        id: "tint".into(),
        label: Label::plain("Tint"),
        kind: OptionKind::Color { palette: None },
        default: OptionValue::Color(kernel::Rgba::WHITE),
    };
    match row(&color, &OptionValue::Color(kernel::Rgba::rgb(0xff0000)), "en") {
        SettingsRow::Text { value, placeholder, .. } => {
            assert_eq!((value.as_str(), placeholder.as_str()), ("#ff0000", "#ffffff"));
        }
        other => panic!("{other:?}"),
    }
    // A row's choice: what it means.
    assert_eq!(chosen(&lights(), 1), Some(OptionValue::Choice("cold".into())));
    assert_eq!(chosen(&lights(), 9), None);
    assert_eq!(chosen(&snow(), 2), Some(OptionValue::Number(0.4)));
    assert_eq!(chosen(&toggle, 0), Some(OptionValue::Bool(false)));
    assert_eq!(chosen(&color, 0), None, "a color is written");
}

/// The same id, the same key (kept once).
#[test]
fn keys_are_kept_once() {
    let a = key("snow-density");
    let b = key("snow-density");
    assert!(std::ptr::eq(a, b));
}

/// The theme in use is process-wide: the tests setting it take turns.
static TURNS: Mutex<()> = Mutex::new(());

#[test]
fn values_are_saved_for_the_theme_in_use_only() {
    let _turn = TURNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let theme = |options: Vec<ThemeOption>| {
        let mut registry = theming::ThemeRegistry::default();
        registry.register(theming::Theme {
            options,
            ..theming::Theme::new("festive", "Festive", theming::Appearance::Dark)
        });
        registry.resolve(&theming::ThemeId::parse("festive")).unwrap()
    };
    set_active(&theme(vec![lights(), snow()]));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let block = ThemeOptionSettings.block(scope);
    assert_eq!(block.rows.len(), 2);
    let before = version();
    ThemeOptionSettings.choose(scope, key("lights"), 1);
    ThemeOptionSettings.choose(scope, key("snow"), 4);
    ThemeOptionSettings.choose(scope, key("nothing"), 1);
    assert!(version() > before, "the windows apply the theme again");
    assert_eq!(
        saved(&settings, "festive"),
        vec![("lights".to_owned(), "cold".to_owned()), ("snow".to_owned(), "0.8".to_owned())]
    );
    assert!(saved(&settings, "other").is_empty());
    // A theme without settings of its own: no rows (the block isn't shown).
    set_active(&theme(Vec::new()));
    assert!(ThemeOptionSettings.block(scope).rows.is_empty());
}

/// The Matrix theme's digital rain (plan 45): on by default; switched off in Settings →
/// Appearance, the choice is saved for Matrix only and wins over the theme's default every
/// time it's resolved, and another theme has no rain whatever Matrix's choice.
#[test]
fn matrix_s_rain_turned_off_stays_off() {
    use theming::decorations::LayerKind;
    let _turn = TURNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let mut registry = theming::ThemeRegistry::default();
    for theme in themes_builtin::builtin_themes() {
        registry.register(theme);
    }
    let matrix = theming::ThemeId::parse("matrix");
    let resolve =
        |id: &theming::ThemeId| registry.resolve_with(id, &saved(&settings, &id.0)).unwrap();
    assert!(resolve(&matrix).decorations().first(LayerKind::Matrix).is_some(), "on by default");
    set_active(&resolve(&matrix));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let block = ThemeOptionSettings.block(scope);
    assert_eq!(block.rows.len(), 2, "the rain's switch and its density");
    ThemeOptionSettings.choose(scope, key("rain"), 0);
    assert_eq!(saved(&settings, "matrix"), vec![("rain".to_owned(), "false".to_owned())]);
    assert!(resolve(&matrix).decorations().first(LayerKind::Matrix).is_none(), "kept off");
    let other = theming::ThemeId::parse("default-dark");
    assert!(resolve(&other).decorations().is_empty());
    // Back on: the theme's own again.
    ThemeOptionSettings.choose(scope, key("rain"), 1);
    assert!(resolve(&matrix).decorations().first(LayerKind::Matrix).is_some());
}
