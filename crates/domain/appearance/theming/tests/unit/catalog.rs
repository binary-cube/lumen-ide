use std::cell::RefCell;

use super::*;

/// A theme's "text" here: `id appearance` (`dark-one dark`).
fn parse(source: &str) -> Result<Theme, String> {
    let (id, appearance) = source.split_once(' ').ok_or("no appearance")?;
    let appearance = match appearance {
        "dark" => Appearance::Dark,
        "light" => Appearance::Light,
        _ => return Err(format!("unknown appearance `{appearance}`")),
    };
    Ok(Theme::new(id, id, appearance))
}

/// The choices and themes saved, in memory.
#[derive(Default)]
struct Saved {
    preferences: RefCell<Vec<ThemePreferences>>,
    files: RefCell<Vec<(String, String)>>,
}

impl ThemeSettings for Saved {
    fn save(&self, preferences: &ThemePreferences) -> std::io::Result<()> {
        self.preferences.borrow_mut().push(preferences.clone());
        Ok(())
    }
}

impl UserThemeFiles for Saved {
    fn save(&self, id: &str, source: &str) -> Result<(), String> {
        self.files.borrow_mut().push((id.to_owned(), source.to_owned()));
        Ok(())
    }
}

fn catalog(preferences: &ThemePreferences) -> ThemeCatalog {
    ThemeCatalog::new(["a-dark dark", "b-light light"], Vec::new(), &parse, preferences).0
}

/// The system decides until a theme is chosen; the next one, too, stops following it.
#[test]
fn the_system_decides_until_a_theme_is_chosen() {
    let saved = Saved::default();
    let mut themes = catalog(&ThemePreferences::default());
    assert_eq!(themes.current().0, "a-dark", "the first dark one");
    assert!(themes.follow_appearance(Appearance::Light));
    assert_eq!(themes.current().0, "b-light");
    assert!(themes.choose("a-dark", &saved).0);
    assert!(!themes.follow_appearance(Appearance::Light), "chosen: the system no longer decides");
    assert_eq!(
        saved.preferences.borrow().last(),
        Some(&ThemePreferences { theme: Some("a-dark".into()), follow_system: false })
    );
    assert!(!themes.choose("missing", &saved).0);
    assert!(themes.set_follow_system(true, Some(Appearance::Light), &saved).0);
    themes.next();
    assert!(!themes.follows_system());
}

/// The theme chosen before is used at startup (if it still exists); a user theme replaces a
/// bundled one with the same id; one that can't be read is reported.
#[test]
fn startup_keeps_the_choice_and_the_user_s_themes() {
    let chosen = ThemePreferences { theme: Some("b-light".into()), follow_system: false };
    let user = vec![
        ("mine.toml".to_owned(), "b-light dark".to_owned()),
        ("broken.toml".to_owned(), "c-x".to_owned()),
    ];
    let (themes, problems) =
        ThemeCatalog::new(["a-dark dark", "b-light light"], user, &parse, &chosen);
    assert_eq!(themes.current().0, "b-light");
    let replaced = themes.registry().themes().iter().find(|t| t.id.0 == "b-light").unwrap();
    assert_eq!(replaced.appearance, Appearance::Dark, "the user's replaces the bundled one");
    assert_eq!(themes.sources().iter().filter(|(id, _)| id == "b-light").count(), 1);
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].0, "broken.toml");
    let gone = ThemePreferences { theme: Some("gone".into()), follow_system: false };
    assert_eq!(catalog(&gone).current().0, "a-dark", "a theme gone: the default");
}

/// A theme written in the configurator: registered, and saved among the user's.
#[test]
fn a_written_theme_is_registered_and_saved() {
    let saved = Saved::default();
    let mut themes = catalog(&ThemePreferences::default());
    let id = themes.save_user_theme("mine dark", &parse, &saved).unwrap();
    assert_eq!(id, "mine");
    assert!(themes.select("mine"));
    assert_eq!(saved.files.borrow().as_slice(), [("mine".to_owned(), "mine dark".to_owned())]);
    assert!(themes.register_source("nonsense", &parse).is_err());
}
