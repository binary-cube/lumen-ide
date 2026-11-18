//! User settings (`settings.toml`): one section per subsystem (`[general]`,
//! `[appearance]`, `[search]`, `[modules]`), read and written through the
//! [`SettingsStore`] port. The file format lives in the adapter (`storage`).

use std::collections::BTreeMap;

/// A setting's value (the types the app uses).
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Text(String),
    List(Vec<String>),
}

impl SettingValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[String]> {
        match self {
            Self::List(l) => Some(l),
            _ => None,
        }
    }
}

/// One section: key → value.
pub type Section = BTreeMap<String, SettingValue>;

/// Where user settings are kept (port).
pub trait SettingsStore: std::fmt::Debug + Send + Sync {
    /// The section `name` (missing, or the file is unreadable / invalid: `None`).
    fn section(&self, name: &str) -> Option<Section>;

    /// Changes the section `name` with `change`, keeping the rest of the settings. Must
    /// refuse to write when the existing settings cannot be parsed (writing would lose
    /// every other section).
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()>;

    /// The sections `names` read at once (`None` for a missing one), or `None` when the
    /// settings can't be read as a whole right now (an invalid file, half-written by hand):
    /// what was read before stays.
    fn sections(&self, names: &[String]) -> Option<Vec<Option<Section>>> {
        Some(names.iter().map(|n| self.section(n)).collect())
    }

    /// A single value.
    fn get(&self, section: &str, key: &str) -> Option<SettingValue> {
        self.section(section)?.remove(key)
    }

    /// Sets a single value.
    fn set(&self, section: &str, key: &str, value: SettingValue) -> std::io::Result<()> {
        let mut value = Some(value);
        self.update(section, &mut |s| {
            if let Some(v) = value.take() {
                s.insert(key.to_owned(), v);
            }
        })
    }
}
