use super::*;

/// The bundled themes, as their module contributes them.
fn bundled() -> Vec<Arc<dyn ThemeProvider>> {
    vec![Arc::new(themes_builtin::Bundled)]
}

#[test]
fn follows_system_until_user_picks() {
    let mut themes = Themes::with_preferences(&bundled(), Preferences::default());
    assert!(themes.follow_appearance(Appearance::Light));
    assert_eq!(themes.current(), &ThemeId("default-light".into()));
    themes.next();
    assert!(!themes.follow_appearance(Appearance::Light));
    assert!(themes.select("default-dark"));
    assert!(!themes.select("no-such-theme"));
}

#[derive(Debug, Default)]
struct Memory(std::sync::Mutex<std::collections::HashMap<String, settings_model::Section>>);

impl settings_model::SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<settings_model::Section> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn update(
        &self,
        name: &str,
        change: &mut dyn FnMut(&mut settings_model::Section),
    ) -> std::io::Result<()> {
        change(self.0.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

#[test]
fn preferences_round_trip() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    assert_eq!(preferences_from(&settings), Preferences::default());
    let prefs = Preferences { theme: Some("default-light".into()), follow_system: false };
    write_to(&settings, &prefs).unwrap();
    assert_eq!(preferences_from(&settings), prefs);
    // No theme chosen again: none read back.
    let none = Preferences { theme: None, follow_system: true };
    write_to(&settings, &none).unwrap();
    assert_eq!(preferences_from(&settings), none);
}

#[test]
fn saved_theme_is_used_at_startup() {
    let prefs = Preferences { theme: Some("default-light".into()), follow_system: false };
    let mut themes = Themes::with_preferences(&bundled(), prefs);
    assert_eq!(themes.current_id(), "default-light");
    assert!(!themes.follow_appearance(Appearance::Dark));
    assert!(themes.options().iter().any(|o| o.id == "default-dark" && o.dark));
}

/// The Task Manager's charts and cards on the default themes: each series' color at least
/// 3:1 on the card's background, the text and the muted text at least 4.5:1.
#[test]
fn the_task_manager_reads_on_the_default_themes() {
    use ui_workbench::chart::{Series, series_color};
    use ui_workbench::severity::contrast;
    let themes = Themes::load(&bundled());
    for id in ["default-dark", "default-light"] {
        let (palette, _) = themes.palette_of(id).expect("a default theme");
        // A card: the editor's background, lit by 3% of the text.
        let card = palette.editor_background.mix(palette.text, 0.03);
        for series in Series::ALL {
            let ratio = contrast(series_color(&palette, series), card);
            assert!(ratio >= 3.0, "{id}: {series:?} at {ratio:.2}");
        }
        // The muted text as the Task Manager draws it (moved toward the text when the theme's
        // is too faint on a card).
        let muted = ui_workbench::severity::readable(palette.text_muted, palette.text, card, 4.5);
        for (name, color) in [("text", palette.text), ("muted", muted)] {
            let ratio = contrast(color, card);
            assert!(ratio >= 4.5, "{id}: {name} at {ratio:.2}");
        }
    }
}

#[test]
fn a_theme_saved_by_another_program_is_adopted() {
    let mut themes = Themes::with_preferences(
        &bundled(),
        Preferences { theme: Some("default-dark".into()), follow_system: false },
    );
    let same = Preferences { theme: Some("default-dark".into()), follow_system: false };
    assert!(!themes.adopt_saved(&same, None));
    let other = Preferences { theme: Some("default-light".into()), follow_system: false };
    assert!(themes.adopt_saved(&other, None));
    assert_eq!(themes.current_id(), "default-light");
    assert!(!themes.follows_system());
}
