//! The available themes and the active theme: the Theming context's catalog
//! (`theming::catalog`), given the themes' texts and where the choice is kept.
//!
//! Sources: the built-in themes (`themes-builtin`) and the user's files in
//! `~/.config/<app id>/themes/*.toml` (on Windows, `%APPDATA%\<app id>\themes`). A user theme
//! with the same `id` replaces the built-in one.
//!
//! The choice (the theme and "follow the system") is kept in `settings.toml`, section
//! `[appearance]`; the rest of the file is left untouched.

use settings_model::SettingsService;
use std::path::PathBuf;

use std::sync::Arc;

use theming::catalog::{ThemeCatalog, ThemeProvider, ThemeSettings, UserThemeFiles, parse_with};
use theming::{Appearance, ResolvedTheme, ThemeId};
use ui_workbench::{Palette, ThemeOption, ThemePreview};

/// What the user chose for appearance (`[appearance]` in `settings.toml`).
pub type Preferences = theming::catalog::ThemePreferences;

/// The choice, kept in `[appearance]` (the rest of the settings left as they are).
struct AppearanceSettings;

impl ThemeSettings for AppearanceSettings {
    fn save(&self, preferences: &Preferences) -> std::io::Result<()> {
        write_preferences(preferences)
    }
}

/// The user's themes, in the config folder's `themes`.
struct ThemeFolder;

impl UserThemeFiles for ThemeFolder {
    fn save(&self, id: &str, source: &str) -> Result<(), String> {
        let dir = user_theme_dir().ok_or("no config folder")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{id}.toml")), source).map_err(|e| e.to_string())
    }
}

/// The themes (the Theming context's catalog), with the palettes the interface draws with.
#[derive(Debug)]
pub struct Themes {
    catalog: ThemeCatalog,
    /// Where they come from (the modules' contributions): their format reads every theme.
    providers: Vec<Arc<dyn ThemeProvider>>,
}

impl Themes {
    pub fn load(providers: &[Arc<dyn ThemeProvider>]) -> Self {
        Self::with_preferences(providers, read_preferences())
    }

    pub fn with_preferences(
        providers: &[Arc<dyn ThemeProvider>],
        preferences: Preferences,
    ) -> Self {
        let mut user = Vec::new();
        for path in user_theme_files() {
            match std::fs::read_to_string(&path) {
                Ok(source) => user.push((path.display().to_string(), source)),
                Err(e) => tracing::warn!("theme {} can't be loaded: {e}", path.display()),
            }
        }
        let bundled: Vec<&'static str> = providers.iter().flat_map(|p| p.bundled()).collect();
        let parse = |source: &str| parse_with(providers, source);
        let (catalog, problems) = ThemeCatalog::new(bundled, user, &parse, &preferences);
        for (place, why) in problems {
            tracing::warn!("theme {place} can't be loaded: {why}");
        }
        // What was ignored while reading them (an option a theme can't have…).
        for theme in catalog.registry().themes() {
            for note in &theme.notes {
                tracing::warn!("theme {}: {note}", theme.id);
            }
        }
        Self { catalog, providers: providers.to_vec() }
    }

    /// The available themes, with colors for the preview.
    pub fn options(&self) -> Vec<ThemeOption> {
        let registry = self.catalog.registry();
        registry
            .themes()
            .iter()
            // The configurator's preview theme is not a choice.
            .filter(|t| t.id.0 != theming::studio::PREVIEW_ID)
            .filter_map(|t| {
                let palette = Palette::from_theme(&registry.resolve(&t.id).ok()?);
                Some(ThemeOption {
                    id: t.id.0.clone(),
                    name: kernel::brand::named(&t.name),
                    dark: t.appearance == Appearance::Dark,
                    preview: ThemePreview::from_palette(&palette),
                })
            })
            .collect()
    }

    /// The themes with their text (the configurator's templates).
    pub fn sources(&self) -> &[(String, std::borrow::Cow<'static, str>)] {
        self.catalog.sources()
    }

    /// The configurator's themes (without its preview), with what was read of each — its
    /// decorations and its own settings —, `current` the one it starts from.
    pub fn studio_input(&self, current: &str) -> theming::studio::StudioInput {
        let names: std::collections::HashMap<String, String> =
            self.options().into_iter().map(|o| (o.id, o.name)).collect();
        let registry = self.catalog.registry();
        let themes = self
            .sources()
            .iter()
            .filter(|(id, _)| id != theming::studio::PREVIEW_ID)
            .map(|(id, src)| {
                let name = names.get(id).cloned().unwrap_or_else(|| id.clone());
                let mut theme = theming::studio::StudioTheme::plain(id, &name, src);
                if let Some((decorations, options)) =
                    registry.declared(&ThemeId::parse(id), &self.saved_options(id))
                {
                    theme.decorations = decorations;
                    theme.options = options;
                }
                theme
            })
            .collect();
        theming::studio::StudioInput { themes, current: current.to_owned() }
    }

    /// What the user chose of theme `id`'s own settings (`(option, value)`).
    fn saved_options(&self, id: &str) -> Vec<(String, String)> {
        crate::theme_options::saved(crate::config::settings(), id)
    }

    /// Theme `id` resolved with the user's values of its own settings.
    fn resolve(&self, id: &ThemeId) -> Result<ResolvedTheme, theming::ThemeError> {
        self.catalog.registry().resolve_with(id, &self.saved_options(&id.0))
    }

    /// Registers a theme from text (the configurator preview or a saved one).
    /// A theme with the same id replaces it.
    pub fn register_source(&mut self, src: &str) -> Result<String, String> {
        let providers = &self.providers;
        self.catalog.register_source(src, &|source: &str| parse_with(providers, source))
    }

    /// Saves a theme in the global folder (`themes/<id>.toml` in the config folder) and
    /// registers it.
    pub fn save_user_theme(&mut self, src: &str) -> Result<String, String> {
        let providers = &self.providers;
        let parse = |source: &str| parse_with(providers, source);
        self.catalog.save_user_theme(src, &parse, &ThemeFolder)
    }

    /// The palette and resolved theme of a specific theme (e.g. the project theme).
    pub fn palette_of(&self, id: &str) -> Option<(Palette, ResolvedTheme)> {
        let theme = self.resolve(&ThemeId::parse(id)).ok()?;
        Some((Palette::from_theme(&theme), theme))
    }

    pub fn current_id(&self) -> &str {
        &self.catalog.current().0
    }

    pub fn follows_system(&self) -> bool {
        self.catalog.follows_system()
    }

    /// Explicit choice from settings: stops following the system and is saved.
    pub fn choose(&mut self, id: &str) -> bool {
        let (changed, saved) = self.catalog.choose(id, &AppearanceSettings);
        if let Err(e) = saved {
            tracing::warn!("can't save the appearance settings: {e}");
        }
        changed
    }

    /// Turns following the system theme on / off. `true` if the active theme changed.
    pub fn set_follow_system(&mut self, on: bool, system: Option<Appearance>) -> bool {
        let (changed, saved) = self.catalog.set_follow_system(on, system, &AppearanceSettings);
        if let Err(e) = saved {
            tracing::warn!("can't save the appearance settings: {e}");
        }
        changed
    }

    /// The preferences as `settings.toml` holds them now (rewritten by another program): the
    /// theme first, as turning following off keeps the active one. `true` if it changed.
    pub fn adopt_saved(&mut self, saved: &Preferences, system: Option<Appearance>) -> bool {
        let mut changed = false;
        if !saved.follow_system
            && let Some(id) = &saved.theme
            && id.as_str() != self.current_id()
        {
            changed |= self.catalog.select(id);
        }
        if saved.follow_system != self.follows_system() {
            changed |= self.set_follow_system(saved.follow_system, system);
        }
        changed
    }

    /// [`Self::adopt_saved`] of what the user's settings hold.
    pub fn adopt_settings(&mut self, system: Option<Appearance>) -> bool {
        self.adopt_saved(&read_preferences(), system)
    }

    /// The active theme's palette (with fallback values if the theme can't be resolved).
    pub fn palette(&self) -> Palette {
        match self.resolve(self.catalog.current()) {
            Ok(theme) => Palette::from_theme(&theme),
            Err(e) => {
                tracing::error!("theme {} can't be used: {e}", self.catalog.current());
                Palette::dark()
            }
        }
    }

    /// The active theme, resolved (for syntax colors).
    pub fn resolved(&self) -> Option<ResolvedTheme> {
        self.resolve(self.catalog.current()).ok()
    }

    #[cfg(test)]
    pub fn current(&self) -> &ThemeId {
        self.catalog.current()
    }

    /// Picks the theme from the system appearance. `true` if the active theme changed.
    pub fn follow_appearance(&mut self, appearance: Appearance) -> bool {
        self.catalog.follow_appearance(appearance)
    }

    /// Switches to the next theme and stops following the system theme.
    pub fn next(&mut self) {
        self.catalog.next();
        tracing::info!(theme = %self.catalog.current(), "theme selected");
    }

    #[cfg(any(test, feature = "snapshot"))]
    /// Selects a theme by id (e.g. for a snapshot). `false` if it doesn't exist.
    pub fn select(&mut self, id: &str) -> bool {
        self.catalog.select(id)
    }
}

fn read_preferences() -> Preferences {
    preferences_from(crate::config::settings())
}

/// The preferences as `settings` hold them (missing: the defaults).
fn preferences_from(settings: &SettingsService) -> Preferences {
    let theme = settings.get(&crate::settings::THEME);
    Preferences {
        theme: (!theme.is_empty()).then_some(theme),
        follow_system: settings.get(&crate::settings::THEME_FOLLOWS_SYSTEM),
    }
}

/// Writes the preferences to the user settings.
fn write_preferences(preferences: &Preferences) -> std::io::Result<()> {
    write_to(crate::config::settings(), preferences)
}

fn write_to(settings: &SettingsService, preferences: &Preferences) -> std::io::Result<()> {
    settings.set(&crate::settings::THEME, preferences.theme.clone().unwrap_or_default())?;
    settings.set(&crate::settings::THEME_FOLLOWS_SYSTEM, preferences.follow_system)
}

fn user_theme_dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("themes"))
}

fn user_theme_files() -> Vec<PathBuf> {
    let Some(dir) = user_theme_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
#[path = "../tests/unit/themes.rs"]
mod tests;
