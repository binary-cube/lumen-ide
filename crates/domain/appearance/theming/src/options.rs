//! A theme's own settings (`[[options]]` in its file): what it lets the user choose about
//! itself — a toggle, a choice, a slider, a color —, shown only while it's in use (in the
//! settings' Appearance and in the studio). A choice changes names of the theme's palette
//! (the interface, the syntax and the decorations written with `$name` follow), a color
//! may too; a slider or a color gives a decoration's option its value (`"@id"`); any of them
//! may show or hide a decoration layer (`when`). The values are the user's, kept per theme
//! in the settings as `"<theme>/<option>=<value>"`.

use kernel::Rgba;

use crate::decorations::{hex, number_text, valid_name};

/// A text in the interface's languages: `(locale, text)`, the first the fallback.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Label(pub Vec<(String, String)>);

impl Label {
    pub fn plain(text: &str) -> Self {
        Self(vec![("en".to_owned(), text.to_owned())])
    }

    /// The text in `locale` (its language without the region too), else in English, else
    /// the first given.
    pub fn get(&self, locale: &str) -> &str {
        let language = locale.split(['-', '_']).next().unwrap_or(locale);
        self.0
            .iter()
            .find(|(l, _)| l == locale)
            .or_else(|| self.0.iter().find(|(l, _)| l == language))
            .or_else(|| self.0.iter().find(|(l, _)| l == "en"))
            .or_else(|| self.0.first())
            .map_or("", |(_, t)| t.as_str())
    }
}

/// One of a choice option's choices: its id, its name, and the palette's names it changes.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionChoice {
    pub id: String,
    pub label: Label,
    pub palette: Vec<(String, Rgba)>,
}

/// What kind of value an option takes.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionKind {
    Toggle,
    Choice(Vec<OptionChoice>),
    /// A number from `min` to `max`, in steps of `step`.
    Slider {
        min: f32,
        max: f32,
        step: f32,
    },
    /// A color; with `palette`, the palette's name it sets.
    Color {
        palette: Option<String>,
    },
}

impl OptionKind {
    pub fn id(&self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Choice(_) => "choice",
            Self::Slider { .. } => "slider",
            Self::Color { .. } => "color",
        }
    }
}

/// An option's value.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionValue {
    Bool(bool),
    Choice(String),
    Number(f32),
    Color(Rgba),
}

/// The most stops a slider has (its step widened if more).
pub const MAX_STOPS: usize = 21;
/// The most options a theme has.
pub const MAX_OPTIONS: usize = 16;

/// One of a theme's own settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeOption {
    pub id: String,
    pub label: Label,
    pub kind: OptionKind,
    pub default: OptionValue,
}

impl ThemeOption {
    /// Whether it takes `value` (its kind, its range, one of its choices).
    pub fn accepts(&self, value: &OptionValue) -> bool {
        match (&self.kind, value) {
            (OptionKind::Toggle, OptionValue::Bool(_)) => true,
            (OptionKind::Choice(choices), OptionValue::Choice(c)) => {
                choices.iter().any(|choice| choice.id == *c)
            }
            (OptionKind::Slider { min, max, .. }, OptionValue::Number(n)) => {
                n.is_finite() && *n >= *min - 1e-4 && *n <= *max + 1e-4
            }
            (OptionKind::Color { .. }, OptionValue::Color(_)) => true,
            _ => false,
        }
    }

    /// A value from its text (as the settings keep it), if it takes it.
    pub fn parse_value(&self, text: &str) -> Option<OptionValue> {
        let text = text.trim();
        let value = match &self.kind {
            OptionKind::Toggle => OptionValue::Bool(match text {
                "true" | "on" | "1" => true,
                "false" | "off" | "0" => false,
                _ => return None,
            }),
            OptionKind::Choice(_) => OptionValue::Choice(text.to_owned()),
            OptionKind::Slider { .. } => OptionValue::Number(text.parse().ok()?),
            OptionKind::Color { .. } => OptionValue::Color(Rgba::from_hex(text)?),
        };
        self.accepts(&value).then_some(value)
    }

    /// The stops a slider offers, from its minimum to its maximum.
    pub fn stops(&self) -> Vec<f32> {
        let OptionKind::Slider { min, max, step } = self.kind else { return Vec::new() };
        let span = (max - min).max(0.0);
        let mut step = step.max(1e-3);
        if span / step + 1.0 > MAX_STOPS as f32 {
            step = span / (MAX_STOPS - 1) as f32;
        }
        let n = (span / step + 1e-3).floor() as usize + 1;
        let mut stops: Vec<f32> = (0..n).map(|i| min + step * i as f32).collect();
        if stops.last().is_some_and(|l| (max - l).abs() > 1e-3) {
            stops.push(max);
        }
        stops
    }

    /// The palette's names `value` changes.
    pub fn palette_of(&self, value: &OptionValue) -> Vec<(String, Rgba)> {
        match (&self.kind, value) {
            (OptionKind::Choice(choices), OptionValue::Choice(c)) => choices
                .iter()
                .find(|choice| choice.id == *c)
                .map_or_else(Vec::new, |choice| choice.palette.clone()),
            (OptionKind::Color { palette: Some(name) }, OptionValue::Color(c)) => {
                vec![(name.clone(), *c)]
            }
            _ => Vec::new(),
        }
    }
}

/// A value as the settings keep it (and a theme file writes a default).
pub fn value_text(value: &OptionValue) -> String {
    match value {
        OptionValue::Bool(b) => b.to_string(),
        OptionValue::Choice(c) => c.clone(),
        OptionValue::Number(n) => number_text(*n),
        OptionValue::Color(c) => hex(*c),
    }
}

/// Each option's value: the user's (`saved`, as text) when the option takes it, else its
/// default. What the user saved for an option the theme doesn't have is left out.
pub fn effective(
    options: &[ThemeOption],
    saved: &[(String, String)],
) -> Vec<(String, OptionValue)> {
    options
        .iter()
        .map(|o| {
            let user = saved.iter().find(|(id, _)| *id == o.id).and_then(|(_, v)| o.parse_value(v));
            (o.id.clone(), user.unwrap_or_else(|| o.default.clone()))
        })
        .collect()
}

/// The palette's names the values change (a later option's over an earlier one's).
pub fn palette_overrides(
    options: &[ThemeOption],
    values: &[(String, OptionValue)],
) -> Vec<(String, Rgba)> {
    let mut out: Vec<(String, Rgba)> = Vec::new();
    for o in options {
        let Some((_, v)) = values.iter().find(|(id, _)| *id == o.id) else { continue };
        for (name, c) in o.palette_of(v) {
            match out.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = c,
                None => out.push((name, c)),
            }
        }
    }
    out
}

/// The options of a theme over those of the theme it extends: one with the same id
/// replaces it (in its place), the new ones follow.
pub fn over(own: &[ThemeOption], under: &[ThemeOption]) -> Vec<ThemeOption> {
    let mut out = under.to_vec();
    for o in own {
        match out.iter_mut().find(|u| u.id == o.id) {
            Some(slot) => *slot = o.clone(),
            None => out.push(o.clone()),
        }
    }
    out.truncate(MAX_OPTIONS);
    out
}

// ---------------------------------------------------------------------------------------
// The values kept in the settings: one list for every theme, `"<theme>/<option>=<value>"`.

/// What `list` holds for `theme`: `(option, value as text)`.
pub fn saved_for(list: &[String], theme: &str) -> Vec<(String, String)> {
    list.iter()
        .filter_map(|entry| {
            let (t, rest) = entry.split_once('/')?;
            let (option, value) = rest.split_once('=')?;
            (t == theme && valid_name(option)).then(|| (option.to_owned(), value.to_owned()))
        })
        .collect()
}

/// `list` with `theme`'s `option` set to `value` (replacing what it held for it).
pub fn with_saved(list: &[String], theme: &str, option: &str, value: &OptionValue) -> Vec<String> {
    let prefix = format!("{theme}/{option}=");
    let mut out: Vec<String> = list.iter().filter(|e| !e.starts_with(&prefix)).cloned().collect();
    out.push(format!("{prefix}{}", value_text(value)));
    out
}

/// Whether an entry of the list is well formed (the setting's validation).
pub fn valid_entry(entry: &str) -> bool {
    entry
        .split_once('/')
        .and_then(|(t, rest)| Some((t, rest.split_once('=')?)))
        .is_some_and(|(t, (option, _))| !t.is_empty() && valid_name(option))
}

#[cfg(test)]
#[path = "../tests/unit/options.rs"]
mod tests;
