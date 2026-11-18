//! Theming bounded context: the theme model, semantic tokens, style resolution.
//!
//! A theme is just data: colors for UI tokens (`editor.background`,
//! `tree.selection`) and styles for syntax scopes (`keyword`, `string`).
//! Lookup is hierarchical: `editor.gutter.foreground.active` falls back to
//! `editor.gutter.foreground`, then to `editor.gutter`, and so on. A theme can inherit
//! another (`extends`) and override only what it cares about.
//!
//! The file format (TOML) is not the domain's concern: the `themes-builtin` module reads it.

pub mod backdrop;
pub mod caret;
pub mod catalog;
pub mod decorations;
pub mod options;
pub mod studio;

use std::collections::HashMap;
use std::fmt;

use kernel::Rgba;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThemeId(pub String);

impl ThemeId {
    /// An id as written in a theme file or in saved settings; ids from before the themes
    /// were renamed (`lumen-dark`, `lumen-arctic`) map to today's (`default-dark`, `arctic`).
    pub fn parse(id: &str) -> Self {
        // `lumen-`: the prefix the bundled themes' ids had (an alias for saved settings).
        let id = id.strip_prefix("lumen-").map_or(id, |rest| match rest {
            "dark" => "default-dark",
            "light" => "default-light",
            "studio-preview" => "studio-preview",
            other => other,
        });
        Self(id.to_owned())
    }
}

impl fmt::Display for ThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

/// The style of a syntax scope.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextStyle {
    pub foreground: Option<Rgba>,
    pub background: Option<Rgba>,
    pub bold: bool,
    pub italic: bool,
}

/// A theme as it was defined (possibly incomplete, if it extends another).
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: ThemeId,
    pub name: String,
    pub appearance: Appearance,
    pub extends: Option<ThemeId>,
    pub ui: HashMap<String, Rgba>,
    pub syntax: HashMap<String, TextStyle>,
    /// The caret (`[caret]`; what it doesn't say: the extended theme's, else the default).
    pub caret: caret::CaretSpec,
    /// What's under the windows over the editor (`[backdrop]`; what it doesn't say: the
    /// extended theme's, else the default).
    pub backdrop: backdrop::BackdropSpec,
    /// The floating panels' glass (`[floating]`; what it doesn't say: the extended theme's,
    /// else opaque).
    pub floating: backdrop::SurfaceSpec,
    /// Its decorations (`[[decorations.layers]]`, or the first format's
    /// `[decorations.<kind>]`; none declared: the extended theme's; none: nothing drawn).
    pub decorations: decorations::DecorationsSpec,
    /// Its own settings (`[[options]]`; with the same id: replacing the extended theme's).
    pub options: Vec<options::ThemeOption>,
    /// The palette's name each UI token was written with (`"$name"`): an option changing
    /// that name changes the token.
    pub ui_refs: HashMap<String, String>,
    /// The palette's name each syntax scope's color was written with.
    pub syntax_refs: HashMap<String, String>,
    /// What was ignored while reading it (an option it can't have…), for the log.
    pub notes: Vec<String>,
}

impl Theme {
    /// A theme with nothing in it yet.
    pub fn new(id: &str, name: &str, appearance: Appearance) -> Self {
        Self {
            id: ThemeId::parse(id),
            name: name.to_owned(),
            appearance,
            extends: None,
            ui: HashMap::new(),
            syntax: HashMap::new(),
            caret: caret::CaretSpec::default(),
            backdrop: backdrop::BackdropSpec::default(),
            floating: backdrop::SurfaceSpec::default(),
            decorations: decorations::DecorationsSpec::default(),
            options: Vec::new(),
            ui_refs: HashMap::new(),
            syntax_refs: HashMap::new(),
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    #[error("theme `{0}` doesn't exist")]
    NotFound(ThemeId),
    #[error("circular inheritance between themes, starting at `{0}`")]
    CircularExtends(ThemeId),
}

/// A theme with inheritance applied, ready to use.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTheme {
    pub id: ThemeId,
    pub name: String,
    pub appearance: Appearance,
    ui: HashMap<String, Rgba>,
    syntax: HashMap<String, TextStyle>,
    caret: caret::CaretLook,
    backdrop: backdrop::BackdropSpec,
    floating: backdrop::SurfaceSpec,
    /// Shared: the window's decorations keep it as long as they draw it.
    decorations: std::sync::Arc<decorations::Decorations>,
    /// Its own settings (through the `extends` chain) and the value each has now.
    options: std::sync::Arc<[options::ThemeOption]>,
    values: Vec<(String, options::OptionValue)>,
}

impl ResolvedTheme {
    /// What the theme draws besides its colors (empty: nothing).
    pub fn decorations(&self) -> &std::sync::Arc<decorations::Decorations> {
        &self.decorations
    }

    /// Its own settings.
    pub fn options(&self) -> &[options::ThemeOption] {
        &self.options
    }

    /// The value each of its own settings has now.
    pub fn option_values(&self) -> &[(String, options::OptionValue)] {
        &self.values
    }

    /// How the caret looks and moves.
    pub fn caret(&self) -> caret::CaretLook {
        self.caret
    }

    /// What it says of the backdrop under the windows over the editor.
    pub fn backdrop(&self) -> backdrop::BackdropSpec {
        self.backdrop
    }

    /// What it says of the floating panels' glass (the menu drawer's).
    pub fn floating(&self) -> backdrop::SurfaceSpec {
        self.floating
    }

    /// The color of a UI token, with hierarchical fallback on prefixes.
    pub fn color(&self, token: &str) -> Option<Rgba> {
        lookup(&self.ui, token).copied()
    }

    /// The color of a token, or `fallback` if the theme doesn't define it at all.
    pub fn color_or(&self, token: &str, fallback: Rgba) -> Rgba {
        self.color(token).unwrap_or(fallback)
    }

    /// All colors whose tokens start with `prefix` (e.g. `icon.` → the file icon colors
    /// per language), with the prefix removed.
    pub fn colors_with_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, Rgba)> + 'a {
        self.ui.iter().filter_map(move |(k, v)| k.strip_prefix(prefix).map(|rest| (rest, *v)))
    }

    /// The style of a syntax scope, with hierarchical fallback (`function.method.call` →
    /// `function.method` → `function`).
    pub fn syntax(&self, scope: &str) -> Option<TextStyle> {
        if let Some(rest) = scope.strip_prefix(DOC_PREFIX) {
            return self.doc_syntax(scope, rest);
        }
        lookup(&self.syntax, scope).copied()
    }

    /// A doc comment's part (`comment.doc.tag.php`): as the theme names it, else from its
    /// code colors (plan 75h) — a tag in the comment's color, bold; a type as types are; a
    /// documented name as parameters are; all as italic as the comment.
    fn doc_syntax(&self, scope: &str, rest: &str) -> Option<TextStyle> {
        let (part, language) = rest.split_once('.').unwrap_or((rest, ""));
        let own = format!("{DOC_PREFIX}{part}");
        let named = self.syntax.get(scope).or_else(|| self.syntax.get(&own)).copied();
        if named.is_some() {
            return named;
        }
        let comment = lookup(&self.syntax, &format!("comment.{language}")).copied()?;
        let code = |base: &str| {
            let key =
                if language.is_empty() { base.to_owned() } else { format!("{base}.{language}") };
            lookup(&self.syntax, &key).copied()
        };
        let derived = match part {
            "tag" => TextStyle { bold: true, ..comment },
            "type" => code("type").map(|t| TextStyle { italic: comment.italic, ..t })?,
            "variable" => {
                code("variable.parameter").map(|t| TextStyle { italic: comment.italic, ..t })?
            }
            _ => comment,
        };
        Some(derived)
    }
}

/// The scopes of a doc comment's parts (`comment.doc.tag`, `.type`, `.variable`).
const DOC_PREFIX: &str = "comment.doc.";

/// The own settings of a chain (the extended themes' first, overridden by id).
fn chain_options(chain: &[&Theme]) -> Vec<options::ThemeOption> {
    let mut list: Vec<options::ThemeOption> = Vec::new();
    for theme in chain.iter().rev() {
        list = options::over(&theme.options, &list);
    }
    list
}

/// Looks up `key`, then its `.`-separated prefixes.
fn lookup<'a, T>(map: &'a HashMap<String, T>, key: &str) -> Option<&'a T> {
    let mut key = key;
    loop {
        if let Some(v) = map.get(key) {
            return Some(v);
        }
        key = &key[..key.rfind('.')?];
    }
}

/// All known themes (bundled, user, from modules).
#[derive(Debug, Clone, Default)]
pub struct ThemeRegistry {
    themes: Vec<Theme>,
}

impl ThemeRegistry {
    /// Registers a theme; one with the same id replaces it (user themes
    /// can override the bundled themes).
    pub fn register(&mut self, theme: Theme) {
        match self.themes.iter_mut().find(|t| t.id == theme.id) {
            Some(existing) => *existing = theme,
            None => self.themes.push(theme),
        }
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    /// The first theme with the requested appearance (for following the system theme).
    pub fn first_with(&self, appearance: Appearance) -> Option<&ThemeId> {
        self.themes.iter().find(|t| t.appearance == appearance).map(|t| &t.id)
    }

    /// The theme that comes after `current`, in registration order (for cycling).
    pub fn next_after(&self, current: &ThemeId) -> Option<&ThemeId> {
        let i = self.themes.iter().position(|t| &t.id == current)?;
        self.themes.get((i + 1) % self.themes.len()).map(|t| &t.id)
    }

    /// Theme `id`, then the one it extends, and so on.
    fn chain(&self, id: &ThemeId) -> Result<Vec<&Theme>, ThemeError> {
        let mut chain = Vec::new();
        let mut current = Some(id.clone());
        while let Some(cid) = current {
            if chain.iter().any(|t: &&Theme| t.id == cid) {
                return Err(ThemeError::CircularExtends(id.clone()));
            }
            let theme =
                self.themes.iter().find(|t| t.id == cid).ok_or(ThemeError::NotFound(cid))?;
            chain.push(theme);
            current = theme.extends.clone();
        }
        Ok(chain)
    }

    /// What theme `id` declares through its `extends` chain, not resolved (the studio
    /// edits it): its decorations (what follows its palette and settings kept) and its own
    /// settings, their defaults the user's values (`saved`, as for [`Self::resolve_with`]).
    pub fn declared(
        &self,
        id: &ThemeId,
        saved: &[(String, String)],
    ) -> Option<(decorations::DecorationsSpec, Vec<options::ThemeOption>)> {
        let chain = self.chain(id).ok()?;
        let mut list = chain_options(&chain);
        let values = options::effective(&list, saved);
        for (o, (_, v)) in list.iter_mut().zip(values) {
            o.default = v;
        }
        let mut decorations = decorations::DecorationsSpec::default();
        for theme in chain.iter().rev() {
            decorations = theme.decorations.clone().over(decorations);
        }
        Some((decorations, list))
    }

    /// Applies the `extends` chain: the base theme first, then the overrides (its own
    /// settings at their defaults).
    pub fn resolve(&self, id: &ThemeId) -> Result<ResolvedTheme, ThemeError> {
        self.resolve_with(id, &[])
    }

    /// The same with the user's values of its own settings (`(option, value as text)`; one
    /// the theme doesn't have, or doesn't take, is left out): the palette's names they
    /// change applied to the tokens written with them, the decorations shown and set by
    /// them.
    pub fn resolve_with(
        &self,
        id: &ThemeId,
        saved: &[(String, String)],
    ) -> Result<ResolvedTheme, ThemeError> {
        let chain = self.chain(id)?;
        let top = chain[0];
        let option_list = chain_options(&chain);
        let values = options::effective(&option_list, saved);
        let palette = options::palette_overrides(&option_list, &values);
        let changed = |refs: &HashMap<String, String>, key: &str| {
            let name = refs.get(key)?;
            palette.iter().find(|(n, _)| n == name).map(|(_, c)| *c)
        };
        let mut resolved = ResolvedTheme {
            id: top.id.clone(),
            name: top.name.clone(),
            appearance: top.appearance,
            ui: HashMap::new(),
            syntax: HashMap::new(),
            caret: caret::CaretLook::default(),
            backdrop: backdrop::BackdropSpec::default(),
            floating: backdrop::SurfaceSpec::default(),
            decorations: std::sync::Arc::default(),
            options: std::sync::Arc::from(Vec::new()),
            values: Vec::new(),
        };
        let mut caret = caret::CaretSpec::default();
        let mut decorations = decorations::DecorationsSpec::default();
        for theme in chain.iter().rev() {
            resolved.ui.extend(
                theme.ui.iter().map(|(k, v)| (k.clone(), changed(&theme.ui_refs, k).unwrap_or(*v))),
            );
            resolved.syntax.extend(theme.syntax.iter().map(|(k, v)| {
                let mut style = *v;
                if let Some(c) = changed(&theme.syntax_refs, k) {
                    style.foreground = Some(c);
                }
                (k.clone(), style)
            }));
            caret = theme.caret.over(caret);
            resolved.backdrop = theme.backdrop.over(resolved.backdrop);
            resolved.floating = theme.floating.over(resolved.floating);
            decorations = theme.decorations.clone().over(decorations);
        }
        resolved.caret = caret.look();
        resolved.decorations = std::sync::Arc::new(decorations.resolve(decorations::ResolveWith {
            values: &values,
            palette: &palette,
            dark: top.appearance == Appearance::Dark,
        }));
        resolved.options = std::sync::Arc::from(option_list);
        resolved.values = values;
        Ok(resolved)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
