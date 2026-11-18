//! Commands bounded context: commands, shortcuts (keymap), and their bindings.
//!
//! A shortcut is written as text, e.g. `"CmdOrCtrl+Shift+S"`. `CmdOrCtrl` means Cmd
//! on macOS and Ctrl elsewhere; matching against the pressed key is done by the app, which
//! knows which platform it runs on. A command's shortcuts come in layers (its own, the
//! chosen preset's, the user's) and work somewhere ([`bindings`]).

pub mod bindings;
pub mod menus;

use std::fmt;

pub use bindings::When;
pub use menus::{MenuAt, MenuContext, MenuSectionSpec, Platforms, Requires, SubmenuSpec};

/// A command identifier, e.g. `"file.save"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandId(pub &'static str);

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The key in a shortcut, independent of the physical layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChordKey {
    /// A character, always lowercase (e.g. `'s'`, `'['`).
    Char(char),
    /// A named key: `Up`, `Down`, `Left`, `Right`, `Enter`, `Tab`, `Escape`, `F3`...
    Named(String),
}

/// A key combination.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyChord {
    /// Cmd on macOS, Ctrl elsewhere.
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
    /// Ctrl on macOS (on other platforms Ctrl is `primary`).
    pub control: bool,
    pub key: ChordKey,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid shortcut: `{0}`")]
pub struct ParseChordError(String);

impl KeyChord {
    pub fn parse(s: &str) -> Result<Self, ParseChordError> {
        let err = || ParseChordError(s.to_owned());
        let mut chord = KeyChord {
            primary: false,
            shift: false,
            alt: false,
            control: false,
            key: ChordKey::Char(' '),
        };
        let mut key = None;
        for part in s.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "cmdorctrl" => chord.primary = true,
                "shift" => chord.shift = true,
                "alt" | "option" => chord.alt = true,
                "ctrl" | "control" => chord.control = true,
                "" => return Err(err()),
                "space" if key.is_none() => key = Some(ChordKey::Char(' ')),
                other if key.is_none() => {
                    let mut chars = other.chars();
                    key = Some(match (chars.next(), chars.next()) {
                        (Some(c), None) => ChordKey::Char(c),
                        _ => ChordKey::Named(normalize_named(part)),
                    });
                }
                _ => return Err(err()),
            }
        }
        chord.key = key.ok_or_else(err)?;
        Ok(chord)
    }
}

impl KeyChord {
    /// As a shortcut is written (`CmdOrCtrl+Shift+K`): what a key press is saved as, read
    /// back by [`KeyChord::parse`] to the same chord. On macOS `control` is `Ctrl`.
    pub fn spec(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.primary {
            parts.push("CmdOrCtrl".into());
        }
        if self.control {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        parts.push(match &self.key {
            ChordKey::Char(' ') => "Space".into(),
            ChordKey::Char(c) => c.to_uppercase().collect(),
            ChordKey::Named(name) => name.clone(),
        });
        parts.join("+")
    }

    /// On Windows/Linux, `Ctrl` and `CmdOrCtrl` are the same key.
    pub fn for_platform(mut self, mac: bool) -> Self {
        if !mac && self.control {
            self.control = false;
            self.primary = true;
        }
        self
    }
}

fn normalize_named(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => {
            first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
        }
        None => String::new(),
    }
}

/// A shortcut written for one system only: `mac:Ctrl+G` is macOS' (⌃G there, nothing
/// elsewhere), `pc:CmdOrCtrl+H` Windows' and Linux'. The shortcut as system `mac` (macOS, or
/// the others) reads it; `None` where it isn't one.
pub fn shortcut_on(shortcut: &str, mac: bool) -> Option<&str> {
    if let Some(rest) = shortcut.strip_prefix("mac:") {
        mac.then_some(rest)
    } else if let Some(rest) = shortcut.strip_prefix("pc:") {
        (!mac).then_some(rest)
    } else {
        Some(shortcut)
    }
}

/// A command's description: where it appears in the menu and what shortcuts it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: CommandId,
    pub label: &'static str,
    /// Where it appears in the menu bar (a section, its place there), if anywhere.
    pub menu: Option<MenuAt>,
    /// The first shortcut is the one shown in the menu; one for a single system is written
    /// `mac:…` / `pc:…` ([`shortcut_on`]).
    pub shortcuts: &'static [&'static str],
}

/// Shortcut → command bindings, each working somewhere.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: Vec<(KeyChord, CommandId, When)>,
}

impl Keymap {
    /// Builds the keymap from the commands' own shortcuts (working everywhere); invalid
    /// shortcuts are reported, not silently ignored. `mac` decides what `Ctrl` means: on
    /// Windows/Linux it's the same as `CmdOrCtrl`.
    pub fn from_specs(specs: &[CommandSpec], mac: bool) -> Result<Self, ParseChordError> {
        let mut bindings = Vec::new();
        for spec in specs {
            for shortcut in spec.shortcuts.iter().filter_map(|s| shortcut_on(s, mac)) {
                let chord = KeyChord::parse(shortcut)?.for_platform(mac);
                bindings.push((chord, spec.id, When::Everywhere));
            }
        }
        Ok(Self { bindings })
    }

    /// Builds the keymap from resolved bindings (the layers, the places).
    pub fn from_bindings(
        bindings: &[bindings::Binding],
        mac: bool,
    ) -> Result<Self, ParseChordError> {
        let mut out = Vec::new();
        for binding in bindings {
            for shortcut in &binding.shortcuts {
                let chord = KeyChord::parse(shortcut)?.for_platform(mac);
                out.push((chord, binding.command, binding.when));
            }
        }
        Ok(Self { bindings: out })
    }

    /// The command of `chord` wherever it is bound (the first one).
    pub fn lookup(&self, chord: &KeyChord) -> Option<CommandId> {
        self.bindings.iter().find(|(c, ..)| c == chord).map(|(_, id, _)| *id)
    }

    /// Every command of `chord` where the keys are (several when the user left the same
    /// keys to more of them): the place's bindings if it has any, else those everywhere.
    pub fn lookup_all_in(&self, chord: &KeyChord, active: &[When]) -> Vec<CommandId> {
        let bound = || self.bindings.iter().filter(|(c, ..)| c == chord);
        let place = active.iter().filter(|w| **w != When::Everywhere).find_map(|place| {
            let ids: Vec<CommandId> =
                bound().filter(|(_, _, w)| w == place).map(|(_, id, _)| *id).collect();
            (!ids.is_empty()).then_some(ids)
        });
        let mut ids = place.unwrap_or_else(|| {
            bound().filter(|(_, _, w)| *w == When::Everywhere).map(|(_, id, _)| *id).collect()
        });
        let mut seen = Vec::new();
        ids.retain(|id| {
            let new = !seen.contains(id);
            seen.push(*id);
            new
        });
        ids
    }

    /// The command of `chord` where the keys are (`active`, the innermost first): a binding
    /// for a place wins over one that works everywhere.
    pub fn lookup_in(&self, chord: &KeyChord, active: &[When]) -> Option<CommandId> {
        let bound = || self.bindings.iter().filter(|(c, ..)| c == chord);
        active
            .iter()
            .filter(|w| **w != When::Everywhere)
            .find_map(|place| bound().find(|(_, _, w)| w == place))
            .or_else(|| bound().find(|(_, _, w)| *w == When::Everywhere))
            .map(|(_, id, _)| *id)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
