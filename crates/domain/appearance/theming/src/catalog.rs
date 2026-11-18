//! The themes to choose from and the one in use — the Theming context's use cases: the
//! bundled themes and the user's (one with the same id replaces a bundled one), the theme
//! following the system's appearance until one is chosen, the next one, a theme written in
//! the configurator registered or saved. What's chosen is saved through [`ThemeSettings`],
//! a user's theme through [`UserThemeFiles`]; a theme's text is read by the caller's
//! `parse` (the themes' file format belongs to their module).

use std::borrow::Cow;

use crate::{Appearance, ResolvedTheme, Theme, ThemeError, ThemeId, ThemeRegistry};

/// Reads a theme from its text.
pub type ParseTheme<'a> = &'a dyn Fn(&str) -> Result<Theme, String>;

/// Where themes come from, contributed by a module: the ones it brings, and the format it
/// reads (the user's theme files in it too).
pub trait ThemeProvider: Send + Sync + std::fmt::Debug {
    /// Its themes' texts, in the order they're offered.
    fn bundled(&self) -> Vec<&'static str>;
    /// A theme from its text, if it's in this provider's format.
    fn parse(&self, source: &str) -> Result<Theme, String>;
}

/// A theme read by the first of `providers` that can read it (else the last one's reason).
pub fn parse_with(
    providers: &[std::sync::Arc<dyn ThemeProvider>],
    source: &str,
) -> Result<Theme, String> {
    let mut why = "no theme format".to_owned();
    for provider in providers {
        match provider.parse(source) {
            Ok(theme) => return Ok(theme),
            Err(e) => why = e,
        }
    }
    Err(why)
}

/// What the user chose for the appearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePreferences {
    pub theme: Option<String>,
    pub follow_system: bool,
}

impl Default for ThemePreferences {
    fn default() -> Self {
        Self { theme: None, follow_system: true }
    }
}

/// Where the choice is kept (port).
pub trait ThemeSettings {
    fn save(&self, preferences: &ThemePreferences) -> std::io::Result<()>;
}

/// Where the user's own themes are kept (port).
pub trait UserThemeFiles {
    /// Writes theme `id`'s text.
    fn save(&self, id: &str, source: &str) -> Result<(), String>;
}

/// The id used when no theme of the system's appearance exists.
const FALLBACK: &str = "default-dark";

/// The themes and the one in use.
#[derive(Debug)]
pub struct ThemeCatalog {
    registry: ThemeRegistry,
    /// The text of each theme (id → its text), for the configurator.
    sources: Vec<(String, Cow<'static, str>)>,
    current: ThemeId,
    /// As long as the user hasn't explicitly chosen a theme, the system's appearance decides.
    follow_system: bool,
}

impl ThemeCatalog {
    /// The bundled themes' texts, then the user's (`(where, text)`: replacing a bundled one
    /// of the same id), read with `parse`; the theme chosen before (if it still exists),
    /// else the first dark one. The user themes that can't be read: `(where, why)`.
    pub fn new(
        bundled: impl IntoIterator<Item = &'static str>,
        user: Vec<(String, String)>,
        parse: ParseTheme<'_>,
        preferences: &ThemePreferences,
    ) -> (Self, Vec<(String, String)>) {
        let mut catalog = Self {
            registry: ThemeRegistry::default(),
            sources: Vec::new(),
            current: ThemeId::parse(FALLBACK),
            follow_system: preferences.follow_system,
        };
        for source in bundled {
            if let Ok(theme) = parse(source) {
                catalog.add(theme, Cow::Borrowed(source));
            }
        }
        let mut problems = Vec::new();
        for (place, source) in user {
            match parse(&source) {
                Ok(theme) => catalog.add(theme, Cow::Owned(source)),
                Err(why) => problems.push((place, why)),
            }
        }
        let default = catalog
            .registry
            .first_with(Appearance::Dark)
            .cloned()
            .unwrap_or_else(|| ThemeId::parse(FALLBACK));
        let saved = preferences.theme.as_deref().map(ThemeId::parse).filter(|id| catalog.has(id));
        catalog.current = saved.unwrap_or(default);
        (catalog, problems)
    }

    fn add(&mut self, theme: Theme, source: Cow<'static, str>) {
        self.sources.retain(|(id, _)| *id != theme.id.0);
        self.sources.push((theme.id.0.clone(), source));
        self.registry.register(theme);
    }

    fn has(&self, id: &ThemeId) -> bool {
        self.registry.themes().iter().any(|t| &t.id == id)
    }

    pub fn registry(&self) -> &ThemeRegistry {
        &self.registry
    }

    /// The themes with their text (the configurator's templates).
    pub fn sources(&self) -> &[(String, Cow<'static, str>)] {
        &self.sources
    }

    /// Registers a theme from its text (the configurator's preview, or one saved); one
    /// with the same id is replaced. Its id.
    pub fn register_source(
        &mut self,
        source: &str,
        parse: ParseTheme<'_>,
    ) -> Result<String, String> {
        let theme = parse(source)?;
        let id = theme.id.0.clone();
        self.add(theme, Cow::Owned(source.to_owned()));
        Ok(id)
    }

    /// Registers a theme from its text and saves it among the user's (`files`). Its id.
    pub fn save_user_theme(
        &mut self,
        source: &str,
        parse: ParseTheme<'_>,
        files: &dyn UserThemeFiles,
    ) -> Result<String, String> {
        let id = self.register_source(source, parse)?;
        files.save(&id, source)?;
        Ok(id)
    }

    pub fn current(&self) -> &ThemeId {
        &self.current
    }

    pub fn follows_system(&self) -> bool {
        self.follow_system
    }

    /// What's chosen now (to save).
    pub fn preferences(&self) -> ThemePreferences {
        ThemePreferences { theme: Some(self.current.0.clone()), follow_system: self.follow_system }
    }

    /// The theme in use, resolved.
    pub fn resolved(&self) -> Result<ResolvedTheme, ThemeError> {
        self.registry.resolve(&self.current)
    }

    /// An explicit choice (the settings): the system no longer decides; saved. Whether the
    /// theme in use changed, and the save's outcome.
    pub fn choose(
        &mut self,
        id: &str,
        settings: &dyn ThemeSettings,
    ) -> (bool, std::io::Result<()>) {
        let changed = self.current.0 != id;
        let exists = self.select(id);
        let saved = if exists { settings.save(&self.preferences()) } else { Ok(()) };
        (exists && changed, saved)
    }

    /// Following the system's appearance on / off (`system`: its appearance now); saved.
    /// Whether the theme in use changed, and the save's outcome.
    pub fn set_follow_system(
        &mut self,
        on: bool,
        system: Option<Appearance>,
        settings: &dyn ThemeSettings,
    ) -> (bool, std::io::Result<()>) {
        self.follow_system = on;
        let changed = match (on, system) {
            (true, Some(appearance)) => self.follow_appearance(appearance),
            _ => false,
        };
        (changed, settings.save(&self.preferences()))
    }

    /// The system's appearance changed: its first theme, while following it. `true` if the
    /// theme in use changed.
    pub fn follow_appearance(&mut self, appearance: Appearance) -> bool {
        if !self.follow_system {
            return false;
        }
        match self.registry.first_with(appearance) {
            Some(id) if *id != self.current => {
                self.current = id.clone();
                true
            }
            _ => false,
        }
    }

    /// The next theme; the system no longer decides.
    pub fn next(&mut self) {
        if let Some(id) = self.registry.next_after(&self.current) {
            self.current = id.clone();
        }
        self.follow_system = false;
    }

    /// Uses theme `id` (the system no longer decides). `false` if it doesn't exist.
    pub fn select(&mut self, id: &str) -> bool {
        let id = ThemeId::parse(id);
        let exists = self.has(&id);
        if exists {
            self.current = id;
            self.follow_system = false;
        }
        exists
    }
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
